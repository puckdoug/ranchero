// SPDX-License-Identifier: AGPL-3.0-only

use std::path::Path;
use std::sync::Mutex;
use rusqlite::Connection;
use serde::{Serialize, de::DeserializeOwned};
use crate::{open, migrate, Migration, Result};

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/store/0001_init.sql"),
    },
];

pub struct KvStore {
    conn: Mutex<Connection>,
}

impl KvStore {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = open(path)?;
        migrate(&conn, MIGRATIONS)?;
        Ok(KvStore { conn: Mutex::new(conn) })
    }

    pub fn put(&self, id: &str, data: &[u8]) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO store (id, data) VALUES (?1, ?2)",
            rusqlite::params![id, data],
        )?;
        Ok(())
    }

    pub fn get(&self, id: &str) -> Result<Option<Vec<u8>>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT data FROM store WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM store WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(n > 0)
    }

    pub fn exists(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT 1 FROM store WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        Ok(rows.next()?.is_some())
    }

    pub fn put_json<T: Serialize>(&self, id: &str, value: &T) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        self.put(id, &bytes)
    }

    pub fn get_json<T: DeserializeOwned>(&self, id: &str) -> Result<Option<T>> {
        match self.get(id)? {
            Some(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            None => Ok(None),
        }
    }
}
