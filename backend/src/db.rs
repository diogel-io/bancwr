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
    pub pubkey: String,
    pub event_kind: u32,
    pub timestamp: DateTime<Utc>,
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

    /// Log a signing event
    pub fn log_signing_event(
        &self,
        event_id: &str,
        pubkey: &str,
        event_kind: u32,
        timestamp: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let id = Uuid::new_v4().to_string();
        let timestamp_str = timestamp.to_rfc3339();
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO signing_logs (id, event_id, pubkey, event_kind, timestamp) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, event_id, pubkey, event_kind, timestamp_str],
        )?;
        Ok(())
    }

    /// Get recent signing logs (for API)
    pub fn get_recent_logs(&self, limit: usize) -> anyhow::Result<Vec<SigningLog>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT id, event_id, pubkey, event_kind, timestamp FROM signing_logs ORDER BY timestamp DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let id_str: String = row.get(0)?;
            let event_kind_raw: i64 = row.get(3)?;
            let timestamp_str: String = row.get(4)?;
            Ok((id_str, row.get::<_, String>(1)?, row.get::<_, String>(2)?, event_kind_raw, timestamp_str))
        })?;

        let mut logs = Vec::new();
        for row in rows {
            let (id_str, event_id, pubkey, event_kind_raw, timestamp_str) = row?;
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
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let removed = conn.execute(
            "DELETE FROM team_members WHERE id = ?1 AND NOT (
                role = 'administrator'
                AND (SELECT count(*) FROM team_members WHERE role = 'administrator') = 1
            )",
            params![id_str],
        )?;
        if removed > 0 {
            return Ok(RemoveOutcome::Removed);
        }
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
