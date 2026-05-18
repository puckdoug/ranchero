// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use zwift_store::{AthletesDb, AthleteRecord};

fn record(id: i64) -> AthleteRecord {
    AthleteRecord {
        id,
        fname: Some("Alice".into()),
        lname: Some("Smith".into()),
        ftp: Some(280),
        weight: Some(62.5),
        badges: None,
        last_seen: 1_000_000,
    }
}

#[test]
fn upsert_inserts_new_row() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    db.upsert(&record(1)).unwrap();

    assert!(db.get(1).unwrap().is_some());
}

#[test]
fn upsert_replaces_existing_row_by_id() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    db.upsert(&record(1)).unwrap();

    let updated = AthleteRecord {
        id: 1,
        fname: Some("Bob".into()),
        ftp: Some(300),
        ..record(1)
    };
    db.upsert(&updated).unwrap();

    let got = db.get(1).unwrap().unwrap();
    assert_eq!(got.fname.as_deref(), Some("Bob"));
    assert_eq!(got.ftp, Some(300));
}

#[test]
fn upsert_preserves_none_optional_columns_as_null() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let rec = AthleteRecord {
        id: 2,
        fname: None,
        lname: None,
        ftp: None,
        weight: None,
        badges: None,
        last_seen: 0,
    };
    db.upsert(&rec).unwrap();

    let got = db.get(2).unwrap().unwrap();
    assert!(got.fname.is_none());
    assert!(got.ftp.is_none());
    assert!(got.weight.is_none());
}

#[test]
fn upsert_multiple_rows_stored_independently() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    db.upsert(&record(1)).unwrap();
    db.upsert(&record(2)).unwrap();

    assert!(db.get(1).unwrap().is_some());
    assert!(db.get(2).unwrap().is_some());
}
