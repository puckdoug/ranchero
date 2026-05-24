// SPDX-License-Identifier: AGPL-3.0-only

use serde_json::json;
use tempfile::tempdir;
use zwift_store::{AthletesDb, AthleteRecord};

#[test]
fn get_returns_none_for_absent_id() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    assert!(db.get(42).unwrap().is_none());
}

#[test]
fn get_returns_all_fields_for_present_row() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let rec = AthleteRecord {
        id:        7,
        data:      json!({
            "firstName": "Carol",
            "lastName":  "Jones",
            "ftp":       250,
            "weight":    58_000,
        }),
        last_seen: 42_000,
    };
    db.upsert(&rec).unwrap();

    let got = db.get(7).unwrap().unwrap();
    assert_eq!(got.id,                          7);
    assert_eq!(got.data["firstName"].as_str(),  Some("Carol"));
    assert_eq!(got.data["lastName"].as_str(),   Some("Jones"));
    assert_eq!(got.data["ftp"].as_u64(),        Some(250));
    assert_eq!(got.data["weight"].as_u64(),     Some(58_000));
    assert_eq!(got.last_seen,                   42_000);
}

#[test]
fn badges_json_round_trips_without_corruption() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let badges = json!([{"id": 1, "name": "Zwifter"}, {"id": 2, "name": "Climber"}]);
    let rec = AthleteRecord {
        id:        3,
        data:      json!({ "badges": [{"id": 1, "name": "Zwifter"}, {"id": 2, "name": "Climber"}] }),
        last_seen: 0,
    };
    db.upsert(&rec).unwrap();

    let got = db.get(3).unwrap().unwrap();
    assert_eq!(got.data["badges"], badges);
}

#[test]
fn get_returns_none_fields_as_none() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let rec = AthleteRecord { id: 5, data: json!({}), last_seen: 0 };
    db.upsert(&rec).unwrap();

    let got = db.get(5).unwrap().unwrap();
    assert!(got.data.get("firstName").is_none_or(|v| v.is_null()));
    assert!(got.data.get("lastName").is_none_or(|v| v.is_null()));
    assert!(got.data.get("ftp").is_none_or(|v| v.is_null()));
    assert!(got.data.get("weight").is_none_or(|v| v.is_null()));
    assert!(got.data.get("badges").is_none_or(|v| v.is_null()));
}
