// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::json;
use tempfile::tempdir;
use zwift_store::{AthletesDb, AthleteRecord};

fn record(id: i64) -> AthleteRecord {
    AthleteRecord {
        id,
        data: json!({
            "firstName": "Alice",
            "lastName":  "Smith",
            "ftp":       280,
            "weight":    62_500,
        }),
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
        id:        1,
        data:      json!({ "firstName": "Bob", "ftp": 300 }),
        last_seen: 1_000_000,
    };
    db.upsert(&updated).unwrap();

    let got = db.get(1).unwrap().unwrap();
    assert_eq!(got.data["firstName"].as_str(), Some("Bob"));
    assert_eq!(got.data["ftp"].as_u64(), Some(300));
}

#[test]
fn upsert_preserves_none_optional_columns_as_null() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let rec = AthleteRecord {
        id:        2,
        data:      json!({}),
        last_seen: 0,
    };
    db.upsert(&rec).unwrap();

    let got = db.get(2).unwrap().unwrap();
    assert!(got.data.get("firstName").is_none_or(|v| v.is_null()));
    assert!(got.data.get("ftp").is_none_or(|v| v.is_null()));
    assert!(got.data.get("weight").is_none_or(|v| v.is_null()));
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
