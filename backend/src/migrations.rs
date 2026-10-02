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

/// 1. `admin` becomes `administrator` and `viewer` becomes `user`; `signer` is unchanged. Any
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
