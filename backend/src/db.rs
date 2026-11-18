use rusqlite::{params, Connection, OptionalExtension};
use chrono::{DateTime, SubsecRound, Utc};
use uuid::Uuid;
use std::path::Path;
use std::fs;
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::info;

use crate::audit::{self, action, db_time, Audit, Cursor};
use crate::rate_limit::RateLimit;
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
    /// The identity that signed (#103), hex: the primary's for a row that names none. None only
    /// before the primary has been recorded.
    pub identity_pubkey: Option<String>,
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

/// What `change_member_role` did.
#[derive(Debug, PartialEq, Eq)]
pub enum RoleChangeOutcome {
    Changed,
    NotFound,
    /// Refused: the member is the only administrator, and the new role is not.
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
             PRAGMA trusted_schema = OFF;
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

    /// The connection, for the modules that keep their queries beside their types (`audit`).
    pub(crate) fn lock(&self) -> anyhow::Result<MutexGuard<'_, Connection>> {
        self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))
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
        self.log_signing_event_for(event_id, pubkey, member_pubkey, connection_id, None, event_kind, timestamp)
    }

    /// `log_signing_event`, naming the identity that signed (#103; None: the primary).
    #[allow(clippy::too_many_arguments)]
    pub fn log_signing_event_for(
        &self,
        event_id: &str,
        pubkey: &str,
        member_pubkey: Option<&str>,
        connection_id: Option<&str>,
        identity: Option<&str>,
        event_kind: u32,
        timestamp: DateTime<Utc>,
    ) -> anyhow::Result<()> {
        let id = Uuid::new_v4().to_string();
        // Fixed width, so text order is time order (#96).
        let timestamp_str = db_time(timestamp);
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "INSERT INTO signing_logs (id, event_id, pubkey, member_pubkey, connection_id, event_kind, timestamp, identity_pubkey)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, event_id, pubkey, member_pubkey, connection_id, event_kind, timestamp_str, identity],
        )?;
        Ok(())
    }

    /// Get recent signing logs (for API)
    pub fn get_recent_logs(&self, limit: usize) -> anyhow::Result<Vec<SigningLog>> {
        self.recent_logs(None, None, limit)
    }

    /// One page of every member's signatures, newest first: the newest `limit`, or the `limit`
    /// before `before` (#96).
    pub fn logs_page(&self, before: Option<&Cursor>, limit: usize) -> anyhow::Result<Vec<SigningLog>> {
        self.recent_logs(None, before, limit)
    }

    /// The recent signatures made for one member (#77's `GET /api/bunker/logs/mine`), by the
    /// member each was attributed to (diogel-io/workspace#38), not the client that asked. A row
    /// logged before #38 that no connection accounts for has no member, so it is never included.
    pub fn get_recent_logs_for_member(&self, member_pubkey: &str, limit: usize) -> anyhow::Result<Vec<SigningLog>> {
        self.recent_logs(Some(member_pubkey), None, limit)
    }

    /// Every member's recent signatures, or only `member_pubkey`'s.
    /// Every member's recent signatures, or only `member_pubkey`'s, older than `before` if given.
    /// Ties on the timestamp are broken by id, so paging never skips or repeats a row.
    fn recent_logs(&self, member_pubkey: Option<&str>, before: Option<&Cursor>, limit: usize) -> anyhow::Result<Vec<SigningLog>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare(
            "SELECT l.id, l.event_id, l.pubkey, l.event_kind, l.timestamp, l.member_pubkey, m.name, l.connection_id,
                    COALESCE(l.identity_pubkey, (SELECT pubkey FROM identities WHERE is_primary = 1))
             FROM signing_logs l LEFT JOIN team_members m ON m.pubkey = l.member_pubkey
             WHERE (?2 IS NULL OR l.member_pubkey = ?2)
               AND (?3 IS NULL OR l.timestamp < ?3 OR (l.timestamp = ?3 AND l.id < ?4))
             ORDER BY l.timestamp DESC, l.id DESC LIMIT ?1",
        )?;
        let (before_time, before_id) = match before {
            Some(cursor) => (Some(cursor.timestamp.as_str()), Some(cursor.id.as_str())),
            None => (None, None),
        };
        let rows = stmt.query_map(params![limit as i64, member_pubkey, before_time, before_id], |row| {
            let id_str: String = row.get(0)?;
            let event_kind_raw: i64 = row.get(3)?;
            let timestamp_str: String = row.get(4)?;
            let attribution: (Option<String>, Option<String>, Option<String>, Option<String>) = (row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?);
            Ok((id_str, row.get::<_, String>(1)?, row.get::<_, String>(2)?, event_kind_raw, timestamp_str, attribution))
        })?;

        let mut logs = Vec::new();
        for row in rows {
            let (id_str, event_id, pubkey, event_kind_raw, timestamp_str, (member_pubkey, member_name, connection_id, identity_pubkey)) = row?;
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
                identity_pubkey,
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
        self.add_team_member_as(name, pubkey, role, None)
    }

    /// Adds a team member and records who added them (#96), in one transaction.
    pub fn add_team_member_as(&self, name: &str, pubkey: &str, role: Role, actor: Option<&str>) -> anyhow::Result<Uuid> {
        let id = Uuid::new_v4();
        let id_str = id.to_string();
        let now = Utc::now();
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO team_members (id, name, pubkey, role, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id_str, name, pubkey, role.as_str(), now.to_rfc3339()],
        )?;
        audit::record(
            &tx,
            &Audit { actor, member: Some(pubkey), detail: Some(format!("role={}", role.as_str())), ..Audit::new(action::MEMBER_ADDED) },
            now,
        )?;
        tx.commit()?;
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

    /// The member with this id, if any: the id the console addresses a member by (#99).
    pub fn find_member_by_id(&self, id: Uuid) -> anyhow::Result<Option<TeamMember>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let row = conn
            .query_row(
                "SELECT id, name, pubkey, role, created_at FROM team_members WHERE id = ?1",
                params![id.to_string()],
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
            self.change_member_role(pubkey, Role::Administrator)?;
            return Ok(SeedOutcome::Promoted);
        }
        self.add_team_member("Administrator (bootstrap)", pubkey, Role::Administrator)?;
        Ok(SeedOutcome::Added)
    }

    /// Changes a member's role: the one place a stored role changes after the member is added.
    /// An administrator changes it from the team page (`PATCH /api/bunker/team/:id`, #99), and the
    /// first-administrator bootstrap promotes. `pubkey` must be canonical hex.
    ///
    /// A member who becomes a viewer cannot sign (#77): in the same transaction, their active
    /// NIP-46 connections are revoked (`role_changed`) and their unused tokens with them. The only
    /// administrator is never demoted, for the reason `remove_team_member` gives.
    pub fn change_member_role(&self, pubkey: &str, role: Role) -> anyhow::Result<RoleChangeOutcome> {
        self.change_member_role_as(pubkey, role, None)
    }

    /// `change_member_role`, recording who made the change (#96).
    pub fn change_member_role_as(&self, pubkey: &str, role: Role, actor: Option<&str>) -> anyhow::Result<RoleChangeOutcome> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let from: Option<String> =
            tx.query_row("SELECT role FROM team_members WHERE pubkey = ?1", params![pubkey], |row| row.get(0)).optional()?;
        let changed = tx.execute(
            "UPDATE team_members SET role = ?2 WHERE pubkey = ?1 AND NOT (
                ?2 != 'administrator' AND role = 'administrator'
                AND (SELECT count(*) FROM team_members WHERE role = 'administrator') = 1
            )",
            params![pubkey, role.as_str()],
        )?;
        if changed == 0 {
            let exists: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM team_members WHERE pubkey = ?1)", params![pubkey], |row| row.get(0))?;
            return Ok(if exists { RoleChangeOutcome::LastAdministrator } else { RoleChangeOutcome::NotFound });
        }
        let now = Utc::now();
        // Recorded only when the role is different: setting the same role changes nothing.
        if from.as_deref() != Some(role.as_str()) {
            audit::record(
                &tx,
                &Audit {
                    actor,
                    member: Some(pubkey),
                    detail: Some(format!("from={};to={}", from.as_deref().unwrap_or(""), role.as_str())),
                    ..Audit::new(action::MEMBER_ROLE_CHANGED)
                },
                now,
            )?;
        }
        if !role.can_sign() {
            let (revoked, _) = revoke_member_access(&tx, pubkey, "role_changed", now)?;
            if revoked > 0 {
                info!("Revoked {} NIP-46 connections of {}: a {} cannot sign (#77)", revoked, pubkey, role);
            }
        }
        tx.commit()?;
        Ok(RoleChangeOutcome::Changed)
    }

    /// Remove a team member, unless they are the only administrator: removing them would leave
    /// nobody able to administer the bunker (#25). One statement, so the check cannot race.
    pub fn remove_team_member(&self, id: Uuid) -> anyhow::Result<RemoveOutcome> {
        self.remove_team_member_as(id, None)
    }

    /// `remove_team_member`, recording who removed them (#96).
    pub fn remove_team_member_as(&self, id: Uuid, actor: Option<&str>) -> anyhow::Result<RemoveOutcome> {
        let id_str = id.to_string();
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let member: Option<(String, String)> = tx
            .query_row("SELECT pubkey, role FROM team_members WHERE id = ?1", params![id_str], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        let now = Utc::now();
        // Recorded first, while the member's name can still be copied; rolled back if refused.
        if let Some((pubkey, role)) = &member {
            audit::record(
                &tx,
                &Audit { actor, member: Some(pubkey), detail: Some(format!("role={}", role)), ..Audit::new(action::MEMBER_REMOVED) },
                now,
            )?;
        }
        let removed = tx.execute(
            "DELETE FROM team_members WHERE id = ?1 AND NOT (
                role = 'administrator'
                AND (SELECT count(*) FROM team_members WHERE role = 'administrator') = 1
            )",
            params![id_str],
        )?;
        if removed > 0 {
            // Their NIP-46 tokens and connections end with them (#53), in the same transaction.
            if let Some((pubkey, _)) = &member {
                revoke_member_access(&tx, pubkey, "member_removed", now)?;
                // And their identity grants (#103), so a key added again later starts with none.
                tx.execute("DELETE FROM identity_access WHERE member_pubkey = ?1", params![pubkey])?;
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

/// Revokes a member's active connections (with `reason`) and unused tokens, recording each, inside
/// the caller's transaction. Returns how many connections and tokens it revoked.
fn revoke_member_access(conn: &Connection, pubkey: &str, reason: &str, now: DateTime<Utc>) -> rusqlite::Result<(usize, usize)> {
    let connections: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT id, client_pubkey FROM nip46_connections WHERE for_pubkey = ?1 AND revoked_at IS NULL")?;
        let rows = stmt.query_map(params![pubkey], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let tokens: Vec<String> = {
        let mut stmt = conn.prepare("SELECT id FROM nip46_tokens WHERE for_pubkey = ?1 AND revoked_at IS NULL AND used_at IS NULL")?;
        let rows = stmt.query_map(params![pubkey], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    conn.execute(
        "UPDATE nip46_tokens SET revoked_at = ?2 WHERE for_pubkey = ?1 AND revoked_at IS NULL AND used_at IS NULL",
        params![pubkey, now.to_rfc3339()],
    )?;
    conn.execute(
        "UPDATE nip46_connections SET revoked_at = ?2, revoked_reason = ?3 WHERE for_pubkey = ?1 AND revoked_at IS NULL",
        params![pubkey, now.to_rfc3339(), reason],
    )?;
    for (id, client) in &connections {
        audit::record(
            conn,
            &Audit {
                member: Some(pubkey),
                connection_id: Some(id),
                client_pubkey: Some(client),
                detail: Some(format!("reason={}", reason)),
                ..Audit::new(action::CONNECTION_REVOKED)
            },
            now,
        )?;
    }
    for id in &tokens {
        audit::record(
            conn,
            &Audit { member: Some(pubkey), token_id: Some(id), detail: Some(format!("reason={}", reason)), ..Audit::new(action::TOKEN_REVOKED) },
            now,
        )?;
    }
    Ok((connections.len(), tokens.len()))
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
    /// What a connection made with it may do, e.g. `sign_event:1,sign_event:7,nip44_encrypt`.
    pub perms: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    /// How long a connection made with it lasts, from when the app connects (#98). None: until
    /// revoked.
    pub connection_lifetime_hours: Option<u32>,
    /// The rate limit of a connection made with it (#101). None: `RateLimit::DEFAULT`.
    pub rate_limit: Option<RateLimit>,
    /// Which of its `sign_event:<kind>` permissions need an administrator's approval (#102).
    pub approval_perms: Vec<String>,
    /// The identity a connection made with it signs as (#103), hex. None: the primary.
    pub identity_pubkey: Option<String>,
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
    /// When it stops working (#98): the token's lifetime after it connected. None: until revoked.
    pub expires_at: Option<DateTime<Utc>>,
    /// How fast it may use the bunker's key (#101), copied from its token. None: `RateLimit::DEFAULT`.
    pub rate_limit: Option<RateLimit>,
    /// Which of its `sign_event:<kind>` permissions are held for an administrator's approval
    /// (#102): the token's, of those it was granted.
    pub approval_perms: Vec<String>,
    /// The identity it signs as (#103), hex, copied from its token. None: the primary.
    pub identity_pubkey: Option<String>,
}

impl Nip46Connection {
    /// The limit in force: its own, or the default.
    pub fn effective_rate_limit(&self) -> RateLimit {
        RateLimit::effective(self.rate_limit)
    }

    /// Whether signing `kind` waits for an administrator's approval (#102).
    pub fn needs_approval(&self, kind: u16) -> bool {
        self.approval_perms.contains(&crate::nip46::sign_permission(kind))
    }
}

/// What a token gives the connection made with it, beyond its permissions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenOptions {
    /// How long the connection lasts once made (#98). None: until revoked.
    pub connection_lifetime_hours: Option<u32>,
    /// How fast it may use the bunker's key (#101). None: `RateLimit::DEFAULT`.
    pub rate_limit: Option<RateLimit>,
    /// Which kinds are held for an administrator's approval (#102), as `sign_event:<kind>`. Each
    /// must be among the token's perms. Empty: none.
    pub approval_perms: Vec<String>,
    /// The identity the connection signs as (#103), hex. None: the primary. The caller checks the
    /// member may use it.
    pub identity_pubkey: Option<String>,
}

/// Unverified client metadata from `connect` (NIP-46 "Client metadata").
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClientMetadata {
    pub name: Option<String>,
    pub url: Option<String>,
    pub image: Option<String>,
}

/// The most new NIP-46 sender keys recorded in any hour (`Database::record_nip46_sender`).
pub const NEW_NIP46_SENDERS_PER_HOUR: i64 = 1000;

/// Why `redeem_nip46_token` refused a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedeemRefusal {
    UnknownSecret,
    Used,
    Revoked,
    Expired,
    /// The member the token was issued for is no longer in the vault.
    MemberRemoved,
    /// The member the token was issued for is now a viewer, who cannot sign (#77).
    MemberCannotSign,
    /// The client asked only for permissions the token does not allow.
    NothingGrantable,
    /// The token's member is no longer granted the token's identity (#103).
    NoIdentityAccess,
    /// The token is for another identity than the `connect` was addressed to (#103).
    OtherIdentity,
}

impl RedeemRefusal {
    /// Whether this refusal comes after the secret matched a live token, which it then uses up: a
    /// `connect` refused for its identity, its member or its permissions can never be redeemed
    /// later, with these parameters or any others. Unknown, used, revoked and expired tokens are
    /// left as they are.
    pub fn uses_up_token(self) -> bool {
        self.reason().is_some()
    }

    /// The `refusal=` value recorded when the token is used up.
    fn reason(self) -> Option<&'static str> {
        match self {
            RedeemRefusal::UnknownSecret | RedeemRefusal::Used | RedeemRefusal::Revoked | RedeemRefusal::Expired => None,
            RedeemRefusal::MemberRemoved => Some("member_removed"),
            RedeemRefusal::MemberCannotSign => Some("member_cannot_sign"),
            RedeemRefusal::NothingGrantable => Some("nothing_grantable"),
            RedeemRefusal::NoIdentityAccess => Some("no_identity_access"),
            RedeemRefusal::OtherIdentity => Some("other_identity"),
        }
    }
}

/// Why a live token's `connect` is refused for its identity or its member (#77, #103), if it is.
fn member_refusal(conn: &Connection, token: &Nip46Token, identity: Option<&str>) -> rusqlite::Result<Option<RedeemRefusal>> {
    if token.identity_pubkey.as_deref() != identity {
        return Ok(Some(RedeemRefusal::OtherIdentity));
    }
    let role: Option<String> =
        conn.query_row("SELECT role FROM team_members WHERE pubkey = ?1", params![token.for_pubkey], |row| row.get(0)).optional()?;
    let Some(role) = role else { return Ok(Some(RedeemRefusal::MemberRemoved)) };
    if !role.parse::<Role>().is_ok_and(|role| role.can_sign()) {
        return Ok(Some(RedeemRefusal::MemberCannotSign));
    }
    if !may_use_identity(conn, &token.for_pubkey, token.identity_pubkey.as_deref())? {
        return Ok(Some(RedeemRefusal::NoIdentityAccess));
    }
    Ok(None)
}

/// Marks `token` used by a `connect` from `client_pubkey` that was refused for `refusal`, inside
/// the caller's transaction, and records it as `token_revoked`, `reason=connect_refused`.
fn use_up_refused_token(conn: &Connection, token: &Nip46Token, client_pubkey: &str, refusal: RedeemRefusal, now: DateTime<Utc>) -> rusqlite::Result<()> {
    conn.execute("UPDATE nip46_tokens SET used_at = ?2 WHERE id = ?1 AND used_at IS NULL", params![token.id, now.to_rfc3339()])?;
    audit::record(
        conn,
        &Audit {
            member: Some(&token.for_pubkey),
            token_id: Some(&token.id),
            client_pubkey: Some(client_pubkey),
            identity: token.identity_pubkey.as_deref(),
            detail: Some(format!("reason=connect_refused;refusal={}", refusal.reason().unwrap_or("other"))),
            ..Audit::new(action::TOKEN_REVOKED)
        },
        now,
    )
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

/// An optional perms column: NULL when there are none.
fn optional_perms(perms: &[String]) -> Option<String> {
    (!perms.is_empty()).then(|| join_perms(perms))
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

/// A condition on `nip46_connections c`: its member may use its identity (#103). Administrators
/// may use every identity; with no identity but the primary, everyone may; otherwise the member
/// needs a grant. Matches `may_use_identity`.
const IDENTITY_ALLOWED: &str = "(EXISTS(SELECT 1 FROM team_members ma WHERE ma.pubkey = c.for_pubkey AND ma.role = 'administrator')
       OR NOT EXISTS(SELECT 1 FROM identities WHERE is_primary = 0)
       OR EXISTS(SELECT 1 FROM identity_access a WHERE a.member_pubkey = c.for_pubkey AND a.identity_pubkey = COALESCE(c.identity_pubkey, '')))";

/// The key `identity_access` stores an identity under: its pubkey, or '' for the primary.
fn access_key(identity: Option<&str>) -> &str {
    identity.unwrap_or("")
}

/// Whether `member` may use `identity` (#103), as `IDENTITY_ALLOWED` decides it.
fn may_use_identity(conn: &Connection, member: &str, identity: Option<&str>) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM team_members WHERE pubkey = ?1 AND role = 'administrator')
             OR NOT EXISTS(SELECT 1 FROM identities WHERE is_primary = 0)
             OR EXISTS(SELECT 1 FROM identity_access WHERE member_pubkey = ?1 AND identity_pubkey = ?2)",
        params![member, access_key(identity)],
        |row| row.get(0),
    )
}

/// Appended inside `EXISTS(SELECT 1 FROM team_members m WHERE ...)`: the member's role may sign
/// (#77). Matches `Role::can_sign`.
const MEMBER_CAN_SIGN: &str = "AND m.role IN ('administrator', 'signer')";

const TOKEN_COLUMNS: &str =
    "id, for_pubkey, issued_by, label, perms, created_at, expires_at, used_at, revoked_at, connection_lifetime_hours, rate_per_minute, rate_burst, approval_perms, identity_pubkey";
const CONNECTION_COLUMNS: &str = "id, client_pubkey, token_id, for_pubkey, perms, client_name, client_url, client_image, connected_at, last_used_at, revoked_at, revoked_reason, revoked_by, expires_at, rate_per_minute, rate_burst, approval_perms, identity_pubkey";

/// A stored rate limit (#101): both columns NULL is none, so the default; one NULL takes the
/// default's part, as issuing does. Values were checked when the token was issued.
fn stored_rate_limit(per_minute: Option<u32>, burst: Option<u32>) -> Option<RateLimit> {
    if per_minute.is_none() && burst.is_none() {
        return None;
    }
    Some(RateLimit { per_minute: per_minute.unwrap_or(RateLimit::DEFAULT.per_minute), burst: burst.unwrap_or(RateLimit::DEFAULT.burst) })
}

/// Appended to a query on `nip46_connections c`, with parameter `?<param>` bound to `db_time(now)`:
/// the connection has not reached its expiry (#98). `expires_at` is fixed-width, so text order is
/// time order.
fn not_expired(param: usize) -> String {
    format!("AND (c.expires_at IS NULL OR c.expires_at > ?{})", param)
}

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
        connection_lifetime_hours: row.get(9)?,
        rate_limit: stored_rate_limit(row.get(10)?, row.get(11)?),
        approval_perms: split_perms(&row.get::<_, Option<String>>(12)?.unwrap_or_default()),
        identity_pubkey: row.get(13)?,
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
        expires_at: parse_optional_time(row.get(13)?)?,
        rate_limit: stored_rate_limit(row.get(14)?, row.get(15)?),
        approval_perms: split_perms(&row.get::<_, Option<String>>(16)?.unwrap_or_default()),
        identity_pubkey: row.get(17)?,
    })
}

impl Database {
    /// Stores a new token whose connection lasts until revoked. `secret_hash` is the hex SHA-256
    /// of the secret; the secret itself is handed to the administrator once and never stored.
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
        self.create_nip46_token_with(secret_hash, for_pubkey, issued_by, label, perms, now, expires_at, TokenOptions::default())
    }

    /// Stores a new token, as `create_nip46_token`, whose connection lasts `connection_lifetime_hours`
    /// from when the app connects (#98), or until revoked when None.
    #[allow(clippy::too_many_arguments)]
    pub fn create_nip46_token_with_lifetime(
        &self,
        secret_hash: &str,
        for_pubkey: &str,
        issued_by: &str,
        label: &str,
        perms: &[String],
        now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        connection_lifetime_hours: Option<u32>,
    ) -> anyhow::Result<Nip46Token> {
        let options = TokenOptions { connection_lifetime_hours, ..TokenOptions::default() };
        self.create_nip46_token_with(secret_hash, for_pubkey, issued_by, label, perms, now, expires_at, options)
    }

    /// Stores a new token, as `create_nip46_token`, with what a connection made with it gets beyond
    /// its permissions: its lifetime (#98) and its rate limit (#101).
    #[allow(clippy::too_many_arguments)]
    pub fn create_nip46_token_with(
        &self,
        secret_hash: &str,
        for_pubkey: &str,
        issued_by: &str,
        label: &str,
        perms: &[String],
        now: DateTime<Utc>,
        expires_at: DateTime<Utc>,
        options: TokenOptions,
    ) -> anyhow::Result<Nip46Token> {
        let TokenOptions { connection_lifetime_hours, rate_limit, approval_perms, identity_pubkey } = options;
        let token = Nip46Token {
            id: Uuid::new_v4().to_string(),
            for_pubkey: for_pubkey.to_string(),
            issued_by: issued_by.to_string(),
            label: label.to_string(),
            perms: perms.to_vec(),
            created_at: now,
            expires_at,
            connection_lifetime_hours,
            rate_limit,
            approval_perms: approval_perms.clone(),
            identity_pubkey: identity_pubkey.clone(),
            used_at: None,
            revoked_at: None,
        };
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO nip46_tokens
                (id, secret_hash, for_pubkey, issued_by, label, perms, created_at, expires_at, connection_lifetime_hours, rate_per_minute, rate_burst,
                 approval_perms, identity_pubkey)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                token.id, secret_hash, token.for_pubkey, token.issued_by, token.label, join_perms(perms), now.to_rfc3339(), expires_at.to_rfc3339(),
                connection_lifetime_hours, rate_limit.map(|l| l.per_minute), rate_limit.map(|l| l.burst), optional_perms(&approval_perms),
                identity_pubkey
            ],
        )?;
        audit::record(
            &tx,
            &Audit {
                actor: audit::actor(issued_by),
                member: Some(for_pubkey),
                token_id: Some(&token.id),
                identity: token.identity_pubkey.as_deref(),
                detail: Some({
                    let mut detail = format!("perms={};expires_at={}", join_perms(perms), db_time(expires_at));
                    if let Some(hours) = connection_lifetime_hours {
                        detail.push_str(&format!(";connection_lifetime_hours={}", hours));
                    }
                    if let Some(limit) = rate_limit.filter(|l| *l != RateLimit::DEFAULT) {
                        detail.push_str(&format!(";rate_per_minute={};rate_burst={}", limit.per_minute, limit.burst));
                    }
                    if !approval_perms.is_empty() {
                        detail.push_str(&format!(";approval={}", join_perms(&approval_perms)));
                    }
                    detail
                }),
                ..Audit::new(action::TOKEN_ISSUED)
            },
            now,
        )?;
        tx.commit()?;
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
        self.revoke_nip46_token_as(id, None, now)
    }

    /// `revoke_nip46_token`, recording who revoked it (#96).
    pub fn revoke_nip46_token_as(&self, id: &str, actor: Option<&str>, now: DateTime<Utc>) -> anyhow::Result<bool> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let changed = tx.execute(
            "UPDATE nip46_tokens SET revoked_at = ?2 WHERE id = ?1 AND revoked_at IS NULL AND used_at IS NULL",
            params![id, now.to_rfc3339()],
        )?;
        if changed > 0 {
            let member = Database::token_member(&tx, id)?;
            audit::record(
                &tx,
                &Audit { actor, member: member.as_deref(), token_id: Some(id), detail: Some("reason=revoked".to_string()), ..Audit::new(action::TOKEN_REVOKED) },
                now,
            )?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    /// Connects `client_pubkey` with the token whose secret hashes to `secret_hash`, in one
    /// transaction: the token is checked (unused, unrevoked, unexpired, its member still in the
    /// vault), marked used, any earlier active connection of the client is replaced, and the new
    /// connection is stored with the granted permissions. A live token refused for its identity,
    /// its member or the permissions asked for is marked used all the same, and recorded as
    /// `token_revoked`, `reason=connect_refused` (`RedeemRefusal::uses_up_token`).
    pub fn redeem_nip46_token(
        &self,
        secret_hash: &str,
        client_pubkey: &str,
        requested_perms: Option<&str>,
        metadata: &ClientMetadata,
        now: DateTime<Utc>,
    ) -> anyhow::Result<Result<Nip46Connection, RedeemRefusal>> {
        self.redeem_nip46_token_for(secret_hash, None, client_pubkey, requested_perms, metadata, now)
    }

    /// `redeem_nip46_token` for a `connect` addressed to `identity` (#103; None: the primary): a
    /// token for another identity is refused, as is one whose member may no longer use it.
    pub fn redeem_nip46_token_for(
        &self,
        secret_hash: &str,
        identity: Option<&str>,
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
        // From here the secret matched a live token: a refusal uses it up, in this transaction.
        let perms = grant(&token.perms, requested_perms);
        let refusal = match member_refusal(&tx, &token, identity)? {
            Some(refusal) => Some(refusal),
            None if perms.is_empty() => Some(RedeemRefusal::NothingGrantable),
            None => None,
        };
        if let Some(refusal) = refusal {
            use_up_refused_token(&tx, &token, client_pubkey, refusal, now)?;
            tx.commit()?;
            return Ok(Err(refusal));
        }
        let perms_granted = perms.clone();

        tx.execute("UPDATE nip46_tokens SET used_at = ?2 WHERE id = ?1", params![token.id, now.to_rfc3339()])?;
        let replaced: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT id, for_pubkey FROM nip46_connections WHERE client_pubkey = ?1 AND revoked_at IS NULL")?;
            let rows = stmt.query_map(params![client_pubkey], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        tx.execute(
            "UPDATE nip46_connections SET revoked_at = ?2, revoked_reason = 'replaced'
             WHERE client_pubkey = ?1 AND revoked_at IS NULL",
            params![client_pubkey, now.to_rfc3339()],
        )?;
        for (id, member) in &replaced {
            audit::record(
                &tx,
                &Audit {
                    member: Some(member),
                    connection_id: Some(id),
                    client_pubkey: Some(client_pubkey),
                    detail: Some("reason=replaced".to_string()),
                    ..Audit::new(action::CONNECTION_REVOKED)
                },
                now,
            )?;
        }
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
            // To the microsecond, as `db_time` stores it, so this value is the stored one.
            expires_at: token.connection_lifetime_hours.map(|hours| (now + chrono::Duration::hours(hours.into())).trunc_subsecs(6)),
            rate_limit: token.rate_limit,
            approval_perms: token.approval_perms.iter().filter(|p| perms_granted.contains(p)).cloned().collect(),
            identity_pubkey: token.identity_pubkey.clone(),
        };
        tx.execute(
            &format!(
                "INSERT INTO nip46_connections ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, NULL, NULL, NULL, ?10, ?11, ?12, ?13, ?14)",
                CONNECTION_COLUMNS
            ),
            params![
                connection.id, connection.client_pubkey, connection.token_id, connection.for_pubkey, join_perms(&connection.perms),
                connection.client_name, connection.client_url, connection.client_image, now.to_rfc3339(), connection.expires_at.map(db_time),
                connection.rate_limit.map(|l| l.per_minute), connection.rate_limit.map(|l| l.burst), optional_perms(&connection.approval_perms),
                connection.identity_pubkey
            ],
        )?;
        audit::record(
            &tx,
            &Audit {
                member: Some(&connection.for_pubkey),
                connection_id: Some(&connection.id),
                token_id: Some(&connection.token_id),
                client_pubkey: Some(client_pubkey),
                identity: connection.identity_pubkey.as_deref(),
                detail: Some({
                    let mut detail = match connection.expires_at {
                        Some(expires_at) => format!("perms={};expires_at={}", join_perms(&connection.perms), db_time(expires_at)),
                        None => format!("perms={}", join_perms(&connection.perms)),
                    };
                    if !connection.approval_perms.is_empty() {
                        detail.push_str(&format!(";approval={}", join_perms(&connection.approval_perms)));
                    }
                    detail
                }),
                ..Audit::new(action::CONNECTION_CREATED)
            },
            now,
        )?;
        tx.commit()?;
        Ok(Ok(connection))
    }

    /// The client's active connection, only while its member is still in the vault and may sign:
    /// an administrator or a signer, not a viewer (#77). A viewer's connections are revoked when
    /// they become one; this refuses them even if one was missed, as for a removed member.
    pub fn active_nip46_connection(&self, client_pubkey: &str) -> anyhow::Result<Option<Nip46Connection>> {
        self.active_nip46_connection_at(client_pubkey, Utc::now())
    }

    /// `active_nip46_connection` as at `now`: a connection that has reached its expiry (#98) is not
    /// active, whether or not `expire_nip46_connections` has marked it yet.
    pub fn active_nip46_connection_at(&self, client_pubkey: &str, now: DateTime<Utc>) -> anyhow::Result<Option<Nip46Connection>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let connection = conn
            .query_row(
                &format!(
                    "SELECT {} FROM nip46_connections c
                     WHERE c.client_pubkey = ?1 AND c.revoked_at IS NULL {}
                       AND EXISTS(SELECT 1 FROM team_members m WHERE m.pubkey = c.for_pubkey {})
                       AND {}",
                    CONNECTION_COLUMNS.split(", ").map(|c| format!("c.{}", c)).collect::<Vec<_>>().join(", "),
                    not_expired(2),
                    MEMBER_CAN_SIGN,
                    IDENTITY_ALLOWED
                ),
                params![client_pubkey, db_time(now)],
                connection_from_row,
            )
            .optional()?;
        Ok(connection)
    }

    /// Ends every connection that has reached its expiry (#98) and is not yet revoked, in one
    /// transaction: `revoked_reason` `expired`, no `revoked_by`, and `revoked_at` the expiry itself,
    /// so the record shows when access ended rather than when the bunker noticed. Each is recorded
    /// as `connection_revoked`, `reason=expired`, with no actor. Returns the connections it ended.
    pub fn expire_nip46_connections(&self, now: DateTime<Utc>) -> anyhow::Result<Vec<Nip46Connection>> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let expired: Vec<Nip46Connection> = {
            let mut stmt = tx.prepare(&format!(
                "SELECT {} FROM nip46_connections WHERE revoked_at IS NULL AND expires_at IS NOT NULL AND expires_at <= ?1 ORDER BY expires_at",
                CONNECTION_COLUMNS
            ))?;
            let rows = stmt.query_map(params![db_time(now)], connection_from_row)?;
            rows.collect::<Result<_, _>>()?
        };
        let mut ended = Vec::with_capacity(expired.len());
        for mut connection in expired {
            let Some(expires_at) = connection.expires_at else { continue };
            tx.execute(
                "UPDATE nip46_connections SET revoked_at = ?2, revoked_reason = 'expired' WHERE id = ?1 AND revoked_at IS NULL",
                params![connection.id, expires_at.to_rfc3339()],
            )?;
            audit::record(
                &tx,
                &Audit {
                    member: Some(&connection.for_pubkey),
                    connection_id: Some(&connection.id),
                    token_id: Some(&connection.token_id),
                    client_pubkey: Some(&connection.client_pubkey),
                    detail: Some("reason=expired".to_string()),
                    ..Audit::new(action::CONNECTION_REVOKED)
                },
                now,
            )?;
            connection.revoked_at = Some(expires_at);
            connection.revoked_reason = Some("expired".to_string());
            ended.push(connection);
        }
        tx.commit()?;
        Ok(ended)
    }

    /// Whether `pubkey` is the key of any NIP-46 app: one that has connected, active or not (#97),
    /// or that has sent the bunker any NIP-46 request, connected or not (`record_nip46_sender`).
    /// The traffic between those apps and the bunker is encrypted with the bunker's key, so the
    /// encryption methods refuse them as a peer.
    pub fn is_nip46_client(&self, pubkey: &str) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM nip46_connections WHERE client_pubkey = ?1)
                 OR EXISTS(SELECT 1 FROM nip46_senders WHERE pubkey = ?1)",
            params![pubkey],
            |row| row.get(0),
        )?)
    }

    /// Records `pubkey` (hex) as the sender of a NIP-46 request, the first time it sends one; later
    /// requests change nothing. Called for every request the bunker reads, before it is handled.
    ///
    /// Anyone can send a request from a new key, and a recorded key is never forgotten (it would
    /// become usable as an encryption peer again), so at most `NEW_NIP46_SENDERS_PER_HOUR` new keys
    /// are recorded in any hour. False when `pubkey` is new and that many already were: its request
    /// is then not handled. A key with a connection, live or not, is never new: it is already
    /// refused as a peer through that connection, including one made before senders were recorded.
    pub fn record_nip46_sender(&self, pubkey: &str, now: DateTime<Utc>) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let known: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM nip46_senders WHERE pubkey = ?1)
                 OR EXISTS(SELECT 1 FROM nip46_connections WHERE client_pubkey = ?1)",
            params![pubkey],
            |row| row.get(0),
        )?;
        if known {
            return Ok(true);
        }
        let recent: i64 = conn.query_row(
            "SELECT COUNT(*) FROM nip46_senders WHERE first_seen_at > ?1",
            params![db_time(now - chrono::Duration::hours(1))],
            |row| row.get(0),
        )?;
        if recent >= NEW_NIP46_SENDERS_PER_HOUR {
            return Ok(false);
        }
        conn.execute("INSERT OR IGNORE INTO nip46_senders (pubkey, first_seen_at) VALUES (?1, ?2)", params![pubkey, db_time(now)])?;
        Ok(true)
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
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let target: Option<(String, String)> = tx
            .query_row(
                "SELECT for_pubkey, client_pubkey FROM nip46_connections
                 WHERE id = ?1 AND revoked_at IS NULL AND (?2 IS NULL OR for_pubkey = ?2)",
                params![id, only_for],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((member, client)) = target else { return Ok(false) };
        tx.execute(
            "UPDATE nip46_connections SET revoked_at = ?3, revoked_reason = ?2, revoked_by = ?4 WHERE id = ?1",
            params![id, reason, now.to_rfc3339(), revoked_by],
        )?;
        audit::record(
            &tx,
            &Audit {
                actor: revoked_by,
                member: Some(&member),
                connection_id: Some(id),
                client_pubkey: Some(&client),
                detail: Some(format!("reason={}", reason)),
                ..Audit::new(action::CONNECTION_REVOKED)
            },
            now,
        )?;
        tx.commit()?;
        Ok(true)
    }

    /// After a restore (#95): revokes every active NIP-46 connection (`restored`) and every token
    /// neither used nor revoked, in one transaction, so apps pair again with new tokens and nothing
    /// shut out after the backup comes back. Members, roles, the signing log and the relays are
    /// untouched. The audit event records when the restored backup was taken, `backup_created_at`.
    /// Returns how many connections and tokens it revoked.
    pub fn revoke_all_for_restore(&self, backup_created_at: DateTime<Utc>, now: DateTime<Utc>) -> anyhow::Result<(usize, usize)> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let members: Vec<String> = {
            let mut stmt = tx.prepare(
                "SELECT for_pubkey FROM nip46_connections WHERE revoked_at IS NULL
                 UNION SELECT for_pubkey FROM nip46_tokens WHERE revoked_at IS NULL AND used_at IS NULL",
            )?;
            let rows = stmt.query_map([], |row| row.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        let (mut connections, mut tokens) = (0, 0);
        for member in &members {
            let (c, t) = revoke_member_access(&tx, member, crate::backup::RESTORED_REASON, now)?;
            connections += c;
            tokens += t;
        }
        audit::record(
            &tx,
            &Audit {
                detail: Some(format!(
                    "connections={};tokens={};backup_created_at={}",
                    connections,
                    tokens,
                    backup_created_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
                )),
                ..Audit::new(action::RESTORED)
            },
            now,
        )?;
        tx.commit()?;
        Ok((connections, tokens))
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

    /// Active connections whose member is still in the vault and may sign (#77).
    pub fn active_nip46_connection_count(&self) -> anyhow::Result<usize> {
        self.active_nip46_connection_count_at(Utc::now())
    }

    /// `active_nip46_connection_count` as at `now`: expired connections (#98) are not counted.
    pub fn active_nip46_connection_count_at(&self, now: DateTime<Utc>) -> anyhow::Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let count: i64 = conn.query_row(
            &format!(
                "SELECT count(*) FROM nip46_connections c WHERE c.revoked_at IS NULL {}
                   AND EXISTS(SELECT 1 FROM team_members m WHERE m.pubkey = c.for_pubkey {})",
                not_expired(1),
                MEMBER_CAN_SIGN
            ),
            params![db_time(now)],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

/// One of the bunker's own NIP-46 relays, as stored by the console (#78).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BunkerRelay {
    /// Normalised (`bunker_relays::normalise_relay_url`).
    pub url: String,
    /// Its place in the list, from 0: the order of the relays in a `bunker://` string.
    pub position: i64,
    /// The administrator who first added it (hex); empty when the router ran without the guard.
    pub added_by: String,
    pub added_at: DateTime<Utc>,
}

impl Database {
    /// The stored relay list, in the administrator's order (#78).
    pub fn list_bunker_relays(&self) -> anyhow::Result<Vec<BunkerRelay>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare("SELECT url, position, added_by, added_at FROM bunker_relays ORDER BY position ASC")?;
        let rows = stmt
            .query_map([], |row| {
                Ok(BunkerRelay {
                    url: row.get(0)?,
                    position: row.get(1)?,
                    added_by: row.get(2)?,
                    added_at: parse_time(&row.get::<_, String>(3)?)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Replaces the stored list with `urls`, in that order, in one transaction, so the list is
    /// always saved whole (#78). A relay already listed keeps who added it and when; a new one is
    /// recorded as added by `by` at `now`. `urls` must already be validated and normalised
    /// (`bunker_relays::validate`): the table refuses a duplicate, which fails the whole save.
    pub fn replace_bunker_relays(&self, urls: &[String], by: &str, now: DateTime<Utc>) -> anyhow::Result<()> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let existing: std::collections::HashMap<String, (String, String)> = {
            let mut stmt = tx.prepare("SELECT url, added_by, added_at FROM bunker_relays")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, (row.get(1)?, row.get(2)?))))?;
            rows.collect::<Result<_, _>>()?
        };
        tx.execute("DELETE FROM bunker_relays", [])?;
        for (position, url) in urls.iter().enumerate() {
            let (added_by, added_at) = existing.get(url).cloned().unwrap_or_else(|| (by.to_string(), now.to_rfc3339()));
            tx.execute(
                "INSERT INTO bunker_relays (url, position, added_by, added_at) VALUES (?1, ?2, ?3, ?4)",
                params![url, position as i64, added_by, added_at],
            )?;
        }
        audit::record(
            &tx,
            &Audit { actor: audit::actor(by), detail: Some(format!("relays={}", urls.join(" "))), ..Audit::new(action::RELAYS_CHANGED) },
            now,
        )?;
        tx.commit()?;
        Ok(())
    }
}

// Identities (#103): the keys this bunker signs as, and who may use each. See migrations.rs,
// migration 13.

/// One identity, never its key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Hex.
    pub pubkey: String,
    pub label: String,
    /// The primary's key is `bunker-key.enc`; every other identity's is in the database.
    pub is_primary: bool,
    pub added_at: DateTime<Utc>,
}

impl Identity {
    /// How tokens, connections and grants refer to it: None for the primary.
    pub fn reference(&self) -> Option<&str> {
        (!self.is_primary).then_some(self.pubkey.as_str())
    }
}

/// An identity other than the primary, with its encrypted key, for unlocking at start.
#[derive(Debug, Clone)]
pub struct StoredIdentity {
    pub pubkey: String,
    pub label: String,
    /// The `bancwr-key v1` line, as `key_store::encrypt` writes it.
    pub key_file: String,
}

/// Why `add_identity` refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddIdentityRefusal {
    /// It is already one of this bunker's identities, the primary included.
    AlreadyAnIdentity,
    /// It is a team member's key, and an identity's key never signs in (T2).
    IsAMember,
}

/// The longest label an identity may have.
pub const MAX_IDENTITY_LABEL_CHARS: usize = 100;

fn identity_from_row(row: &rusqlite::Row) -> rusqlite::Result<Identity> {
    Ok(Identity {
        pubkey: row.get(0)?,
        label: row.get(1)?,
        is_primary: row.get::<_, i64>(2)? != 0,
        added_at: parse_time(&row.get::<_, String>(3)?)?,
    })
}

impl Database {
    /// Records the primary identity, whose key is `bunker-key.enc`, once it is unlocked: its row
    /// is added the first time, labelled "Primary", and its pubkey follows the key file (after a
    /// restore of another bunker, say). Grants to the primary are kept: they are to the primary,
    /// whichever key it is.
    pub fn ensure_primary_identity(&self, pubkey: &str, now: DateTime<Utc>) -> anyhow::Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let current: Option<String> = conn.query_row("SELECT pubkey FROM identities WHERE is_primary = 1", [], |row| row.get(0)).optional()?;
        match current {
            Some(current) if current == pubkey => {}
            Some(current) => {
                conn.execute("UPDATE identities SET pubkey = ?2 WHERE pubkey = ?1 AND is_primary = 1", params![current, pubkey])?;
            }
            None => {
                conn.execute(
                    "INSERT INTO identities (pubkey, label, key_file, is_primary, added_at) VALUES (?1, 'Primary', NULL, 1, ?2)",
                    params![pubkey, db_time(now)],
                )?;
            }
        }
        Ok(())
    }

    /// Every identity: the primary first, then the others in the order they were added.
    pub fn list_identities(&self) -> anyhow::Result<Vec<Identity>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare("SELECT pubkey, label, is_primary, added_at FROM identities ORDER BY is_primary DESC, added_at, pubkey")?;
        let rows = stmt.query_map([], identity_from_row)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// The identities other than the primary, with their encrypted keys, for unlocking.
    pub fn stored_identities(&self) -> anyhow::Result<Vec<StoredIdentity>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare("SELECT pubkey, label, key_file FROM identities WHERE is_primary = 0 AND key_file IS NOT NULL ORDER BY added_at, pubkey")?;
        let rows = stmt
            .query_map([], |row| Ok(StoredIdentity { pubkey: row.get(0)?, label: row.get(1)?, key_file: row.get(2)? }))?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Adds an identity whose key is `key_file` (encrypted, as `key_store::encrypt` writes it).
    /// The first identity added beside the primary grants the primary to every member who may
    /// sign, in the same transaction, so nobody loses access when grants start to matter. Recorded
    /// as `identity_added`, with no actor: it is done at the bunker's own command line.
    pub fn add_identity(&self, pubkey: &str, label: &str, key_file: &str, now: DateTime<Utc>) -> anyhow::Result<Result<(), AddIdentityRefusal>> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM identities WHERE pubkey = ?1)", params![pubkey], |row| row.get(0))?;
        if exists {
            return Ok(Err(AddIdentityRefusal::AlreadyAnIdentity));
        }
        let member: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM team_members WHERE pubkey = ?1)", params![pubkey], |row| row.get(0))?;
        if member {
            return Ok(Err(AddIdentityRefusal::IsAMember));
        }
        let first: bool = tx.query_row("SELECT NOT EXISTS(SELECT 1 FROM identities WHERE is_primary = 0)", [], |row| row.get(0))?;
        if first {
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO identity_access (identity_pubkey, member_pubkey, granted_by, granted_at)
                     SELECT '', m.pubkey, NULL, ?1 FROM team_members m WHERE 1 = 1 {}",
                    MEMBER_CAN_SIGN
                ),
                params![db_time(now)],
            )?;
        }
        tx.execute(
            "INSERT INTO identities (pubkey, label, key_file, is_primary, added_at) VALUES (?1, ?2, ?3, 0, ?4)",
            params![pubkey, label, key_file, db_time(now)],
        )?;
        audit::record(&tx, &Audit { identity: Some(pubkey), detail: Some(format!("label={}", label)), ..Audit::new(action::IDENTITY_ADDED) }, now)?;
        tx.commit()?;
        Ok(Ok(()))
    }

    /// The identity with this pubkey, primary included.
    pub fn find_identity(&self, pubkey: &str) -> anyhow::Result<Option<Identity>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(conn
            .query_row("SELECT pubkey, label, is_primary, added_at FROM identities WHERE pubkey = ?1", params![pubkey], identity_from_row)
            .optional()?)
    }

    /// Renames an identity, recorded as `identity_renamed` by `actor`. False when there is none.
    pub fn rename_identity(&self, pubkey: &str, label: &str, actor: Option<&str>, now: DateTime<Utc>) -> anyhow::Result<bool> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let changed = tx.execute("UPDATE identities SET label = ?2 WHERE pubkey = ?1", params![pubkey, label])?;
        if changed > 0 {
            audit::record(&tx, &Audit { actor, identity: Some(pubkey), detail: Some(format!("label={}", label)), ..Audit::new(action::IDENTITY_RENAMED) }, now)?;
        }
        tx.commit()?;
        Ok(changed > 0)
    }

    /// Whether this bunker has an identity beside the primary, from when grants matter.
    pub fn has_several_identities(&self) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM identities WHERE is_primary = 0)", [], |row| row.get(0))?)
    }

    /// Whether `member` may use `identity` (None: the primary).
    pub fn may_use_identity(&self, member: &str, identity: Option<&str>) -> anyhow::Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        Ok(may_use_identity(&conn, member, identity)?)
    }

    /// The members granted `identity` (None: the primary), hex. Administrators may use every
    /// identity without a grant, and are not listed unless granted.
    pub fn identity_members(&self, identity: Option<&str>) -> anyhow::Result<Vec<String>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare("SELECT member_pubkey FROM identity_access WHERE identity_pubkey = ?1 ORDER BY member_pubkey")?;
        let rows = stmt.query_map(params![access_key(identity)], |row| row.get(0))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Grants `member` `identity` (None: the primary), recorded as `identity_access_granted` by
    /// `actor`. False when it was granted already.
    pub fn grant_identity(&self, identity: Option<&str>, member: &str, actor: Option<&str>, now: DateTime<Utc>) -> anyhow::Result<bool> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let added = tx.execute(
            "INSERT OR IGNORE INTO identity_access (identity_pubkey, member_pubkey, granted_by, granted_at) VALUES (?1, ?2, ?3, ?4)",
            params![access_key(identity), member, actor, db_time(now)],
        )?;
        if added > 0 {
            audit::record(&tx, &Audit { actor, member: Some(member), identity, ..Audit::new(action::IDENTITY_ACCESS_GRANTED) }, now)?;
        }
        tx.commit()?;
        Ok(added > 0)
    }

    /// Removes `member`'s grant of `identity` (None: the primary), and in the same transaction
    /// revokes their active connections and unused tokens for it (`access_removed`), each recorded,
    /// as a role change does (#77). Recorded as `identity_access_removed` by `actor`. Returns how
    /// many connections and tokens it revoked, or None when there was no grant.
    pub fn remove_identity_grant(&self, identity: Option<&str>, member: &str, actor: Option<&str>, now: DateTime<Utc>) -> anyhow::Result<Option<(usize, usize)>> {
        let mut conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        let removed = tx.execute("DELETE FROM identity_access WHERE identity_pubkey = ?1 AND member_pubkey = ?2", params![access_key(identity), member])?;
        if removed == 0 {
            return Ok(None);
        }
        audit::record(&tx, &Audit { actor, member: Some(member), identity, ..Audit::new(action::IDENTITY_ACCESS_REMOVED) }, now)?;
        // An administrator may still use it, so only a member who no longer may loses anything.
        let revoked = if may_use_identity(&tx, member, identity)? { (0, 0) } else { revoke_identity_access(&tx, member, identity, now)? };
        tx.commit()?;
        Ok(Some(revoked))
    }
}

/// Revokes `member`'s active connections and unused tokens for `identity` (None: the primary),
/// recording each, inside the caller's transaction.
fn revoke_identity_access(conn: &Connection, member: &str, identity: Option<&str>, now: DateTime<Utc>) -> rusqlite::Result<(usize, usize)> {
    const SAME_IDENTITY: &str = "COALESCE(identity_pubkey, '') = ?2";
    let key = access_key(identity);
    let connections: Vec<(String, String)> = {
        let mut stmt = conn.prepare(&format!("SELECT id, client_pubkey FROM nip46_connections WHERE for_pubkey = ?1 AND revoked_at IS NULL AND {}", SAME_IDENTITY))?;
        let rows = stmt.query_map(params![member, key], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    let tokens: Vec<String> = {
        let mut stmt = conn.prepare(&format!("SELECT id FROM nip46_tokens WHERE for_pubkey = ?1 AND revoked_at IS NULL AND used_at IS NULL AND {}", SAME_IDENTITY))?;
        let rows = stmt.query_map(params![member, key], |row| row.get(0))?;
        rows.collect::<Result<_, _>>()?
    };
    conn.execute(
        &format!("UPDATE nip46_tokens SET revoked_at = ?3 WHERE for_pubkey = ?1 AND revoked_at IS NULL AND used_at IS NULL AND {}", SAME_IDENTITY),
        params![member, key, now.to_rfc3339()],
    )?;
    conn.execute(
        &format!("UPDATE nip46_connections SET revoked_at = ?3, revoked_reason = 'access_removed' WHERE for_pubkey = ?1 AND revoked_at IS NULL AND {}", SAME_IDENTITY),
        params![member, key, now.to_rfc3339()],
    )?;
    for (id, client) in &connections {
        audit::record(
            conn,
            &Audit {
                member: Some(member),
                connection_id: Some(id),
                client_pubkey: Some(client),
                identity,
                detail: Some("reason=access_removed".to_string()),
                ..Audit::new(action::CONNECTION_REVOKED)
            },
            now,
        )?;
    }
    for id in &tokens {
        audit::record(
            conn,
            &Audit { member: Some(member), token_id: Some(id), identity, detail: Some("reason=access_removed".to_string()), ..Audit::new(action::TOKEN_REVOKED) },
            now,
        )?;
    }
    Ok((connections.len(), tokens.len()))
}
