// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use zwift_store::KvStore;

#[test]
fn insert_and_get_returns_same_bytes() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    kv.put("key1", b"hello world").unwrap();

    let got = kv.get("key1").unwrap();
    assert_eq!(got, Some(b"hello world".to_vec()));
}

#[test]
fn get_missing_key_returns_none() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    assert_eq!(kv.get("absent").unwrap(), None);
}

#[test]
fn put_overwrites_existing_value() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    kv.put("k", b"first").unwrap();
    kv.put("k", b"second").unwrap();

    assert_eq!(kv.get("k").unwrap(), Some(b"second".to_vec()));
}

#[test]
fn delete_removes_key_and_returns_true() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    kv.put("k", b"v").unwrap();
    assert!(kv.delete("k").unwrap());
    assert_eq!(kv.get("k").unwrap(), None);
}

#[test]
fn delete_absent_key_returns_false() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    assert!(!kv.delete("absent").unwrap());
}

#[test]
fn exists_returns_true_for_present_key() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    kv.put("k", b"v").unwrap();
    assert!(kv.exists("k").unwrap());
}

#[test]
fn exists_returns_false_for_absent_key() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    assert!(!kv.exists("absent").unwrap());
}

#[test]
fn binary_data_round_trips_without_corruption() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    // Bytes covering the full 0x00-0xFF range to confirm BLOB safety.
    let data: Vec<u8> = (0u8..=255).collect();
    kv.put("bin", &data).unwrap();

    assert_eq!(kv.get("bin").unwrap(), Some(data));
}
