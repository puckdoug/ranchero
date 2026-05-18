// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use serde_json::json;
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
        fname:     Some("Carol".into()),
        lname:     Some("Jones".into()),
        ftp:       Some(250),
        weight:    Some(58.0),
        badges:    None,
        last_seen: 42_000,
    };
    db.upsert(&rec).unwrap();

    let got = db.get(7).unwrap().unwrap();
    assert_eq!(got.id,        7);
    assert_eq!(got.fname,     Some("Carol".into()));
    assert_eq!(got.lname,     Some("Jones".into()));
    assert_eq!(got.ftp,       Some(250));
    assert_eq!(got.weight,    Some(58.0));
    assert_eq!(got.last_seen, 42_000);
}

#[test]
fn badges_json_round_trips_without_corruption() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let badges = json!([{"id": 1, "name": "Zwifter"}, {"id": 2, "name": "Climber"}]);
    let rec = AthleteRecord {
        id:        3,
        fname:     None,
        lname:     None,
        ftp:       None,
        weight:    None,
        badges:    Some(badges.clone()),
        last_seen: 0,
    };
    db.upsert(&rec).unwrap();

    let got = db.get(3).unwrap().unwrap();
    assert_eq!(got.badges, Some(badges));
}

#[test]
fn get_returns_none_fields_as_none() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let rec = AthleteRecord {
        id: 5, fname: None, lname: None,
        ftp: None, weight: None, badges: None, last_seen: 0,
    };
    db.upsert(&rec).unwrap();

    let got = db.get(5).unwrap().unwrap();
    assert!(got.fname.is_none());
    assert!(got.lname.is_none());
    assert!(got.ftp.is_none());
    assert!(got.weight.is_none());
    assert!(got.badges.is_none());
}
