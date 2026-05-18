// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use zwift_store::open;

fn pragma_str(conn: &rusqlite::Connection, name: &str) -> String {
    conn.query_row(
        &format!("PRAGMA {name}"),
        [],
        |row| row.get::<_, String>(0),
    )
    .unwrap_or_else(|_| panic!("PRAGMA {name} failed"))
}

fn pragma_i64(conn: &rusqlite::Connection, name: &str) -> i64 {
    conn.query_row(
        &format!("PRAGMA {name}"),
        [],
        |row| row.get::<_, i64>(0),
    )
    .unwrap_or_else(|_| panic!("PRAGMA {name} failed"))
}

#[test]
fn open_creates_file_and_enables_wal() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test.sqlite");

    assert!(!db_path.exists(), "file must not exist before open");

    let conn = open(&db_path).expect("open must succeed");

    assert!(db_path.exists(), "open must create the file");
    assert_eq!(pragma_str(&conn, "journal_mode"), "wal");
}

#[test]
fn open_sets_foreign_keys_on() {
    let dir = tempdir().unwrap();
    let conn = open(&dir.path().join("fk.sqlite")).expect("open");
    assert_eq!(pragma_i64(&conn, "foreign_keys"), 1);
}

#[test]
fn open_sets_positive_busy_timeout() {
    let dir = tempdir().unwrap();
    let conn = open(&dir.path().join("bt.sqlite")).expect("open");
    assert!(pragma_i64(&conn, "busy_timeout") > 0);
}
