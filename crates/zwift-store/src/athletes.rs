// SPDX-License-Identifier: AGPL-3.0-only

use std::path::Path;
use std::sync::Mutex;
use rusqlite::Connection;
use crate::{open, migrate, Migration, Result};

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/athletes/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../migrations/athletes/0002_json_blob.sql"),
    },
];

#[derive(Debug, Clone, PartialEq)]
pub struct AthleteRecord {
    pub id:        i64,
    pub data:      serde_json::Value,
    pub last_seen: i64,
}

pub struct AthletesDb {
    conn: Mutex<Connection>,
}

impl AthletesDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = open(path)?;
        migrate(&conn, MIGRATIONS)?;
        Ok(AthletesDb { conn: Mutex::new(conn) })
    }

    pub fn upsert(&self, rec: &AthleteRecord) -> Result<()> {
        let data = rec.data.to_string();
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO athletes (id, data, last_seen)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
                 data      = excluded.data,
                 last_seen = excluded.last_seen",
            rusqlite::params![rec.id, data, rec.last_seen],
        )?;
        Ok(())
    }

    pub fn touch(&self, id: i64, last_seen: i64) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "UPDATE athletes SET last_seen = ?2 WHERE id = ?1",
            rusqlite::params![id, last_seen],
        )?;
        Ok(n > 0)
    }

    pub fn get(&self, id: i64) -> Result<Option<AthleteRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT id, data, last_seen FROM athletes WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        match rows.next()? {
            None => Ok(None),
            Some(row) => {
                let data_str: String = row.get(1)?;
                let data: serde_json::Value = serde_json::from_str(&data_str)?;
                Ok(Some(AthleteRecord {
                    id:        row.get(0)?,
                    data,
                    last_seen: row.get(2)?,
                }))
            }
        }
    }

    /// Return the IDs of every athlete whose JSON blob has `marked: true`.
    pub fn marked(&self) -> Result<Vec<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT id FROM athletes WHERE json_extract(data, '$.marked') = 1",
        )?;
        let ids = stmt
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(ids)
    }
}
