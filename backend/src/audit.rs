//! The audit trail (#96): audit events beside the signing log, paging, export and retention.
//!
//! Every change to who may sign is recorded as an audit event in the same transaction as the
//! change, so the record cannot disagree with the database: tokens issued and revoked,
//! connections made and revoked, members added, removed and given another role, the bunker's
//! relays, and the log's own retention. Exports and pruning are recorded too. Event content is
//! never recorded, here or in the signing log.
//!
//! Timestamps in `signing_logs` and `audit_events` are fixed-width UTC (`db_time`), so text order
//! is time order: ranges and paging compare them as text, on the index (migration 9).

use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

use crate::db::{Database, SigningLog};

/// What an audit event records. Each is a fixed value: free text never goes in `action`.
pub mod action {
    pub const TOKEN_ISSUED: &str = "token_issued";
    pub const TOKEN_REVOKED: &str = "token_revoked";
    pub const CONNECTION_CREATED: &str = "connection_created";
    pub const CONNECTION_REVOKED: &str = "connection_revoked";
    pub const MEMBER_ADDED: &str = "member_added";
    pub const MEMBER_REMOVED: &str = "member_removed";
    pub const MEMBER_ROLE_CHANGED: &str = "member_role_changed";
    pub const RELAYS_CHANGED: &str = "relays_changed";
    pub const RETENTION_CHANGED: &str = "retention_changed";
    pub const LOGS_PRUNED: &str = "logs_pruned";
    pub const LOGS_EXPORTED: &str = "logs_exported";
    pub const RESTORED: &str = "restored";
}

/// A timestamp as `signing_logs` and `audit_events` store it: UTC, always six fractional digits
/// and a `Z`, so that comparing two as text compares them in time.
pub fn db_time(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Micros, true)
}

/// One audit event, before it is stored. Unset fields are stored as NULL.
#[derive(Debug, Default, Clone)]
pub struct Audit<'a> {
    pub action: &'a str,
    /// Who did it (hex). None for the bunker itself: pruning, a restore, a NIP-46 client's own
    /// connect or logout, or a router without the guard.
    pub actor: Option<&'a str>,
    /// The member it concerns (hex). Their name is copied when the event is recorded, so it
    /// survives their removal.
    pub member: Option<&'a str>,
    pub connection_id: Option<&'a str>,
    pub token_id: Option<&'a str>,
    pub client_pubkey: Option<&'a str>,
    /// Fixed values: a revocation reason, permissions, a role change, counts. Never event content.
    pub detail: Option<String>,
}

impl<'a> Audit<'a> {
    pub fn new(action: &'a str) -> Self {
        Audit { action, ..Default::default() }
    }
}

/// An empty actor (a router without the guard) is no actor.
pub fn actor(pubkey: &str) -> Option<&str> {
    (!pubkey.is_empty()).then_some(pubkey)
}

/// Stores `event` on `conn`, which callers pass inside the transaction that made the change.
pub fn record(conn: &Connection, event: &Audit, now: DateTime<Utc>) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO audit_events
            (id, action, timestamp, actor_pubkey, member_pubkey, member_name, connection_id, token_id, client_pubkey, detail)
         VALUES (?1, ?2, ?3, ?4, ?5, (SELECT name FROM team_members WHERE pubkey = ?5), ?6, ?7, ?8, ?9)",
        params![
            Uuid::new_v4().to_string(),
            event.action,
            db_time(now),
            event.actor,
            event.member,
            event.connection_id,
            event.token_id,
            event.client_pubkey,
            event.detail,
        ],
    )?;
    Ok(())
}

/// Where a page of the signing log continues: older than this row. Sent to the console as
/// `<timestamp>|<id>`, which it hands back as `before` without reading it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub timestamp: String,
    pub id: String,
}

impl Cursor {
    pub fn of(log: &SigningLog) -> Self {
        Cursor { timestamp: db_time(log.timestamp), id: log.id.to_string() }
    }

    pub fn encode(&self) -> String {
        format!("{}|{}", self.timestamp, self.id)
    }

    /// None for anything `encode` could not have produced.
    pub fn decode(value: &str) -> Option<Self> {
        let (timestamp, id) = value.split_once('|')?;
        let time = DateTime::parse_from_rfc3339(timestamp).ok()?.with_timezone(&Utc);
        let id = Uuid::parse_str(id).ok()?;
        Some(Cursor { timestamp: db_time(time), id: id.to_string() })
    }
}

/// The retention policy (#96). Unset means keep everything, which is the default, so an upgrade
/// never deletes history.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Retention {
    pub max_age_days: Option<u32>,
    pub max_rows: Option<u64>,
}

/// The shortest age a policy may keep, so a typo cannot wipe the trail.
pub const MIN_AGE_DAYS: u32 = 30;
/// The fewest rows a policy may keep, in each of the two tables.
pub const MIN_ROWS: u64 = 10_000;
/// Upper bounds, so the values stay well inside what SQLite and chrono handle.
pub const MAX_AGE_DAYS: u32 = 36_500;
pub const MAX_ROWS: u64 = 1_000_000_000;

const AGE_KEY: &str = "log_retention_days";
const ROWS_KEY: &str = "log_retention_max_rows";

/// Why a policy was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetentionRefusal {
    AgeOutOfRange,
    RowsOutOfRange,
}

impl RetentionRefusal {
    pub fn code(&self) -> &'static str {
        match self {
            RetentionRefusal::AgeOutOfRange => "invalid_max_age",
            RetentionRefusal::RowsOutOfRange => "invalid_max_rows",
        }
    }

    pub fn message(&self) -> String {
        match self {
            RetentionRefusal::AgeOutOfRange => {
                format!("Keep logs for at least {} days, and at most {}, or leave it empty to keep them indefinitely.", MIN_AGE_DAYS, MAX_AGE_DAYS)
            }
            RetentionRefusal::RowsOutOfRange => {
                format!("Keep at least {} rows, or leave it empty for no row limit.", MIN_ROWS)
            }
        }
    }
}

impl Retention {
    pub fn validate(&self) -> Result<(), RetentionRefusal> {
        if self.max_age_days.is_some_and(|days| !(MIN_AGE_DAYS..=MAX_AGE_DAYS).contains(&days)) {
            return Err(RetentionRefusal::AgeOutOfRange);
        }
        if self.max_rows.is_some_and(|rows| !(MIN_ROWS..=MAX_ROWS).contains(&rows)) {
            return Err(RetentionRefusal::RowsOutOfRange);
        }
        Ok(())
    }

    fn describe(&self) -> String {
        let show = |v: Option<String>| v.unwrap_or_else(|| "none".to_string());
        format!(
            "max_age_days={};max_rows={}",
            show(self.max_age_days.map(|d| d.to_string())),
            show(self.max_rows.map(|r| r.to_string()))
        )
    }
}

/// How many rows a prune removed from each table.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pruned {
    pub signatures: u64,
    pub audit_events: u64,
}

/// Rows deleted per statement while pruning, so the database lock is never held for long.
pub const PRUNE_BATCH: u64 = 1_000;

/// Whether a row is a signature or an audit event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Signature,
    Audit,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Signature => "signature",
            Source::Audit => "audit",
        }
    }
}

/// One row of an export: a signature or an audit event, in the same shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportRow {
    pub time: String,
    pub source: Source,
    /// `signed`, or the audit event's action.
    pub action: String,
    pub event_kind: Option<u32>,
    pub event_id: Option<String>,
    pub member_pubkey: Option<String>,
    /// A signature's member as named now (none once removed); an audit event's as named then.
    pub member_name: Option<String>,
    pub connection_id: Option<String>,
    pub token_id: Option<String>,
    /// For a signature, the NIP-46 app that asked.
    pub client_pubkey: Option<String>,
    pub actor_pubkey: Option<String>,
    pub detail: Option<String>,
    /// Not exported: the row's id, and its insertion order within its table, which orders rows
    /// recorded in the same microsecond (one transaction's events) as they happened.
    #[serde(skip)]
    pub id: String,
    #[serde(skip)]
    pub seq: i64,
}

/// The export's order within one timestamp: signatures, then audit events, then as inserted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportCursor {
    pub time: String,
    pub source: i64,
    pub seq: i64,
}

impl ExportRow {
    pub fn cursor(&self) -> ExportCursor {
        ExportCursor {
            time: self.time.clone(),
            source: if self.source == Source::Signature { 0 } else { 1 },
            seq: self.seq,
        }
    }
}

/// An inclusive range of UTC dates, as `db_time` bounds: from the first day's midnight, to before
/// the midnight after the last day.
pub fn date_bounds(from: NaiveDate, to: NaiveDate) -> Option<(String, String)> {
    if from > to {
        return None;
    }
    let start = from.and_hms_opt(0, 0, 0)?.and_utc();
    let end = to.succ_opt()?.and_hms_opt(0, 0, 0)?.and_utc();
    Some((db_time(start), db_time(end)))
}

impl Database {
    /// The retention policy in force.
    pub fn log_retention(&self) -> anyhow::Result<Retention> {
        Ok(Retention {
            max_age_days: self.get_config(AGE_KEY)?.and_then(|v| v.parse().ok()),
            max_rows: self.get_config(ROWS_KEY)?.and_then(|v| v.parse().ok()),
        })
    }

    /// Stores a validated policy and records who changed it, in one transaction. The caller
    /// prunes afterwards.
    pub fn set_log_retention(&self, retention: Retention, actor: Option<&str>, now: DateTime<Utc>) -> anyhow::Result<()> {
        retention.validate().map_err(|refusal| anyhow::anyhow!(refusal.message()))?;
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        for (key, value) in [(AGE_KEY, retention.max_age_days.map(u64::from)), (ROWS_KEY, retention.max_rows)] {
            match value {
                Some(value) => tx.execute(
                    "INSERT INTO config (key, value, updated_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![key, value.to_string(), now.to_rfc3339()],
                )?,
                None => tx.execute("DELETE FROM config WHERE key = ?1", params![key])?,
            };
        }
        record(&tx, &Audit { actor, detail: Some(retention.describe()), ..Audit::new(action::RETENTION_CHANGED) }, now)?;
        tx.commit()?;
        Ok(())
    }

    /// Applies the retention policy to `signing_logs` and `audit_events`, each on its own: rows
    /// older than the age, then the oldest rows over the count. Deletes in batches, taking the
    /// lock once per batch. Records `logs_pruned` when anything went.
    pub fn prune_logs(&self, now: DateTime<Utc>) -> anyhow::Result<Pruned> {
        let retention = self.log_retention()?;
        if retention.validate().is_err() {
            // Only a hand-edited database gets here: prune nothing rather than guess.
            anyhow::bail!("The stored log retention policy is out of range; set it again on the Config page");
        }
        let cutoff = retention.max_age_days.map(|days| db_time(now - Duration::days(days.into())));
        let mut pruned = Pruned::default();
        for (table, count) in [("signing_logs", &mut pruned.signatures), ("audit_events", &mut pruned.audit_events)] {
            if let Some(cutoff) = &cutoff {
                *count += self.delete_batches(|conn, batch| {
                    conn.execute(
                        &format!("DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE timestamp < ?1 LIMIT ?2)"),
                        params![cutoff, batch as i64],
                    )
                })?;
            }
            if let Some(max_rows) = retention.max_rows {
                let total: i64 = self.lock()?.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))?;
                let mut excess = (total as u64).saturating_sub(max_rows);
                while excess > 0 {
                    let batch = excess.min(PRUNE_BATCH);
                    let deleted = self.lock()?.execute(
                        &format!(
                            "DELETE FROM {table} WHERE rowid IN
                             (SELECT rowid FROM {table} ORDER BY timestamp ASC, rowid ASC LIMIT ?1)"
                        ),
                        params![batch as i64],
                    )? as u64;
                    if deleted == 0 {
                        break;
                    }
                    *count += deleted;
                    excess -= deleted;
                }
            }
        }
        if pruned != Pruned::default() {
            let conn = self.lock()?;
            record(
                &conn,
                &Audit {
                    detail: Some(format!("signatures={};audit_events={}", pruned.signatures, pruned.audit_events)),
                    ..Audit::new(action::LOGS_PRUNED)
                },
                now,
            )?;
        }
        Ok(pruned)
    }

    fn delete_batches(&self, delete: impl Fn(&Connection, u64) -> rusqlite::Result<usize>) -> anyhow::Result<u64> {
        let mut total = 0;
        loop {
            let deleted = {
                let conn = self.lock()?;
                delete(&conn, PRUNE_BATCH)? as u64
            };
            total += deleted;
            if deleted < PRUNE_BATCH {
                return Ok(total);
            }
        }
    }

    /// Records an export: who, the format and the range. The rows follow in `export_batch`.
    pub fn record_export(&self, actor: Option<&str>, format: &str, from: NaiveDate, to: NaiveDate, now: DateTime<Utc>) -> anyhow::Result<()> {
        let conn = self.lock()?;
        record(
            &conn,
            &Audit { actor, detail: Some(format!("format={};from={};to={}", format, from, to)), ..Audit::new(action::LOGS_EXPORTED) },
            now,
        )?;
        Ok(())
    }

    /// Up to `limit` signatures and audit events with `start <= time < end` (`date_bounds`), in
    /// time order, after `after` when given.
    pub fn export_batch(&self, start: &str, end: &str, after: Option<&ExportCursor>, limit: usize) -> anyhow::Result<Vec<ExportRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT ts, src, id, action, event_kind, event_id, member_pubkey, member_name, connection_id, token_id,
                    client_pubkey, actor_pubkey, detail, seq
             FROM (
                 SELECT l.timestamp AS ts, 0 AS src, l.rowid AS seq, l.id AS id, 'signed' AS action, l.event_kind AS event_kind,
                        l.event_id AS event_id, l.member_pubkey AS member_pubkey, m.name AS member_name,
                        l.connection_id AS connection_id, NULL AS token_id, l.pubkey AS client_pubkey,
                        NULL AS actor_pubkey, NULL AS detail
                 FROM signing_logs l LEFT JOIN team_members m ON m.pubkey = l.member_pubkey
                 WHERE l.timestamp >= ?1 AND l.timestamp < ?2
                 UNION ALL
                 SELECT a.timestamp, 1, a.rowid, a.id, a.action, NULL, NULL, a.member_pubkey, a.member_name,
                        a.connection_id, a.token_id, a.client_pubkey, a.actor_pubkey, a.detail
                 FROM audit_events a
                 WHERE a.timestamp >= ?1 AND a.timestamp < ?2
             )
             WHERE ?3 IS NULL OR (ts, src, seq) > (?3, ?4, ?5)
             ORDER BY ts, src, seq
             LIMIT ?6",
        )?;
        let (time, source, id) = match after {
            Some(c) => (Some(c.time.as_str()), Some(c.source), Some(c.seq)),
            None => (None, None, None),
        };
        let rows = stmt.query_map(params![start, end, time, source, id, limit as i64], |row| {
            let source: i64 = row.get(1)?;
            Ok(ExportRow {
                time: row.get(0)?,
                source: if source == 0 { Source::Signature } else { Source::Audit },
                id: row.get(2)?,
                action: row.get(3)?,
                event_kind: row.get::<_, Option<i64>>(4)?.map(|k| k as u32),
                event_id: row.get(5)?,
                member_pubkey: row.get(6)?,
                member_name: row.get(7)?,
                connection_id: row.get(8)?,
                token_id: row.get(9)?,
                client_pubkey: row.get(10)?,
                actor_pubkey: row.get(11)?,
                detail: row.get(12)?,
                seq: row.get(13)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every audit event, oldest first. For tests and diagnostics; the console exports them.
    pub fn audit_events(&self) -> anyhow::Result<Vec<ExportRow>> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT timestamp, id, action, member_pubkey, member_name, connection_id, token_id, client_pubkey, actor_pubkey, detail, rowid
             FROM audit_events ORDER BY timestamp, rowid",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ExportRow {
                time: row.get(0)?,
                source: Source::Audit,
                id: row.get(1)?,
                action: row.get(2)?,
                event_kind: None,
                event_id: None,
                member_pubkey: row.get(3)?,
                member_name: row.get(4)?,
                connection_id: row.get(5)?,
                token_id: row.get(6)?,
                client_pubkey: row.get(7)?,
                actor_pubkey: row.get(8)?,
                detail: row.get(9)?,
                seq: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// How many audit events are stored.
    pub fn audit_event_count(&self) -> anyhow::Result<u64> {
        let count: i64 = self.lock()?.query_row("SELECT count(*) FROM audit_events", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// The member's key a token was issued for, if the token exists.
    pub(crate) fn token_member(conn: &Connection, id: &str) -> rusqlite::Result<Option<String>> {
        conn.query_row("SELECT for_pubkey FROM nip46_tokens WHERE id = ?1", params![id], |row| row.get(0)).optional()
    }
}
