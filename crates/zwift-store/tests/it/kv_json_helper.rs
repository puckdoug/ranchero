// SPDX-License-Identifier: AGPL-3.0-only

use tempfile::tempdir;
use serde_json::json;
use zwift_store::KvStore;

#[test]
fn put_json_and_get_json_round_trip_value() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    let val = json!({ "ftp": 280, "name": "Alice", "active": true });
    kv.put_json("profile", &val).unwrap();

    let got: serde_json::Value = kv.get_json("profile").unwrap().unwrap();
    assert_eq!(got, val);
}

#[test]
fn get_json_returns_none_for_absent_key() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    let got: Option<serde_json::Value> = kv.get_json("absent").unwrap();
    assert!(got.is_none());
}

#[test]
fn put_json_overwrites_previous_value() {
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    kv.put_json("k", &json!(1)).unwrap();
    kv.put_json("k", &json!(2)).unwrap();

    let got: serde_json::Value = kv.get_json("k").unwrap().unwrap();
    assert_eq!(got, json!(2));
}

#[test]
fn json_and_raw_bytes_share_the_same_column() {
    // put_json stores bytes that get returns verbatim — the column is BLOB,
    // not TEXT, so raw and JSON access are interchangeable at the byte level.
    let dir = tempdir().unwrap();
    let kv = KvStore::open(&dir.path().join("kv.sqlite")).unwrap();

    kv.put_json("k", &json!("hello")).unwrap();
    let raw = kv.get("k").unwrap().unwrap();
    // serde_json serialises a string as `"hello"` (with quotes).
    assert_eq!(raw, b"\"hello\"");
}
