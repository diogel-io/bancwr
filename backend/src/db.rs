use rusqlite::{params, Connection, OptionalExtension};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use std::path::Path;
use std::fs;
use std::sync::{Arc, Mutex};
use tracing::info;

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
    pub pubkey: String,
    pub role: String, // "admin", "signer", "viewer"
    pub created_at: DateTime<Utc>,
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

        let conn = Connection::open(db_path)?;

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

    /// Get total number of signatures
    pub fn signature_count(&self) -> anyhow::Result<u64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let count: i64 = conn.query_row("SELECT count(*) FROM signing_logs", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    /// Add a team member
    pub fn add_team_member(
        &self,
        name: &str,
        pubkey: &str,
        role: &str,
    ) -> anyhow::Result<Uuid> {
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let now = Utc::now().to_rfc3339();
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id_str, name, pubkey, role, now],
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
            let (id_str, name, pubkey, role, created_at_str) = row?;
            let id = Uuid::parse_str(&id_str)
                .map_err(|e| anyhow::anyhow!("Malformed UUID in team_members.id '{}': {}", id_str, e))?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| anyhow::anyhow!("Malformed timestamp in team_members.created_at '{}': {}", created_at_str, e))?
                .with_timezone(&Utc);
            members.push(TeamMember { id, name, pubkey, role, created_at });
        }
        Ok(members)
    }

    /// Remove a team member. Returns false when no member has that id.
    pub fn remove_team_member(&self, id: Uuid) -> anyhow::Result<bool> {
        let id_str = id.to_string();
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let removed = conn.execute(
            "DELETE FROM team_members WHERE id = ?1",
            params![id_str],
        )?;
        Ok(removed > 0)
    }
}
