//! Tests for `print_auth_check_to`.
//!
//! The original red-state tests (Defect 9) verified that auth-check prints the
//! configured `zwift_endpoints` instead of hard-coded defaults.
//!
//! Tests AC-1 and AC-2 (STEP-12.9 §Item-2) are in red state: they fail to
//! compile until `ResolvedConfig` gains a `watched_athlete_id: Option<u64>`
//! field and `print_auth_check_to` prints it.

use std::path::PathBuf;

use ranchero::cli::print_auth_check_to;
use ranchero::config::{EditingMode, RedactedString, ResolvedConfig, ZwiftEndpoints};
use ranchero::credentials::InMemoryKeyringStore;

fn make_config() -> ResolvedConfig {
    ResolvedConfig {
        main_email: Some("rider@example.com".to_string()),
        main_password: Some(RedactedString::new("secret".to_string())),
        monitor_email: None,
        monitor_password: None,
        server_bind: "127.0.0.1".into(),
        server_port: 1080,
        server_https: false,
        log_level: None,
        log_file: PathBuf::from("/tmp/ranchero-auth-check.log"),
        pidfile: PathBuf::from("/tmp/ranchero-auth-check.pid"),
        config_path: None,
        editing_mode: EditingMode::Default,
        zwift_endpoints: ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base: "http://127.0.0.1:1".into(),
        },
        relay_enabled: false,
        watched_athlete_id: None,
    }
}

#[test]
fn auth_check_reports_configured_endpoints_not_defaults() {
    let mut resolved = make_config();
    resolved.zwift_endpoints = ZwiftEndpoints {
        auth_base: "http://auth.staging.example.com".into(),
        api_base: "http://api.staging.example.com".into(),
    };

    let keyring = InMemoryKeyringStore::default();
    let mut buf = Vec::<u8>::new();
    print_auth_check_to(&mut buf, &resolved, &keyring)
        .expect("print_auth_check_to must not fail");

    let output = String::from_utf8(buf).unwrap();
    assert!(
        output.contains("http://auth.staging.example.com"),
        "auth-check must print the configured auth endpoint, got:\n{output}",
    );
    assert!(
        !output.contains("auth.zwift.com"),
        "auth-check must not print the default production URL, got:\n{output}",
    );
}

// STEP-12.9 §Item-2 — watched_athlete_id in auth-check output.
//
// AC-1 and AC-2 fail to compile until ResolvedConfig gains
// `watched_athlete_id: Option<u64>` and `print_auth_check_to` prints it.

// AC-1
#[test]
fn auth_check_reports_watched_athlete_id_when_set() {
    let mut resolved = make_config();
    resolved.watched_athlete_id = Some(123_456u64); // RED: field missing on ResolvedConfig
    let keyring = InMemoryKeyringStore::default();
    let mut buf = Vec::<u8>::new();
    print_auth_check_to(&mut buf, &resolved, &keyring)
        .expect("print_auth_check_to must not fail");
    let output = String::from_utf8(buf).unwrap();
    assert!(
        output.contains("123456"),
        "auth-check must print the watched athlete ID when set; got:\n{output}",
    );
}

// AC-2
#[test]
fn auth_check_reports_unset_when_watched_athlete_id_absent() {
    let mut resolved = make_config();
    resolved.watched_athlete_id = None; // RED: field missing on ResolvedConfig
    let keyring = InMemoryKeyringStore::default();
    let mut buf = Vec::<u8>::new();
    print_auth_check_to(&mut buf, &resolved, &keyring)
        .expect("print_auth_check_to must not fail");
    let output = String::from_utf8(buf).unwrap();
    assert!(
        output.contains("not set")
            || output.contains("none")
            || output.contains("not configured"),
        "auth-check must indicate when watched_athlete_id is not configured; got:\n{output}",
    );
}
