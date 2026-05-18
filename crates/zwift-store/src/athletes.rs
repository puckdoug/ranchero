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
];

#[derive(Debug, Clone, PartialEq)]
pub struct AthleteRecord {
    pub id:        i64,
    pub fname:     Option<String>,
    pub lname:     Option<String>,
    pub ftp:       Option<i32>,
    pub weight:    Option<f64>,
    pub badges:    Option<serde_json::Value>,
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
        let badges = rec.badges.as_ref().map(|v| v.to_string());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO athletes (id, fname, lname, ftp, weight, badges, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                 fname     = excluded.fname,
                 lname     = excluded.lname,
                 ftp       = excluded.ftp,
                 weight    = excluded.weight,
                 badges    = excluded.badges,
                 last_seen = excluded.last_seen",
            rusqlite::params![
                rec.id, rec.fname, rec.lname,
                rec.ftp, rec.weight, badges, rec.last_seen,
            ],
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
            "SELECT id, fname, lname, ftp, weight, badges, last_seen
             FROM athletes WHERE id = ?1",
        )?;
        let mut rows = stmt.query(rusqlite::params![id])?;
        match rows.next()? {
            None => Ok(None),
            Some(row) => {
                let badges: Option<String> = row.get(5)?;
                let badges = badges
                    .map(|s| serde_json::from_str(&s))
                    .transpose()?;
                Ok(Some(AthleteRecord {
                    id:        row.get(0)?,
                    fname:     row.get(1)?,
                    lname:     row.get(2)?,
                    ftp:       row.get(3)?,
                    weight:    row.get(4)?,
                    badges,
                    last_seen: row.get(6)?,
                }))
            }
        }
    }
}
