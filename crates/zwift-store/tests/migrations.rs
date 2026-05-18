// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use zwift_store::{open, migrate, Migration, Error};

const MIGS: &[Migration] = &[
    Migration { version: 1, sql: "CREATE TABLE t1 (id INTEGER PRIMARY KEY);" },
    Migration { version: 2, sql: "CREATE TABLE t2 (id INTEGER PRIMARY KEY);" },
];

fn user_version(conn: &rusqlite::Connection) -> u32 {
    conn.query_row("PRAGMA user_version", [], |r| r.get::<_, u32>(0)).unwrap()
}

// 16.3-T — fresh DB runs all migrations and lands at user_version = N.
#[test]
fn fresh_db_runs_all_migrations() {
    let dir = tempdir().unwrap();
    let conn = open(&dir.path().join("m.sqlite")).unwrap();
    assert_eq!(user_version(&conn), 0, "fresh DB must start at user_version 0");

    migrate(&conn, MIGS).unwrap();

    assert_eq!(user_version(&conn), 2);
    // Both tables must exist — would error if migration was skipped or failed.
    conn.execute("INSERT INTO t1 (id) VALUES (1)", []).unwrap();
    conn.execute("INSERT INTO t2 (id) VALUES (1)", []).unwrap();
}

// 16.3-T — a DB at user_version = k runs only migrations k+1..=N.
#[test]
fn partial_db_runs_only_remaining_migrations() {
    let dir = tempdir().unwrap();
    let conn = open(&dir.path().join("m.sqlite")).unwrap();

    // Simulate a DB that already ran migration 1.
    conn.execute_batch("CREATE TABLE t1 (id INTEGER PRIMARY KEY);").unwrap();
    conn.execute_batch("PRAGMA user_version = 1;").unwrap();

    migrate(&conn, MIGS).unwrap();

    assert_eq!(user_version(&conn), 2);
    // t2 must exist (migration 2 ran); t1 must still be intact.
    conn.execute("INSERT INTO t2 (id) VALUES (1)", []).unwrap();
    conn.execute("INSERT INTO t1 (id) VALUES (2)", []).unwrap();
}

// 16.3-T — running migrate on a fully-migrated DB is a no-op.
#[test]
fn migrate_is_idempotent() {
    let dir = tempdir().unwrap();
    let conn = open(&dir.path().join("m.sqlite")).unwrap();

    migrate(&conn, MIGS).unwrap();
    migrate(&conn, MIGS).unwrap(); // second call must not error or re-run

    assert_eq!(user_version(&conn), 2);
}

// 16.4-T — migrate refuses when the DB's user_version exceeds the highest
//           known migration and returns Error::SchemaTooNew.
#[test]
fn schema_too_new_returns_error() {
    let dir = tempdir().unwrap();
    let conn = open(&dir.path().join("m.sqlite")).unwrap();
    conn.execute_batch("PRAGMA user_version = 99;").unwrap();

    let err = migrate(&conn, MIGS).unwrap_err();
    assert!(
        matches!(err, Error::SchemaTooNew { found: 99, max: 2 }),
        "expected SchemaTooNew {{ found: 99, max: 2 }}, got: {err}",
    );
}
