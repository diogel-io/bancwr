//! The signing log and audit trail over the API (#96): paging, export and retention. Every route
//! here is the administrator's (`Access::Administrator` in `server.rs`).
//!
//! - `GET /api/bunker/logs?limit=&before=` pages the signing log, newest first. With no
//!   parameters it is the newest 100, as before. `before` is `<timestamp>|<id>` of the last row of
//!   the previous page.
//! - `GET /api/bunker/logs/export?format=csv|jsonl&from=YYYY-MM-DD&to=YYYY-MM-DD` downloads every
//!   signature and audit event in the UTC date range, inclusive, in time order. It is streamed in
//!   batches, never built whole in memory, and recorded as `logs_exported`.
//! - `GET` and `PUT /api/bunker/logs/retention` read and set the retention policy, which is
//!   applied at once and then hourly (`main.rs`).

use axum::{
    body::{Body, Bytes},
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use chrono::{NaiveDate, Utc};
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::audit::{self, date_bounds, Cursor, ExportCursor, ExportRow, Retention};
use crate::connections_api::{api_error, caller, database_error, ApiError};
use crate::proxy_auth::Caller;
use crate::server::LogEntry;
use crate::state::AppState;

/// A page of the signing log when no `limit` is given: the 100 the console always showed.
pub const DEFAULT_PAGE: usize = 100;
/// The most one page may hold.
pub const MAX_PAGE: usize = 500;
/// Rows read per batch while an export streams.
pub const EXPORT_BATCH: usize = 1_000;

/// The export's columns, in order: the CSV header, and the JSONL keys.
pub const EXPORT_COLUMNS: [&str; 12] = [
    "time",
    "source",
    "action",
    "event_kind",
    "event_id",
    "member_pubkey",
    "member_name",
    "connection_id",
    "token_id",
    "client_pubkey",
    "actor_pubkey",
    "detail",
];

#[derive(Debug, Deserialize)]
pub struct LogsQuery {
    pub limit: Option<usize>,
    pub before: Option<String>,
}

/// One page of the signing log
/// GET /api/bunker/logs
pub async fn get_logs(State(state): State<AppState>, Query(query): Query<LogsQuery>) -> Result<Json<Vec<LogEntry>>, ApiError> {
    let limit = query.limit.unwrap_or(DEFAULT_PAGE);
    if limit == 0 || limit > MAX_PAGE {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_limit", "Ask for 1 to 500 rows."));
    }
    let before = match query.before.as_deref().filter(|v| !v.is_empty()) {
        Some(value) => Some(
            Cursor::decode(value)
                .ok_or_else(|| api_error(StatusCode::BAD_REQUEST, "invalid_cursor", "That is not a position in the log."))?,
        ),
        None => None,
    };
    let logs = state.db.logs_page(before.as_ref(), limit).map_err(database_error)?;
    Ok(Json(logs.into_iter().map(Into::into).collect()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Csv,
    Jsonl,
}

impl ExportFormat {
    fn extension(&self) -> &'static str {
        match self {
            ExportFormat::Csv => "csv",
            ExportFormat::Jsonl => "jsonl",
        }
    }

    fn content_type(&self) -> &'static str {
        match self {
            ExportFormat::Csv => "text/csv; charset=utf-8",
            ExportFormat::Jsonl => "application/x-ndjson",
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    pub format: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

fn parse_date(value: Option<&str>) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(value?.trim(), "%Y-%m-%d").ok()
}

/// Export the signing log and audit events
/// GET /api/bunker/logs/export
pub async fn export_logs(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Query(query): Query<ExportQuery>,
) -> Result<Response, ApiError> {
    let format = match query.format.as_deref().unwrap_or("csv") {
        "csv" => ExportFormat::Csv,
        "jsonl" => ExportFormat::Jsonl,
        _ => return Err(api_error(StatusCode::BAD_REQUEST, "invalid_format", "Choose csv or jsonl.")),
    };
    let (Some(from), Some(to)) = (parse_date(query.from.as_deref()), parse_date(query.to.as_deref())) else {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_range", "Give a from and a to date, as YYYY-MM-DD."));
    };
    let Some((start, end)) = date_bounds(from, to) else {
        return Err(api_error(StatusCode::BAD_REQUEST, "invalid_range", "The from date must not be after the to date."));
    };

    let (by, _) = caller(who);
    state.db.record_export(audit::actor(&by), format.extension(), from, to, Utc::now()).map_err(database_error)?;
    info!("Exporting the log from {} to {} as {} (by {})", from, to, format.extension(), if by.is_empty() { "-" } else { &by });

    let header_line = match format {
        ExportFormat::Csv => Some(Bytes::from(format!("{}\r\n", EXPORT_COLUMNS.join(",")))),
        ExportFormat::Jsonl => None,
    };
    struct Export {
        db: std::sync::Arc<crate::db::Database>,
        start: String,
        end: String,
        after: Option<ExportCursor>,
        done: bool,
    }
    let export = Export { db: state.db.clone(), start, end, after: None, done: false };
    let rows = stream::unfold(export, move |mut export| async move {
        if export.done {
            return None;
        }
        match export.db.export_batch(&export.start, &export.end, export.after.as_ref(), EXPORT_BATCH) {
            Ok(batch) => {
                export.done = batch.len() < EXPORT_BATCH;
                export.after = batch.last().map(ExportRow::cursor);
                let mut out = String::new();
                for row in &batch {
                    match format {
                        ExportFormat::Csv => out.push_str(&csv_line(row)),
                        ExportFormat::Jsonl => {
                            out.push_str(&serde_json::to_string(row).unwrap_or_default());
                            out.push('\n');
                        }
                    }
                }
                Some((Ok::<Bytes, std::io::Error>(Bytes::from(out)), export))
            }
            Err(e) => {
                // The status is already sent: end the body early, so the download fails rather
                // than looking complete.
                error!("Log export failed part way: {}", e);
                export.done = true;
                Some((Err(std::io::Error::other("log export failed")), export))
            }
        }
    });
    let body = match header_line {
        Some(header_line) => Body::from_stream(futures_util::StreamExt::chain(stream::iter([Ok(header_line)]), rows)),
        None => Body::from_stream(rows),
    };

    let filename = format!("bancwr-log-{}-{}.{}", from, to, format.extension());
    Ok((
        [
            (header::CONTENT_TYPE, format.content_type().to_string()),
            (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{}\"", filename)),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        body,
    )
        .into_response())
}

/// One CSV record (RFC 4180), with CRLF.
pub fn csv_line(row: &ExportRow) -> String {
    let fields: [Option<String>; 12] = [
        Some(row.time.clone()),
        Some(row.source.as_str().to_string()),
        Some(row.action.clone()),
        row.event_kind.map(|k| k.to_string()),
        row.event_id.clone(),
        row.member_pubkey.clone(),
        row.member_name.clone(),
        row.connection_id.clone(),
        row.token_id.clone(),
        row.client_pubkey.clone(),
        row.actor_pubkey.clone(),
        row.detail.clone(),
    ];
    let mut line = fields.iter().map(|f| csv_field(f.as_deref().unwrap_or(""))).collect::<Vec<_>>().join(",");
    line.push_str("\r\n");
    line
}

/// A CSV cell. A value a spreadsheet would read as a formula gets a leading `'`, so a member's or
/// app's name cannot run one when the export is opened (CWE-1236). Quoted when it needs to be.
pub fn csv_field(value: &str) -> String {
    let value = if value.starts_with(['=', '+', '-', '@', '\t', '\r']) { format!("'{}", value) } else { value.to_string() };
    if value.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value
    }
}

#[derive(Debug, Serialize)]
pub struct RetentionResponse {
    pub max_age_days: Option<u32>,
    pub max_rows: Option<u64>,
    /// The floors a policy may not go under, for the console to explain.
    pub min_age_days: u32,
    pub min_rows: u64,
}

impl From<Retention> for RetentionResponse {
    fn from(r: Retention) -> Self {
        RetentionResponse { max_age_days: r.max_age_days, max_rows: r.max_rows, min_age_days: audit::MIN_AGE_DAYS, min_rows: audit::MIN_ROWS }
    }
}

/// The retention policy
/// GET /api/bunker/logs/retention
pub async fn get_retention(State(state): State<AppState>) -> Result<Json<RetentionResponse>, ApiError> {
    Ok(Json(state.db.log_retention().map_err(database_error)?.into()))
}

/// Set the retention policy, and prune to it at once
/// PUT /api/bunker/logs/retention
pub async fn set_retention(
    State(state): State<AppState>,
    who: Option<Extension<Caller>>,
    Json(retention): Json<Retention>,
) -> Result<Json<RetentionResponse>, ApiError> {
    retention.validate().map_err(|refusal| api_error(StatusCode::BAD_REQUEST, refusal.code(), &refusal.message()))?;
    let (by, _) = caller(who);
    let now = Utc::now();
    state.db.set_log_retention(retention, audit::actor(&by), now).map_err(database_error)?;
    info!(
        "Log retention set to {} days and {} rows (by {})",
        retention.max_age_days.map_or("unlimited".to_string(), |d| d.to_string()),
        retention.max_rows.map_or("unlimited".to_string(), |r| r.to_string()),
        if by.is_empty() { "-" } else { &by }
    );
    prune(&state.db);
    Ok(Json(state.db.log_retention().map_err(database_error)?.into()))
}

/// Applies the retention policy now, logging what it removed. Never fails the caller: a prune
/// that fails is retried on the next hour.
pub fn prune(db: &crate::db::Database) {
    match db.prune_logs(Utc::now()) {
        Ok(pruned) if pruned.signatures > 0 || pruned.audit_events > 0 => info!(
            "Pruned {} signing log entries and {} audit events under the retention policy",
            pruned.signatures, pruned.audit_events
        ),
        Ok(_) => {}
        Err(e) => error!("Failed to prune the logs: {}", e),
    }
}
