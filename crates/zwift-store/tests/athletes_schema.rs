// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use zwift_store::AthletesDb;

#[test]
fn athletes_table_exists_with_correct_columns() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();
    let conn = db.raw_conn();

    // Each expected column must appear in table_info.
    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(athletes)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    for expected in &["id", "fname", "lname", "ftp", "weight", "badges", "last_seen"] {
        assert!(cols.contains(&expected.to_string()), "missing column: {expected}");
    }
}

#[test]
fn athletes_table_has_primary_key_on_id() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();
    let conn = db.raw_conn();

    let pk_col: String = conn
        .query_row(
            "SELECT name FROM pragma_table_info('athletes') WHERE pk = 1",
            [],
            |r| r.get(0),
        )
        .expect("no primary-key column found");

    assert_eq!(pk_col, "id");
}

#[test]
fn athletes_table_has_index_on_last_seen() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();
    let conn = db.raw_conn();

    let indexes: Vec<String> = conn
        .prepare("PRAGMA index_list(athletes)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert!(
        indexes.iter().any(|n| n.contains("last_seen")),
        "expected an index on last_seen, found: {indexes:?}",
    );
}
