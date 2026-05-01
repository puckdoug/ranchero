//! Red-state tests for Defect 9: `print_auth_check` reports
//! `Config::default()` URLs instead of `cfg.zwift_endpoints`.
//!
//! The test calls `ranchero::cli::print_auth_check_to`, which does not yet
//! exist. The target fails to compile until `print_auth_check` is refactored
//! to accept a writer and reads its endpoint URLs from the resolved config.

use std::path::PathBuf;

use ranchero::cli::print_auth_check_to;
use ranchero::config::{EditingMode, LogLevel, RedactedString, ResolvedConfig, ZwiftEndpoints};
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
        log_level: LogLevel::Info,
        log_file: PathBuf::from("/tmp/ranchero-auth-check.log"),
        pidfile: PathBuf::from("/tmp/ranchero-auth-check.pid"),
        config_path: None,
        editing_mode: EditingMode::Default,
        zwift_endpoints: ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base: "http://127.0.0.1:1".into(),
        },
        relay_enabled: false,
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
