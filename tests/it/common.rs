// SPDX-License-Identifier: AGPL-3.0-only
//! Shared helpers for root-crate integration tests.

use std::path::PathBuf;
use ranchero::config::{EditingMode, ResolvedConfig, ZwiftEndpoints};

/// Return a minimal `ResolvedConfig` suitable for integration tests.
///
/// `name` is used to make the log-file and pidfile paths unique per caller,
/// avoiding conflicts when tests run in parallel.
#[allow(dead_code)]
pub fn test_config(name: &str) -> ResolvedConfig {
    ResolvedConfig {
        main_email:            None,
        main_password:         None,
        monitor_email:         None,
        monitor_password:      None,
        server_bind:           "127.0.0.1".into(),
        server_port:           0,
        server_https:          false,
        log_level:             None,
        log_file:              PathBuf::from(format!("/tmp/ranchero-{name}-test.log")),
        pidfile:               PathBuf::from(format!("/tmp/ranchero-{name}-test.pid")),
        config_path:           None,
        editing_mode:          EditingMode::Default,
        zwift_endpoints:       ZwiftEndpoints {
            auth_base: "http://127.0.0.1:1".into(),
            api_base:  "http://127.0.0.1:1".into(),
        },
        relay_enabled:         false,
        watched_athlete_id:    None,
        server_pages_root:     PathBuf::from("pages"),
        server_https_cert_dir: PathBuf::from("https"),
        event_behavior:        Default::default(),
    }
}
