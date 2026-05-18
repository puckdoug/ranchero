// SPDX-License-Identifier: AGPL-3.0-only

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use rusqlite::Connection;
use crate::{open, migrate, Migration, Result};

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../migrations/segments/0001_init.sql"),
    },
];

fn to_unix_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs() as i64
}

pub struct SegmentsDb {
    conn: Mutex<Connection>,
}

impl SegmentsDb {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = open(path)?;
        migrate(&conn, MIGRATIONS)?;
        Ok(SegmentsDb { conn: Mutex::new(conn) })
    }

    pub fn put(
        &self,
        segment_id: i64,
        payload: &[u8],
        ttl: Duration,
        now: SystemTime,
    ) -> Result<()> {
        let inserted_at = to_unix_secs(now);
        let expires_at = inserted_at + ttl.as_secs() as i64;
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO leaderboards (segment_id, payload, inserted_at, expires_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(segment_id) DO UPDATE SET
                 payload     = excluded.payload,
                 inserted_at = excluded.inserted_at,
                 expires_at  = excluded.expires_at",
            rusqlite::params![segment_id, payload, inserted_at, expires_at],
        )?;
        Ok(())
    }

    pub fn get(&self, segment_id: i64, now: SystemTime) -> Result<Option<Vec<u8>>> {
        let now_secs = to_unix_secs(now);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached(
            "SELECT payload FROM leaderboards
             WHERE segment_id = ?1 AND expires_at > ?2",
        )?;
        let mut rows = stmt.query(rusqlite::params![segment_id, now_secs])?;
        match rows.next()? {
            Some(row) => Ok(Some(row.get(0)?)),
            None => Ok(None),
        }
    }

    pub fn evict_expired(&self, now: SystemTime) -> Result<usize> {
        let now_secs = to_unix_secs(now);
        let conn = self.conn.lock().unwrap();
        let n = conn.execute(
            "DELETE FROM leaderboards WHERE expires_at <= ?1",
            rusqlite::params![now_secs],
        )?;
        Ok(n)
    }
}
