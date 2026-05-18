// SPDX-License-Identifier: AGPL-3.0-only

use std::path::Path;
use rusqlite::Connection;
use crate::Result;

pub fn open(path: &Path) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch("
        PRAGMA journal_mode  = WAL;
        PRAGMA synchronous   = NORMAL;
        PRAGMA foreign_keys  = ON;
        PRAGMA busy_timeout  = 5000;
        PRAGMA temp_store    = MEMORY;
    ")?;
    Ok(conn)
}
