//! Schema and data migrations, versioned with SQLite's `PRAGMA user_version` (#24).
//!
//! `Database::new` creates any missing tables, then applies every migration above the stored
//! version, in order, each in its own transaction that also records the new version. A failure
//! stops startup rather than leave the bunker running on a half-migrated database. To change the
//! schema or stored data, append a migration; never edit or reorder one that has shipped.

use std::collections::HashMap;

use rusqlite::{params, Connection, Transaction};
use tracing::{info, warn};

use crate::registry::{canonical_pubkey, Role};

type Migration = fn(&Transaction) -> anyhow::Result<()>;

/// Index + 1 is the `user_version` each migration leaves the database at.
const MIGRATIONS: &[(&str, Migration)] = &[
    ("rename roles to administrator, user and signer", rename_roles),
    ("store pubkeys as lowercase hex", canonicalise_pubkeys),
    ("persist NIP-46 connection tokens and connections", nip46_connections),
    ("record who revoked a NIP-46 connection", nip46_revoked_by),
    ("record the member and connection a signature was for", signing_log_member),
    ("store the bunker's own NIP-46 relays", bunker_relays),
    ("replace the roles with administrator, signer and viewer", admin_signer_viewer_roles),
];

/// The version a fully migrated database is at.
pub const LATEST_VERSION: usize = MIGRATIONS.len();

pub fn run(conn: &mut Connection) -> anyhow::Result<()> {
    let current: usize = conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))? as usize;
    for (index, (name, migrate)) in MIGRATIONS.iter().enumerate().skip(current) {
        let version = index + 1;
        let tx = conn.transaction()?;
        migrate(&tx).map_err(|e| anyhow::anyhow!("Migration {} ({}) failed: {}", version, name, e))?;
        tx.pragma_update(None, "user_version", version as i64)?;
        tx.commit()?;
        info!("Applied database migration {}: {}", version, name);
    }
    Ok(())
}

/// 1. `admin` becomes `administrator` and `viewer` becomes `user`; `signer` is unchanged. (#77's
///    migration 7 later maps these to administrator, signer and viewer.) Any
///    other value is left as it is and logged: it grants no access until an administrator fixes it.
fn rename_roles(tx: &Transaction) -> anyhow::Result<()> {
    tx.execute("UPDATE team_members SET role = 'administrator' WHERE role = 'admin'", [])?;
    tx.execute("UPDATE team_members SET role = 'user' WHERE role = 'viewer'", [])?;

    let mut stmt = tx.prepare("SELECT name, role FROM team_members")?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    for row in rows {
        let (name, role) = row?;
        if role.parse::<Role>().is_err() {
            warn!("Team member '{}' has an unrecognised role '{}' and cannot sign in until it is corrected", name, role);
        }
    }
    Ok(())
}

/// 2. Every pubkey becomes lowercase hex. Before #24 anything starting `npub1` was accepted and
///    stored as given, so the same key could be stored twice under two encodings, which the UNIQUE
///    constraint could not see. For each key the oldest row is kept and later duplicates are deleted
///    and logged. A value that is not a valid key is left as it is and logged: it never matches.
fn canonicalise_pubkeys(tx: &Transaction) -> anyhow::Result<()> {
    struct Row {
        id: String,
        name: String,
        role: String,
        pubkey: String,
    }
    let rows: Vec<Row> = {
        let mut stmt = tx.prepare("SELECT id, name, role, pubkey FROM team_members ORDER BY created_at ASC, id ASC")?;
        let mapped = stmt.query_map([], |row| {
            Ok(Row { id: row.get(0)?, name: row.get(1)?, role: row.get(2)?, pubkey: row.get(3)? })
        })?;
        mapped.collect::<Result<_, _>>()?
    };

    // Oldest first, so the first row seen for a key is the one kept.
    let mut kept: HashMap<String, String> = HashMap::new();
    let mut updates: Vec<(String, String)> = Vec::new();
    for row in &rows {
        let Ok(hex) = canonical_pubkey(&row.pubkey) else {
            warn!("Team member '{}' has an invalid pubkey '{}' and cannot sign in until it is corrected", row.name, row.pubkey);
            continue;
        };
        if let Some(kept_name) = kept.get(&hex) {
            tx.execute("DELETE FROM team_members WHERE id = ?1", params![row.id])?;
            warn!(
                "Removed team member '{}' ({}): the same key is already registered as '{}'",
                row.name, row.role, kept_name
            );
            continue;
        }
        kept.insert(hex.clone(), row.name.clone());
        if hex != row.pubkey {
            updates.push((row.id.clone(), hex));
        }
    }
    // After the deletes, so no rewrite collides with a duplicate still present.
    for (id, hex) in updates {
        tx.execute("UPDATE team_members SET pubkey = ?1 WHERE id = ?2", params![hex, id])?;
    }
    Ok(())
}

/// 3. NIP-46 connection tokens and connections (#53): a client connects only with a single-use
///    token an administrator issued for a vault member, and the connection is kept (it used to
///    live in memory) and attributed to that member. Secrets are stored as SHA-256 hashes.
fn nip46_connections(tx: &Transaction) -> anyhow::Result<()> {
    tx.execute_batch(
        "CREATE TABLE nip46_tokens (
            id TEXT PRIMARY KEY,
            secret_hash TEXT NOT NULL UNIQUE,
            for_pubkey TEXT NOT NULL,
            issued_by TEXT NOT NULL,
            label TEXT NOT NULL,
            perms TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            used_at TEXT,
            revoked_at TEXT
        );
        CREATE INDEX idx_nip46_tokens_for ON nip46_tokens(for_pubkey);

        CREATE TABLE nip46_connections (
            id TEXT PRIMARY KEY,
            client_pubkey TEXT NOT NULL,
            token_id TEXT NOT NULL REFERENCES nip46_tokens(id),
            for_pubkey TEXT NOT NULL,
            perms TEXT NOT NULL,
            client_name TEXT,
            client_url TEXT,
            client_image TEXT,
            connected_at TEXT NOT NULL,
            last_used_at TEXT,
            revoked_at TEXT,
            revoked_reason TEXT
        );
        CREATE INDEX idx_nip46_connections_for ON nip46_connections(for_pubkey);
        -- One active connection per client key.
        CREATE UNIQUE INDEX idx_nip46_connections_active_client
            ON nip46_connections(client_pubkey) WHERE revoked_at IS NULL;",
    )?;
    Ok(())
}

/// 4. Who revoked a NIP-46 connection (#31): a member may now revoke connections made for them,
///    and an administrator any, so the record names the key that did. Empty for a connection that
///    ended any other way (logout, replaced, member removed).
fn nip46_revoked_by(tx: &Transaction) -> anyhow::Result<()> {
    tx.execute_batch("ALTER TABLE nip46_connections ADD COLUMN revoked_by TEXT;")?;
    Ok(())
}

/// 5. The member and connection each signature was for (diogel-io/workspace#38). `pubkey` has
///    always held the NIP-46 client's key, which the console showed as the member. Existing rows
///    are attributed to the connection that client held when it signed: connected at or before the
///    signature, and not revoked before it. A row no connection accounts for keeps NULLs.
fn signing_log_member(tx: &Transaction) -> anyhow::Result<()> {
    // Added only when missing, so a database whose version was rewound re-runs this cleanly.
    let columns: Vec<String> = tx
        .prepare("SELECT name FROM pragma_table_info('signing_logs')")?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    for column in ["member_pubkey", "connection_id"] {
        if !columns.iter().any(|c| c == column) {
            tx.execute_batch(&format!("ALTER TABLE signing_logs ADD COLUMN {} TEXT;", column))?;
        }
    }
    // julianday, not text comparison: RFC 3339 timestamps vary in their fractional digits.
    tx.execute_batch(
        "UPDATE signing_logs SET connection_id = (
             SELECT c.id FROM nip46_connections c
             WHERE c.client_pubkey = signing_logs.pubkey
               AND julianday(c.connected_at) <= julianday(signing_logs.timestamp)
               AND (c.revoked_at IS NULL OR julianday(c.revoked_at) >= julianday(signing_logs.timestamp))
             ORDER BY julianday(c.connected_at) DESC
             LIMIT 1
         );
         UPDATE signing_logs SET member_pubkey = (
             SELECT c.for_pubkey FROM nip46_connections c WHERE c.id = signing_logs.connection_id
         )
         WHERE connection_id IS NOT NULL;",
    )?;
    let unattributed: i64 =
        tx.query_row("SELECT count(*) FROM signing_logs WHERE connection_id IS NULL", [], |row| row.get(0))?;
    if unattributed > 0 {
        warn!("{} signing log entries could not be attributed to a NIP-46 connection; they show no member", unattributed);
    }
    Ok(())
}

/// 6. The bunker's own NIP-46 relays (#78), as an administrator sets them in the console: kept
///    here, never published to Nostr, and in force unless NIP46_RELAYS overrides them. `position`
///    keeps the administrator's order, which is the order of the relays in a `bunker://` string.
///    `IF NOT EXISTS`, so a database whose version was rewound re-runs this cleanly.
fn bunker_relays(tx: &Transaction) -> anyhow::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS bunker_relays (
            url TEXT PRIMARY KEY,
            position INTEGER NOT NULL,
            added_by TEXT NOT NULL,
            added_at TEXT NOT NULL
        );",
    )?;
    Ok(())
}

/// 7. The roles become administrator, signer and viewer (#77), replacing #24's administrator, user
///    and signer. Least privilege: today's `signer` becomes `viewer` first, then `user` becomes
///    `signer`, in that order so no member is mapped twice; `administrator` is unchanged. A viewer
///    never holds a NIP-46 connection, so every active connection of a member who is now a viewer
///    is revoked (`role_changed`), and their unused tokens with it. Every change is logged.
///
///    The mapping is recorded in `role_migrations`, and is skipped when already recorded: a
///    database whose version was rewound re-runs this, and running the mapping twice would turn
///    the signers it made into viewers. The revocation is idempotent and always runs.
fn admin_signer_viewer_roles(tx: &Transaction) -> anyhow::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS role_migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )?;
    let applied: bool =
        tx.query_row("SELECT EXISTS(SELECT 1 FROM role_migrations WHERE name = '77')", [], |row| row.get(0))?;
    let now = chrono::Utc::now().to_rfc3339();

    if !applied {
        // (name, from, to): read before any update, so each member is logged with their old role.
        let changes: Vec<(String, String, &str)> = {
            let mut stmt = tx.prepare("SELECT name, role FROM team_members WHERE role IN ('signer', 'user')")?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
            rows.map(|row| row.map(|(name, role)| {
                let to = if role == "signer" { "viewer" } else { "signer" };
                (name, role, to)
            }))
            .collect::<Result<_, _>>()?
        };
        // Signer to viewer first: the other order would turn every user into a viewer.
        tx.execute("UPDATE team_members SET role = 'viewer' WHERE role = 'signer'", [])?;
        tx.execute("UPDATE team_members SET role = 'signer' WHERE role = 'user'", [])?;
        for (name, from, to) in &changes {
            info!("Team member '{}' changed role from {} to {} (#77)", name, from, to);
        }
        tx.execute("INSERT INTO role_migrations (name, applied_at) VALUES ('77', ?1)", params![now])?;
    }

    // Viewers cannot sign: end what they hold. Read first, to log each one.
    let revoked: Vec<(String, String, String)> = {
        let mut stmt = tx.prepare(
            "SELECT c.id, m.name, COALESCE(c.client_name, c.client_pubkey) FROM nip46_connections c
             JOIN team_members m ON m.pubkey = c.for_pubkey
             WHERE c.revoked_at IS NULL AND m.role = 'viewer'",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (id, member, client) in &revoked {
        tx.execute(
            "UPDATE nip46_connections SET revoked_at = ?2, revoked_reason = 'role_changed' WHERE id = ?1",
            params![id, now],
        )?;
        warn!("Revoked NIP-46 connection {} ({}) of '{}': viewers cannot sign (#77)", id, client, member);
    }
    let tokens = tx.execute(
        "UPDATE nip46_tokens SET revoked_at = ?1
         WHERE revoked_at IS NULL AND used_at IS NULL
           AND for_pubkey IN (SELECT pubkey FROM team_members WHERE role = 'viewer')",
        params![now],
    )?;
    if tokens > 0 {
        warn!("Revoked {} unused NIP-46 connection tokens issued for members who are now viewers (#77)", tokens);
    }
    Ok(())
}
