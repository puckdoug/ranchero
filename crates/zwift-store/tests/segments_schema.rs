// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use zwift_store::{SegmentsDb, open};

#[test]
fn leaderboards_table_exists_with_correct_columns() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("segments.sqlite");
    SegmentsDb::open(&db_path).unwrap();
    let conn = open(&db_path).unwrap();

    let cols: Vec<String> = conn
        .prepare("PRAGMA table_info(leaderboards)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    for expected in &["segment_id", "payload", "inserted_at", "expires_at"] {
        assert!(cols.contains(&expected.to_string()), "missing column: {expected}");
    }
}

#[test]
fn leaderboards_table_has_primary_key_on_segment_id() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("segments.sqlite");
    SegmentsDb::open(&db_path).unwrap();
    let conn = open(&db_path).unwrap();

    let pk_col: String = conn
        .query_row(
            "SELECT name FROM pragma_table_info('leaderboards') WHERE pk = 1",
            [],
            |r| r.get(0),
        )
        .expect("no primary-key column found");

    assert_eq!(pk_col, "segment_id");
}

#[test]
fn leaderboards_table_has_index_on_expires_at() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("segments.sqlite");
    SegmentsDb::open(&db_path).unwrap();
    let conn = open(&db_path).unwrap();

    let indexes: Vec<String> = conn
        .prepare("PRAGMA index_list(leaderboards)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert!(
        indexes.iter().any(|n| n.contains("expires_at")),
        "expected an index on expires_at, found: {indexes:?}",
    );
}
