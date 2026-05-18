// SPDX-License-Identifier: AGPL-3.0-only

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;
use zwift_store::SegmentsDb;

fn ts(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[test]
fn put_and_get_returns_payload_before_expiry() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    db.put(42, b"leaderboard-data", Duration::from_secs(300), ts(1000)).unwrap();

    // now = 1100 < expires_at 1300 — should return the payload.
    let got = db.get(42, ts(1100)).unwrap();
    assert_eq!(got, Some(b"leaderboard-data".to_vec()));
}

#[test]
fn get_returns_none_after_expiry() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    db.put(42, b"stale", Duration::from_secs(300), ts(1000)).unwrap();

    // now = expires_at — must be treated as expired.
    assert_eq!(db.get(42, ts(1300)).unwrap(), None);

    // now > expires_at — also expired.
    assert_eq!(db.get(42, ts(9999)).unwrap(), None);
}

#[test]
fn get_returns_none_for_absent_segment_id() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    assert_eq!(db.get(99, ts(1000)).unwrap(), None);
}

#[test]
fn put_overwrites_existing_entry() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    db.put(1, b"old", Duration::from_secs(300), ts(1000)).unwrap();
    db.put(1, b"new", Duration::from_secs(300), ts(1000)).unwrap();

    assert_eq!(db.get(1, ts(1100)).unwrap(), Some(b"new".to_vec()));
}

#[test]
fn binary_payload_round_trips_without_corruption() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    let payload: Vec<u8> = (0u8..=255).collect();
    db.put(7, &payload, Duration::from_secs(3600), ts(0)).unwrap();

    assert_eq!(db.get(7, ts(1)).unwrap(), Some(payload));
}
