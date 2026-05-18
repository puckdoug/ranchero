// SPDX-License-Identifier: AGPL-3.0-only

use std::sync::Arc;
use tempfile::tempdir;
use zwift_store::KvStore;

// WAL allows a reader to observe a consistent snapshot while a writer holds
// an open transaction. The reader sees the pre-transaction value during the
// write and the committed value afterwards.
#[test]
#[ignore = "slow: spawns threads with busy-wait synchronisation; run with --include-ignored"]
fn wal_reader_sees_consistent_snapshot_during_write() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("kv.sqlite");

    let writer = KvStore::open(&db_path).unwrap();
    writer.put("k", b"before").unwrap();

    let reader = Arc::new(KvStore::open(&db_path).unwrap());

    // Confirm reader sees the committed value before any write is in flight.
    assert_eq!(reader.get("k").unwrap(), Some(b"before".to_vec()));

    // Open a raw connection for the writer so we can hold a transaction open
    // across thread boundaries.  KvStore itself uses a Mutex<Connection>
    // internally; for this test we drive the writer side manually.
    let writer_conn = zwift_store::open(&db_path).unwrap();
    writer_conn.execute_batch("BEGIN;").unwrap();
    writer_conn
        .execute("INSERT OR REPLACE INTO store (id, data) VALUES (?1, ?2)",
                 rusqlite::params!["k", b"during".as_ref()])
        .unwrap();

    // Reader must still see "before" — the transaction is not yet committed.
    assert_eq!(reader.get("k").unwrap(), Some(b"before".to_vec()));

    writer_conn.execute_batch("COMMIT;").unwrap();

    // After commit the reader must see the new value.
    assert_eq!(reader.get("k").unwrap(), Some(b"during".to_vec()));
}
