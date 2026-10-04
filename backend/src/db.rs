use rusqlite::{params, Connection, OptionalExtension};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::path::Path;
use std::fs;
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::registry::Role;

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Debug, Clone)]
pub struct SigningLog {
    pub id: Uuid,
    pub event_id: String,
    /// The key that asked for the signature: the NIP-46 client's (hex), not the member's.
    pub pubkey: String,
    pub event_kind: u32,
    pub timestamp: DateTime<Utc>,
    /// The vault member the connection was made for (hex) (diogel-io/workspace#38). None for a row
    /// logged before #38 that no connection accounts for.
    pub member_pubkey: Option<String>,
    /// That member's name, read when the log is: None once the member has been removed.
    pub member_name: Option<String>,
    /// The NIP-46 connection that signed.
    pub connection_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TeamMember {
    pub id: Uuid,
    pub name: String,
    /// Lowercase hex (#24). Rows stored before #24 that could not be canonicalised keep their
    /// original value, which never matches a lookup.
    pub pubkey: String,
    /// As stored. Use `role()`: a value outside the three roles can only come from before #24,
    /// and grants nothing.
    pub role: String,
    pub created_at: DateTime<Utc>,
}

impl TeamMember {
    /// The member's role, or `None` for a stored value that is not one of the three.
    pub fn role(&self) -> Option<Role> {
        self.role.parse().ok()
    }
}

/// What `remove_team_member` did.
#[derive(Debug, PartialEq, Eq)]
pub enum RemoveOutcome {
    Removed,
    NotFound,
    /// Refused: the member is the only administrator.
    LastAdministrator,
}

/// What `seed_administrator` did.
#[derive(Debug, PartialEq, Eq)]
pub enum SeedOutcome {
    /// An administrator already exists, so nothing was changed.
    AdministratorExists,
    /// The key was not registered, and was added as an administrator.
    Added,
    /// The key was registered with another role, and was promoted to administrator.
    Promoted,
}

impl Database {
    /// Initialize database connection and create tables if not exist
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        if db_path != ":memory:" {
            let path = Path::new(db_path);
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent)?;
                }
            }
        }

        let mut conn = Connection::open(db_path)?;

        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA busy_timeout = 5000;
             PRAGMA journal_mode = WAL;",
        )?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS signing_logs (
                id TEXT PRIMARY KEY,
                event_id TEXT NOT NULL,
                pubkey TEXT NOT NULL,
                event_kind INTEGER NOT NULL,
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON signing_logs(timestamp DESC);

            CREATE TABLE IF NOT EXISTS config (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS team_members (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                pubkey TEXT NOT NULL UNIQUE,
                role TEXT NOT NULL,
                created_at TEXT NOT NULL
            );",
        )?;

        crate::migrations::run(&mut conn)?;

        info!("Database initialized at {}", db_path);
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Log a signing event. `pubkey` is the requesting client's key; `member_pubkey` and
    /// `connection_id` say which member and connection it signed for (diogel-io/workspace#38).
    pub fn log_signing_event(
        &self,
        event_id: &str,
        pubkey: &str,
        member_pubkey: Option<&str>,
        connection_id: Option<&str>,
        event_kind: u32,
        timestamp: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let id = Uuid::new_v4().to_string();
        let timestamp_str = timestamp.to_rfc3339();
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO signing_logs (id, event_id, pubkey, member_pubkey, connection_id, event_kind, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, event_id, pubkey, member_pubkey, connection_id, event_kind, timestamp_str],
        )?;
        Ok(())
    }

    /// Get recent signing logs (for API)
    pub fn get_recent_logs(&self, limit: usize) -> anyhow::Result<Vec<SigningLog>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT l.id, l.event_id, l.pubkey, l.event_kind, l.timestamp, l.member_pubkey, m.name, l.connection_id
             FROM signing_logs l LEFT JOIN team_members m ON m.pubkey = l.member_pubkey
             ORDER BY l.timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let id_str: String = row.get(0)?;
            let event_kind_raw: i64 = row.get(3)?;
            let timestamp_str: String = row.get(4)?;
            let attribution: (Option<String>, Option<String>, Option<String>) = (row.get(5)?, row.get(6)?, row.get(7)?);
            Ok((id_str, row.get::<_, String>(1)?, row.get::<_, String>(2)?, event_kind_raw, timestamp_str, attribution))
        })?;

        let mut logs = Vec::new();
        for row in rows {
            let (id_str, event_id, pubkey, event_kind_raw, timestamp_str, (member_pubkey, member_name, connection_id)) = row?;
            let id = Uuid::parse_str(&id_str)
                .map_err(|e| anyhow::anyhow!("Malformed UUID in signing_logs.id '{}': {}", id_str, e))?;
            let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
                .map_err(|e| anyhow::anyhow!("Malformed timestamp in signing_logs.timestamp '{}': {}", timestamp_str, e))?
                .with_timezone(&Utc);
            logs.push(SigningLog {
                id,
                event_id,
                pubkey,
                event_kind: event_kind_raw as u32,
                timestamp,
                member_pubkey,
                member_name,
                connection_id,
            });
        }
        Ok(logs)
    }

    /// Store configuration
    pub fn set_config(&self, key: &str, value: &str) -> anyhow::Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO config (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, now],
        )?;
        Ok(())
    }

    /// Get configuration
    pub fn get_config(&self, key: &str) -> anyhow::Result<Option<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare("SELECT value FROM config WHERE key = ?1")?;
        let value = stmt.query_row(params![key], |row| row.get::<_, String>(0)).optional()?;
        Ok(value)
    }

    /// Securely delete the `nsec` and `nsec_file` rows that the removed
    /// `POST /api/bunker/config` used to write (#42). They were never read, and the nsec was
    /// stored in plain text. Returns true if an nsec was found and deleted.
    ///
    /// `secure_delete` makes SQLite overwrite deleted content instead of leaving it in free
    /// space, and the TRUNCATE checkpoint copies the change into the database file and empties the
    /// WAL file, which still held the frames that originally wrote the key. Afterwards the live
    /// `.db` and `-wal` files no longer contain it; copies of the database made earlier still do.
    pub fn purge_stored_key_config(&self) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let had_nsec: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM config WHERE key = 'nsec')",
            [],
            |row| row.get(0),
        )?;

        // Must be on before the DELETE, or the deleted bytes stay where they were.
        conn.execute_batch("PRAGMA secure_delete = ON;")?;
        let removed = conn.execute("DELETE FROM config WHERE key IN ('nsec', 'nsec_file')", [])?;
        if removed > 0 {
            // Returns a (busy, log, checkpointed) row, so query it rather than execute it.
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        }
        Ok(had_nsec)
    }

    /// Reads the audit log's table, for the health check (#27). Cheap: at most one row.
    pub fn ping(&self) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.query_row("SELECT count(*) FROM (SELECT 1 FROM signing_logs LIMIT 1)", [], |_| Ok(()))?;
        Ok(())
    }

    /// Get total number of signatures
    pub fn signature_count(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let count: i64 = conn.query_row("SELECT count(*) FROM signing_logs", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Add a team member. `pubkey` must already be canonical hex (`registry::canonical_pubkey`).
    pub fn add_team_member(
        &self,
        name: &str,
        pubkey: &str,
        role: Role,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id_str, name, pubkey, role.as_str(), now],
        )?;
        Ok(id)
    }

    /// Get all team members
    pub fn get_team_members(&self) -> anyhow::Result<Vec<TeamMember>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, name, pubkey, role, created_at FROM team_members ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;

        let mut members = Vec::new();
        for row in rows {
            members.push(team_member_from_row(row?)?);
        }
        Ok(members)
    }

    /// The member registered under this key, if any. `pubkey` must be canonical hex. The
    /// single-key lookup #25 and #11 authorise against.
    pub fn find_member_by_pubkey(&self, pubkey: &str) -> anyhow::Result<Option<TeamMember>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let row = conn
            .query_row(
                "SELECT id, name, pubkey, role, created_at FROM team_members WHERE pubkey = ?1",
                params![pubkey],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
            )
            .optional()?;
        row.map(team_member_from_row).transpose()
    }

    /// How many members hold the administrator role.
    pub fn administrator_count(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM team_members WHERE role = ?1",
            params![Role::Administrator.as_str()],
            |row| row.get(0),
        )?;
        Ok(count as u64)
    }

    /// First-administrator bootstrap (`BANCWR_ADMIN_PUBKEY`). When no administrator exists, add
    /// this key as one, or promote it if it is already a member. Once any administrator exists it
    /// changes nothing, so it never re-adds a removed key or demotes anyone. `pubkey` must be
    /// canonical hex.
    pub fn seed_administrator(&self, pubkey: &str) -> anyhow::Result<SeedOutcome> {
        if self.administrator_count()? > 0 {
            return Ok(SeedOutcome::AdministratorExists);
        }
        if self.find_member_by_pubkey(pubkey)?.is_some() {
            let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
            conn.execute(
                "UPDATE team_members SET role = ?1 WHERE pubkey = ?2",
                params![Role::Administrator.as_str(), pubkey],
            )?;
            return Ok(SeedOutcome::Promoted);
        }
        self.add_team_member("Administrator (bootstrap)", pubkey, Role::Administrator)?;
        Ok(SeedOutcome::Added)
    }

    /// Remove a team member, unless they are the only administrator: removing them would leave
    /// nobody able to administer the bunker (#25). One statement, so the check cannot race.
    pub fn remove_team_member(&self, id: Uuid) -> anyhow::Result<RemoveOutcome> {
        let id_str = id.to_string();
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let pubkey: Option<String> = tx
            .query_row("SELECT pubkey FROM team_members WHERE id = ?1", params![id_str], |row| row.get(0))
            .optional()?;
        let removed = tx.execute(
            "DELETE FROM team_members WHERE id = ?1 AND NOT (
                role = 'administrator'
                AND (SELECT count(*) FROM team_members WHERE role = 'administrator') = 1
            )",
            params![id_str],
        )?;
        if removed > 0 {
            // Their NIP-46 tokens and connections end with them (#53), in the same transaction.
            if let Some(pubkey) = pubkey {
                let now = Utc::now().to_rfc3339();
                tx.execute(
                    "UPDATE nip46_tokens SET revoked_at = ?2 WHERE for_pubkey = ?1 AND revoked_at IS NULL AND used_at IS NULL",
                    params![pubkey, now],
                )?;
                tx.execute(
                    "UPDATE nip46_connections SET revoked_at = ?2, revoked_reason = 'member_removed'
                     WHERE for_pubkey = ?1 AND revoked_at IS NULL",
                    params![pubkey, now],
                )?;
            }
            tx.commit()?;
            return Ok(RemoveOutcome::Removed);
        }
        drop(tx);
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM team_members WHERE id = ?1)",
            params![id_str],
            |row| row.get(0),
        )?;
        Ok(if exists { RemoveOutcome::LastAdministrator } else { RemoveOutcome::NotFound })
    }
}

fn team_member_from_row(
    (id_str, name, pubkey, role, created_at_str): (String, String, String, String, String),
) -> anyhow::Result<TeamMember> {
    let id = Uuid::parse_str(&id_str)
        .map_err(|e| anyhow::anyhow!("Malformed UUID in team_members.id '{}': {}", id_str, e))?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map_err(|e| anyhow::anyhow!("Malformed timestamp in team_members.created_at '{}': {}", created_at_str, e))?
        .with_timezone(&Utc);
    Ok(TeamMember { id, name, pubkey, role, created_at })
}

// NIP-46 connection tokens and connections (#53). See migrations.rs, migration 3.

/// A connection token as stored: never the secret, only its hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip46Token {
    pub id: String,
    /// The vault member the connection will be attributed to (hex).
    pub for_pubkey: String,
    /// The administrator who issued it (hex; empty only from an unauthenticated test router).
    pub issued_by: String,
    pub label: String,
    /// What a connection made with it may do, e.g. `sign_event:1,sign_event:7`.
    pub perms: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nip46Connection {
    pub id: String,
    /// The client's NIP-46 key (hex), which signs its requests.
    pub client_pubkey: String,
    pub token_id: String,
    pub for_pubkey: String,
    /// What it was granted: the token's perms intersected with what it asked for.
    pub perms: Vec<String>,
    /// NIP-46 client metadata: self-reported, unverified, for display only.
    pub client_name: Option<String>,
    pub client_url: Option<String>,
    pub client_image: Option<String>,
    pub connected_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub revoked_reason: Option<String>,
    /// The key that revoked it (#31), when someone did; empty for logout, replaced, member removed.
    pub revoked_by: Option<String>,
}

/// Unverified client metadata from `connect` (NIP-46 "Client metadata").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientMetadata {
    pub name: Option<String>,
    pub url: Option<String>,
    pub image: Option<String>,
}

/// Why `redeem_nip46_token` refused a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedeemRefusal {
    UnknownSecret,
    Used,
    Revoked,
    Expired,
    /// The member the token was issued for is no longer in the vault.
    MemberRemoved,
    /// The client asked only for permissions the token does not allow.
    NothingGrantable,
}

/// The permissions granted: the token's, or those of them the client asked for. A request that
/// names none (or is empty) gets all of the token's. Only permissions the token holds are ever
/// granted.
pub fn grant(token_perms: &[String], requested: Option<&str>) -> Vec<String> {
    let requested: Vec<&str> = requested.unwrap_or("").split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    if requested.is_empty() {
        return token_perms.to_vec();
    }
    token_perms.iter().filter(|p| requested.contains(&p.as_str())).cloned().collect()
}

fn join_perms(perms: &[String]) -> String {
    perms.join(",")
}

fn split_perms(perms: &str) -> Vec<String> {
    perms.split(',').filter(|p| !p.is_empty()).map(str::to_string).collect()
}

fn parse_time(value: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))
}

fn parse_optional_time(value: Option<String>) -> rusqlite::Result<Option<DateTime<Utc>>> {
    value.as_deref().map(parse_time).transpose()
}

const TOKEN_COLUMNS: &str = "id, for_pubkey, issued_by, label, perms, created_at, expires_at, used_at, revoked_at";
const CONNECTION_COLUMNS: &str = "id, client_pubkey, token_id, for_pubkey, perms, client_name, client_url, client_image, connected_at, last_used_at, revoked_at, revoked_reason, revoked_by";

fn token_from_row(row: &rusqlite::Row) -> rusqlite::Result<Nip46Token> {
    Ok(Nip46Token {
        id: row.get(0)?,
        for_pubkey: row.get(1)?,
        issued_by: row.get(2)?,
        label: row.get(3)?,
        perms: split_perms(&row.get::<_, String>(4)?),
        created_at: parse_time(&row.get::<_, String>(5)?)?,
        expires_at: parse_time(&row.get::<_, String>(6)?)?,
        used_at: parse_optional_time(row.get(7)?)?,
        revoked_at: parse_optional_time(row.get(8)?)?,
    })
}

fn connection_from_row(row: &rusqlite::Row) -> rusqlite::Result<Nip46Connection> {
    Ok(Nip46Connection {
        id: row.get(0)?,
        client_pubkey: row.get(1)?,
        token_id: row.get(2)?,
        for_pubkey: row.get(3)?,
        perms: split_perms(&row.get::<_, String>(4)?),
        client_name: row.get(5)?,
        client_url: row.get(6)?,
        client_image: row.get(7)?,
        connected_at: parse_time(&row.get::<_, String>(8)?)?,
        last_used_at: parse_optional_time(row.get(9)?)?,
        revoked_at: parse_optional_time(row.get(10)?)?,
        revoked_reason: row.get(11)?,
        revoked_by: row.get(12)?,
    })
}

impl Database {
    /// Stores a new token. `secret_hash` is the hex SHA-256 of the secret; the secret itself is
    /// handed to the administrator once and never stored.
    #[allow(clippy::too_many_arguments)]
    pub fn create_nip46_token(
        &self,
        secret_hash: &str,
        for_pubkey: &str,
        issued_by: &str,
        label: &str,
        perms: &[String],
        now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    ) -> anyhow::Result<Nip46Token> {
        let token = Nip46Token {
            id: Uuid::new_v4().to_string(),
            for_pubkey: for_pubkey.to_string(),
            issued_by: issued_by.to_string(),
            label: label.to_string(),
            perms: perms.to_vec(),
            created_at: now,
            expires_at,
            used_at: None,
            revoked_at: None,
        };
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO nip46_tokens (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![token.id, secret_hash, token.for_pubkey, token.issued_by, token.label, join_perms(perms), now.to_rfc3339(), expires_at.to_rfc3339()],
        )?;
        Ok(token)
    }

    /// Every token, newest first. Never the secrets.
    pub fn list_nip46_tokens(&self) -> anyhow::Result<Vec<Nip46Token>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(&format!("SELECT {} FROM nip46_tokens ORDER BY created_at DESC", TOKEN_COLUMNS))?;
        let tokens = stmt.query_map([], token_from_row)?.collect::<Result<_, _>>()?;
        Ok(tokens)
    }

    /// Revokes an unused, unrevoked token. False when there is no such token.
    pub fn revoke_nip46_token(&self, id: &str, now: DateTime<Utc>) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let changed = conn.execute(
            "UPDATE nip46_tokens SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL AND used_at IS NULL",
            params![id, now.to_rfc3339()],
        )?;
        Ok(changed > 0)
    }

    /// Connects `client_pubkey` with the token whose secret hashes to `secret_hash`, in one
    /// transaction: the token is checked (unused, unrevoked, unexpired, its member still in the
    /// vault), marked used, any earlier active connection of the client is replaced, and the new
    /// connection is stored with the granted permissions.
    pub fn redeem_nip46_token(
        &self,
        secret_hash: &str,
        client_pubkey: &str,
        requested_perms: Option<&str>,
        metadata: &ClientMetadata,
        now: DateTime<Utc>,
    ) -> anyhow::Result<Result<Nip46Connection, RedeemRefusal>> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let token = tx
            .query_row(
                &format!("SELECT {} FROM nip46_tokens WHERE secret_hash = ?1", TOKEN_COLUMNS),
                params![secret_hash],
                token_from_row,
            )
            .optional()?;
        let Some(token) = token else { return Ok(Err(RedeemRefusal::UnknownSecret)) };
        if token.revoked_at.is_some() {
            return Ok(Err(RedeemRefusal::Revoked));
        }
        if token.used_at.is_some() {
            return Ok(Err(RedeemRefusal::Used));
        }
        if token.expires_at <= now {
            return Ok(Err(RedeemRefusal::Expired));
        }
        let member: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM team_members WHERE pubkey = ?1)",
            params![token.for_pubkey],
            |row| row.get(0),
        )?;
        if !member {
            return Ok(Err(RedeemRefusal::MemberRemoved));
        }
        let perms = grant(&token.perms, requested_perms);
        if perms.is_empty() {
            return Ok(Err(RedeemRefusal::NothingGrantable));
        }

        tx.execute("UPDATE nip46_tokens SET used_at = ?2 WHERE id = ?1", params![token.id, now.to_rfc3339()])?;
        tx.execute(
            "UPDATE nip46_connections SET revoked_at = ?2, revoked_reason = 'replaced'
             WHERE client_pubkey = ?1 AND revoked_at IS NULL",
            params![client_pubkey, now.to_rfc3339()],
        )?;
        let connection = Nip46Connection {
            id: Uuid::new_v4().to_string(),
            client_pubkey: client_pubkey.to_string(),
            token_id: token.id.clone(),
            for_pubkey: token.for_pubkey.clone(),
            perms,
            client_name: metadata.name.clone(),
            client_url: metadata.url.clone(),
            client_image: metadata.image.clone(),
            connected_at: now,
            last_used_at: None,
            revoked_at: None,
            revoked_reason: None,
            revoked_by: None,
        };
        tx.execute(
            &format!("INSERT INTO nip46_connections ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, NULL, NULL)", CONNECTION_COLUMNS),
            params![
                connection.id, connection.client_pubkey, connection.token_id, connection.for_pubkey, join_perms(&connection.perms),
                connection.client_name, connection.client_url, connection.client_image, now.to_rfc3339()
            ],
        )?;
        tx.commit()?;
        Ok(Ok(connection))
    }

    /// The client's active connection, only while its member is still in the vault.
    pub fn active_nip46_connection(&self, client_pubkey: &str) -> anyhow::Result<Option<Nip46Connection>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let connection = conn
            .query_row(
                &format!(
                    "SELECT {} FROM nip46_connections c
                     WHERE c.client_pubkey = ?1 AND c.revoked_at IS NULL
                       AND EXISTS(SELECT 1 FROM team_members m WHERE m.pubkey = c.for_pubkey)",
                    CONNECTION_COLUMNS.split(", ").map(|c| format!("c.{}", c)).collect::<Vec<_>>().join(", ")
                ),
                params![client_pubkey],
                connection_from_row,
            )
            .optional()?;
        Ok(connection)
    }

    pub fn touch_nip46_connection(&self, id: &str, now: DateTime<Utc>) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute("UPDATE nip46_connections SET last_used_at = ?2 WHERE id = ?1", params![id, now.to_rfc3339()])?;
        Ok(())
    }

    /// Revokes an active connection. False when there is no such active connection.
    pub fn revoke_nip46_connection(&self, id: &str, reason: &str, now: DateTime<Utc>) -> anyhow::Result<bool> {
        self.revoke_nip46_connection_as(id, reason, None, None, now)
    }

    /// Revokes an active connection on someone's behalf (#31): `revoked_by` is recorded, and with
    /// `only_for` set, only a connection made for that key is touched. False when there is no such
    /// active connection, so a caller cannot tell another member's connection from none.
    pub fn revoke_nip46_connection_as(&self, id: &str, reason: &str, revoked_by: Option<&str>, only_for: Option<&str>, now: DateTime<Utc>) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let changed = conn.execute(
            "UPDATE nip46_connections SET revoked_at = ?3, revoked_reason = ?2, revoked_by = ?4
             WHERE id = ?1 AND revoked_at IS NULL AND (?5 IS NULL OR for_pubkey = ?5)",
            params![id, reason, now.to_rfc3339(), revoked_by, only_for],
        )?;
        Ok(changed > 0)
    }

    /// Every connection, newest first, or only those for `for_pubkey`.
    pub fn list_nip46_connections(&self, for_pubkey: Option<&str>) -> anyhow::Result<Vec<Nip46Connection>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {} FROM nip46_connections WHERE ?1 IS NULL OR for_pubkey = ?1 ORDER BY connected_at DESC",
            CONNECTION_COLUMNS
        ))?;
        let rows = stmt.query_map(params![for_pubkey], connection_from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Active connections whose member is still in the vault.
    pub fn active_nip46_connection_count(&self) -> anyhow::Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM nip46_connections c WHERE c.revoked_at IS NULL
               AND EXISTS(SELECT 1 FROM team_members m WHERE m.pubkey = c.for_pubkey)",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}
