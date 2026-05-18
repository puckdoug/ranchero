// SPDX-License-Identifier: AGPL-3.0-only

use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::tempdir;
use zwift_store::SegmentsDb;

fn ts(secs: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(secs)
}

#[test]
fn evict_repeated_on_fully_expired_db_returns_zero_second_time() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    db.put(1, b"x", Duration::from_secs(100), ts(0)).unwrap(); // expires 100

    let first  = db.evict_expired(ts(200)).unwrap();
    let second = db.evict_expired(ts(200)).unwrap();

    assert_eq!(first,  1);
    assert_eq!(second, 0);
}

#[test]
fn evict_does_not_touch_rows_that_expire_in_the_future() {
    let dir = tempdir().unwrap();
    let db = SegmentsDb::open(&dir.path().join("segments.sqlite")).unwrap();

    db.put(1, b"soon",   Duration::from_secs(50),  ts(0)).unwrap(); // expires 50
    db.put(2, b"later",  Duration::from_secs(500), ts(0)).unwrap(); // expires 500

    db.evict_expired(ts(100)).unwrap(); // evicts row 1
    db.evict_expired(ts(100)).unwrap(); // second pass — nothing left to evict

    // Row 2 must survive both passes and still be readable.
    assert_eq!(db.get(2, ts(50)).unwrap(), Some(b"later".to_vec()));
}
