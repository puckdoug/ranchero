// SPDX-License-Identifier: AGPL-3.0-only

// 17.35-T: `ranchero status` reports the web-server bind, port, HTTPS
// state, and active connection count under a "Web server" section, even
// when the daemon is not running. In the not-running case the bind and
// port come from `ResolvedConfig` and the connection count reads
// "daemon not running".
//
// Fails at runtime (missing output section) until 17.35-I extends
// runtime::status to print the Web server block.
//
// See docs/plans/STEP-17-web-server.md, item 17.35-T.

use std::process::{Command, Stdio};

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_ranchero")
}

#[ignore = "slow: spawns the ranchero binary (~0.85 s cold-start)"]
#[test]
fn status_reports_web_server_section_when_daemon_not_running() {
    let dir = tempfile::tempdir().unwrap();

    // Config isolates keyring lookups to a test-only service that has no
    // entries, so the binary never touches the operator's real credentials.
    // A specific server port is set so the assertion can match a concrete
    // value rather than the 1080 default.
    let pidfile_path = dir.path().join("ranchero.pid");
    let config_path  = dir.path().join("ranchero.toml");
    std::fs::write(&config_path, format!(
        "schema_version = 1\n\
         [daemon]\n\
         pidfile = \"{}\"\n\
         [server]\n\
         port = 9931\n\
         [relay]\n\
         enabled = false\n\
         [keyring]\n\
         service = \"ranchero-test-isolated\"\n",
        pidfile_path.display(),
    )).unwrap();

    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();

    let out = Command::new(binary_path())
        .env("RANCHERO_DATA_DIR", &data_dir)
        .args(["--config", &config_path.to_string_lossy()])
        .arg("status")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn ranchero status");

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );

    assert!(
        combined.to_lowercase().contains("web server"),
        "expected a 'Web server' section in status output, got: {combined}",
    );
    assert!(
        combined.contains("127.0.0.1"),
        "expected the bind address '127.0.0.1' in status output, got: {combined}",
    );
    assert!(
        combined.contains("9931"),
        "expected the configured port '9931' in status output, got: {combined}",
    );
    assert!(
        combined.to_lowercase().contains("https"),
        "expected an 'https' field in the Web server section, got: {combined}",
    );
    assert!(
        combined.contains("daemon not running"),
        "expected connections to read 'daemon not running' when the daemon \
         is down, got: {combined}",
    );
}
