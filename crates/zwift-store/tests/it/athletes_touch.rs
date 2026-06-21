// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::json;
use tempfile::tempdir;
use zwift_store::{AthletesDb, AthleteRecord};

fn base_record() -> AthleteRecord {
    AthleteRecord {
        id:        1,
        data:      json!({
            "firstName": "Alice",
            "ftp":       280,
            "weight":    62_500,
        }),
        last_seen: 1_000,
    }
}

#[test]
fn touch_updates_last_seen_and_returns_true() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    db.upsert(&base_record()).unwrap();
    let found = db.touch(1, 9_999).unwrap();

    assert!(found);
    assert_eq!(db.get(1).unwrap().unwrap().last_seen, 9_999);
}

#[test]
fn touch_does_not_modify_other_columns() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    db.upsert(&base_record()).unwrap();
    db.touch(1, 9_999).unwrap();

    let got = db.get(1).unwrap().unwrap();
    assert_eq!(got.data["firstName"].as_str(), Some("Alice"));
    assert_eq!(got.data["ftp"].as_u64(), Some(280));
    assert_eq!(got.data["weight"].as_u64(), Some(62_500));
}

#[test]
fn touch_absent_id_returns_false() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    assert!(!db.touch(99, 1_000).unwrap());
}
