// SPDX-License-Identifier: AGPL-3.0-only

use rusqlite::Connection;
use crate::{Error, Result};

pub struct Migration {
    pub version: u32,
    pub sql:     &'static str,
}

pub fn migrate(conn: &Connection, migrations: &[Migration]) -> Result<()> {
    let current: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;

    let max_version = migrations.iter().map(|m| m.version).max().unwrap_or(0);

    if current > max_version {
        return Err(Error::SchemaTooNew { found: current, max: max_version });
    }

    let mut pending: Vec<&Migration> = migrations.iter()
        .filter(|m| m.version > current)
        .collect();
    pending.sort_by_key(|m| m.version);

    for m in pending {
        // PRAGMA user_version is transactional in SQLite — it rolls back with
        // the transaction if the migration SQL fails.
        conn.execute_batch(&format!(
            "BEGIN;\n{}\nPRAGMA user_version = {};\nCOMMIT;",
            m.sql, m.version,
        ))?;
    }

    Ok(())
}
