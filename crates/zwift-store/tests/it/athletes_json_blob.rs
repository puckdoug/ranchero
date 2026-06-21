// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 2 — failing tests for the JSON-blob athletes schema.
//
// Red state: AthleteRecord still uses fixed columns (fname, lname, ftp,
// weight, badges). These tests reference the new JSON-blob API and will
// fail to compile until Step 2 implementation:
//   a) migrates the schema to athletes(id, data TEXT, last_seen)
//   b) updates AthleteRecord to { id, data: serde_json::Value, last_seen }
//   c) adds AthletesDb::marked()

use serde_json::json;
use tempfile::tempdir;
use zwift_store::{AthletesDb, AthleteRecord};

// S2-A: Upsert a full athlete JSON blob and read it back intact.
//
// The new schema stores the complete athlete record as a TEXT column (`data`),
// replacing the fixed fname/lname/ftp/weight/badges columns. Fields that sauce
// stores in athletes.sqlite — marked, following, gender, privacy — must survive
// the round-trip unchanged, because they have no dedicated column to fall back on.
#[test]
fn athletes_db_roundtrips_json_blob() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    let data = json!({
        "firstName": "Test",
        "lastName":  "Rider",
        "ftp":       220,
        "weight":    72000,
        "marked":    true,
        "following": false,
        "gender":    "male",
        "privacy":   { "displayAge": false, "displayWeight": false },
    });

    let rec = AthleteRecord { id: 42, data: data.clone(), last_seen: 1_000_000 };
    db.upsert(&rec).unwrap();

    let got = db.get(42).unwrap().expect("record must exist after upsert");
    assert_eq!(got.id, 42);
    assert_eq!(got.last_seen, 1_000_000);
    assert_eq!(got.data["firstName"], "Test",  "firstName must round-trip");
    assert_eq!(got.data["lastName"],  "Rider", "lastName must round-trip");
    assert_eq!(got.data["ftp"],       220,     "ftp must round-trip as a number");
    assert_eq!(got.data["marked"],    true,    "marked flag must round-trip");
    assert_eq!(got.data["following"], false,   "following flag must round-trip");
    assert_eq!(got.data["gender"],    "male",  "gender must round-trip");
    assert_eq!(
        got.data["privacy"]["displayAge"],
        false,
        "nested privacy fields must round-trip intact",
    );
}

// S2-B: marked() returns the IDs of every athlete whose data blob has
// marked: true, using SQLite's json_extract to filter without a dedicated
// column. Mirrors sauce4zwift's approach (stats.mjs:2440).
#[test]
fn athletes_db_marked_query() {
    let dir = tempdir().unwrap();
    let db = AthletesDb::open(&dir.path().join("athletes.sqlite")).unwrap();

    db.upsert(&AthleteRecord {
        id:        1,
        data:      json!({ "firstName": "MarkedYes", "marked": true }),
        last_seen: 0,
    }).unwrap();
    db.upsert(&AthleteRecord {
        id:        2,
        data:      json!({ "firstName": "MarkedNo", "marked": false }),
        last_seen: 0,
    }).unwrap();
    db.upsert(&AthleteRecord {
        id:        3,
        data:      json!({ "firstName": "NoMarkField" }),
        last_seen: 0,
    }).unwrap();

    let marked = db.marked().unwrap();

    assert!(
        marked.contains(&1),
        "id 1 (marked: true) must appear in marked(); got {marked:?}",
    );
    assert!(
        !marked.contains(&2),
        "id 2 (marked: false) must not appear in marked(); got {marked:?}",
    );
    assert!(
        !marked.contains(&3),
        "id 3 (no marked field) must not appear in marked(); got {marked:?}",
    );
}
