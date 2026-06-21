//! STEP-05 credential storage tests.
//!
//! These describe the behaviour of `ranchero::credentials`:
//!   - the on-disk JSON blob format (`{"username":"…","password":"…"}`,
//!     no whitespace, fields in insertion order);
//!   - the trait surface (`KeyringStore`) and an in-memory fake;
//!   - the role -> keychain account-name mapping (`main` -> `zwift-login`,
//!     `monitor` -> `zwift-monitor-login`);
//!   - the real OS-keychain backend (`OsKeyringStore`), gated by
//!     `#[cfg(target_os = ...)]` and `#[ignore]` so a plain `cargo test`
//!     does not touch the user's keychain.

use ranchero::credentials::{
    InMemoryKeyringStore, KeyringError, KeyringStore, SERVICE_NAME,
    account_name, parse_credentials, serialize_credentials,
};

// ---------------------------------------------------------------------------
// Service name constant — ranchero-isolated (does not share with other apps).
// ---------------------------------------------------------------------------

#[test]
fn service_name_is_ranchero() {
    assert_eq!(SERVICE_NAME, "ranchero");
}

// ---------------------------------------------------------------------------
// Role -> keychain account-name mapping.
//
// The KeyringStore trait talks in clean role IDs ("main", "monitor"); the
// real backend translates those to descriptive account names so a
// keychain inspector can tell what each entry is for.
// ---------------------------------------------------------------------------

#[test]
fn role_main_maps_to_zwift_login() {
    assert_eq!(account_name("main").unwrap(), "zwift-login");
}

#[test]
fn role_monitor_maps_to_zwift_monitor_login() {
    assert_eq!(account_name("monitor").unwrap(), "zwift-monitor-login");
}

#[test]
fn unknown_role_is_rejected() {
    let err = account_name("admin").unwrap_err();
    assert!(
        matches!(&err, KeyringError::UnknownRole(s) if s == "admin"),
        "expected UnknownRole(\"admin\"), got {err:?}",
    );
}

// ---------------------------------------------------------------------------
// JSON blob format.
//
// Compact JSON, no whitespace, fields in insertion order:
//
//   {"username":"<u>","password":"<p>"}
// ---------------------------------------------------------------------------

#[test]
fn serialize_produces_compact_json_blob() {
    let blob = serialize_credentials("rider@example.com", "hunter2").unwrap();
    assert_eq!(
        blob,
        r#"{"username":"rider@example.com","password":"hunter2"}"#,
    );
}

#[test]
fn serialize_field_order_is_username_then_password() {
    let blob = serialize_credentials("u", "p").unwrap();
    let u_idx = blob.find("\"username\"").unwrap();
    let p_idx = blob.find("\"password\"").unwrap();
    assert!(u_idx < p_idx, "username must come before password: {blob}");
}

#[test]
fn serialize_escapes_special_characters() {
    let blob = serialize_credentials("a\"b", "c\\d").unwrap();
    assert_eq!(blob, r#"{"username":"a\"b","password":"c\\d"}"#);
}

#[test]
fn serialize_emits_no_whitespace_or_pretty_printing() {
    let blob = serialize_credentials("u", "p").unwrap();
    assert!(!blob.contains('\n'), "no newlines: {blob}");
    assert!(!blob.contains(": "), "no space after colon: {blob}");
    assert!(!blob.contains(", "), "no space after comma: {blob}");
}

#[test]
fn parse_round_trip() {
    let blob = serialize_credentials("alice", "wonderland").unwrap();
    let entry = parse_credentials(&blob).unwrap();
    assert_eq!(entry.username, "alice");
    assert_eq!(entry.password, "wonderland");
}

#[test]
fn parse_tolerates_extra_fields_for_forward_compat() {
    // If a future ranchero version writes extra fields, we must still
    // recover the two we care about today.
    let blob = r#"{"username":"u","password":"p","captured_at":"2026-01-01"}"#;
    let entry = parse_credentials(blob).unwrap();
    assert_eq!(entry.username, "u");
    assert_eq!(entry.password, "p");
}

#[test]
fn parse_rejects_malformed_json() {
    let err = parse_credentials("not json").unwrap_err();
    assert!(matches!(err, KeyringError::Serialization(_)), "got {err:?}");
}

#[test]
fn parse_rejects_missing_username() {
    let err = parse_credentials(r#"{"password":"p"}"#).unwrap_err();
    assert!(matches!(err, KeyringError::Serialization(_)), "got {err:?}");
}

#[test]
fn parse_rejects_missing_password() {
    let err = parse_credentials(r#"{"username":"u"}"#).unwrap_err();
    assert!(matches!(err, KeyringError::Serialization(_)), "got {err:?}");
}

// ---------------------------------------------------------------------------
// In-memory fake — covers the full trait surface so any KeyringStore
// implementation (OS-backed or otherwise) is held to the same contract.
// ---------------------------------------------------------------------------

#[test]
fn in_memory_get_missing_returns_ok_none() {
    let store = InMemoryKeyringStore::default();
    assert!(store.get("main").unwrap().is_none());
}

#[test]
fn in_memory_round_trip() {
    let mut store = InMemoryKeyringStore::default();
    store.set("main", "u@example.com", "pw").unwrap();
    let got = store.get("main").unwrap().expect("entry should be present");
    assert_eq!(got.username, "u@example.com");
    assert_eq!(got.password, "pw");
}

#[test]
fn in_memory_set_overwrites_existing_entry() {
    let mut store = InMemoryKeyringStore::default();
    store.set("main", "old@example.com", "old").unwrap();
    store.set("main", "new@example.com", "new").unwrap();
    let got = store.get("main").unwrap().unwrap();
    assert_eq!(got.username, "new@example.com");
    assert_eq!(got.password, "new");
}

#[test]
fn in_memory_delete_removes_entry() {
    let mut store = InMemoryKeyringStore::default();
    store.set("main", "u", "p").unwrap();
    store.delete("main").unwrap();
    assert!(store.get("main").unwrap().is_none());
}

#[test]
fn in_memory_delete_missing_is_idempotent() {
    let mut store = InMemoryKeyringStore::default();
    store.delete("monitor").unwrap();
}

#[test]
fn in_memory_main_and_monitor_are_independent() {
    let mut store = InMemoryKeyringStore::default();
    store.set("main", "main@x", "mp").unwrap();
    store.set("monitor", "mon@x", "mon-p").unwrap();
    assert_eq!(store.get("main").unwrap().unwrap().username, "main@x");
    assert_eq!(store.get("monitor").unwrap().unwrap().username, "mon@x");
    store.delete("main").unwrap();
    assert!(store.get("main").unwrap().is_none());
    assert!(
        store.get("monitor").unwrap().is_some(),
        "deleting one role must not affect the other",
    );
}

// ---------------------------------------------------------------------------
// Real OS-keychain backend.
//
// These tests poke the actual OS secret store. They:
//   - require a host that provides one (macOS Keychain, Windows Credential
//     Manager, libsecret on Linux);
//   - on macOS specifically, may prompt the user to allow keychain access,
//     which would block a non-interactive `cargo test` run.
//
// They are therefore gated by both `#[cfg(target_os = ...)]` (compile only
// on supported platforms) and `#[ignore]` (require explicit
// `cargo test -- --ignored` to run, presumably from CI). They write under
// a `OsKeyringStore::with_service_name(...)` instance using a unique,
// disposable service name so they cannot collide with the user's real
// ranchero entries.
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
mod os_backend {
    use super::*;
    use ranchero::credentials::OsKeyringStore;

    /// A unique, disposable service name keeps test entries from colliding
    /// with the user's real ranchero credentials and from leaking across
    /// test runs.
    fn unique_service() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("ranchero-test-{nanos}")
    }

    #[test]
    #[ignore = "touches real OS keychain; run with `cargo test -- --ignored`"]
    fn os_round_trip() {
        let svc = unique_service();
        let mut store = OsKeyringStore::with_service_name(&svc);
        store.set("main", "u@example.com", "pw").unwrap();
        let got = store.get("main").unwrap().expect("entry should be present");
        assert_eq!(got.username, "u@example.com");
        assert_eq!(got.password, "pw");
        store.delete("main").unwrap();
        assert!(store.get("main").unwrap().is_none());
    }

    #[test]
    #[ignore = "touches real OS keychain; run with `cargo test -- --ignored`"]
    fn os_get_missing_returns_ok_none_not_error() {
        let svc = unique_service();
        let store = OsKeyringStore::with_service_name(&svc);
        assert!(store.get("main").unwrap().is_none());
    }

    #[test]
    #[ignore = "touches real OS keychain; run with `cargo test -- --ignored`"]
    fn os_delete_missing_is_idempotent() {
        let svc = unique_service();
        let mut store = OsKeyringStore::with_service_name(&svc);
        store.delete("main").unwrap();
    }

    #[test]
    #[ignore = "touches real OS keychain; run with `cargo test -- --ignored`"]
    fn os_main_and_monitor_are_independent() {
        let svc = unique_service();
        let mut store = OsKeyringStore::with_service_name(&svc);
        store.set("main", "main@x", "mp").unwrap();
        store.set("monitor", "mon@x", "mon-p").unwrap();
        store.delete("main").unwrap();
        assert!(store.get("main").unwrap().is_none());
        let mon = store.get("monitor").unwrap().expect("monitor preserved");
        assert_eq!(mon.username, "mon@x");
        store.delete("monitor").unwrap();
    }
}
