// SPDX-License-Identifier: AGPL-3.0-only

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;
use zwift_store::SegmentsDb;

fn ts(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[test]
fn evict_expired_deletes_expired_rows_and_returns_count() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    db.put(1, b"a", Duration::from_secs(100), ts(1000)).unwrap(); // expires 1100
    db.put(2, b"b", Duration::from_secs(200), ts(1000)).unwrap(); // expires 1200
    db.put(3, b"c", Duration::from_secs(500), ts(1000)).unwrap(); // expires 1500

    // now = 1200 — rows 1 and 2 have expires_at <= 1200, row 3 has not.
    let deleted = db.evict_expired(ts(1200)).unwrap();
    assert_eq!(deleted, 2);

    // Rows 1 and 2 must be gone; row 3 must still be present and readable.
    assert_eq!(db.get(1, ts(1100)).unwrap(), None);
    assert_eq!(db.get(2, ts(1100)).unwrap(), None);
    assert_eq!(db.get(3, ts(1100)).unwrap(), Some(b"c".to_vec()));
}

#[test]
fn evict_on_empty_db_returns_zero() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    assert_eq!(db.evict_expired(ts(9999)).unwrap(), 0);
}

#[test]
fn evict_leaves_non_expired_rows_intact() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    db.put(10, b"keep", Duration::from_secs(1000), ts(0)).unwrap(); // expires 1000

    // now = 999 — row has not expired yet.
    let deleted = db.evict_expired(ts(999)).unwrap();
    assert_eq!(deleted, 0);
    assert_eq!(db.get(10, ts(500)).unwrap(), Some(b"keep".to_vec()));
}
