// SPDX-License-Identifier: AGPL-3.0-only

// 16.17-T: `ranchero status` reports on-disk sizes of all three DB files under
// a "Persistence" section even when the daemon is not running.
//
// Fails at runtime (missing output section) until 16.17-I extends
// runtime::status to print the Persistence block.

use std::process::{Command, Stdio};

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_ranchero")
}

#[ignore = "slow: spawns the ranchero binary (~0.85 s cold-start)"]
#[test]
fn status_reports_persistence_section_when_daemon_not_running() {
    let dir = tempfile::tempdir().unwrap();

    // Config isolates keyring lookups to a test-only service that has no
    // entries, so the binary never touches the operator's real credentials.
    let pidfile_path = dir.path().join("ranchero.pid");
    let config_path  = dir.path().join("ranchero.toml");
    std::fs::write(&config_path, format!(
        "schema_version = 1\n\
         [daemon]\n\
         pidfile = \"{}\"\n\
         [relay]\n\
         enabled = false\n\
         [keyring]\n\
         service = \"ranchero-test-isolated\"\n",
        pidfile_path.display(),
    )).unwrap();

    // Minimal byte sequences so file sizes are non-zero; status reads sizes
    // only and never opens a live connection.
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::write(data_dir.join("store.sqlite"),    b"SQLite format 3\x00").unwrap();
    std::fs::write(data_dir.join("athletes.sqlite"), b"SQLite format 3\x00").unwrap();
    std::fs::write(data_dir.join("segments.sqlite"), b"SQLite format 3\x00").unwrap();

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
        combined.to_lowercase().contains("persistence"),
        "expected 'Persistence' section in status output, got: {combined}",
    );
    assert!(
        combined.contains("store.sqlite"),
        "expected 'store.sqlite' in status output, got: {combined}",
    );
    assert!(
        combined.contains("athletes.sqlite"),
        "expected 'athletes.sqlite' in status output, got: {combined}",
    );
    assert!(
        combined.contains("segments.sqlite"),
        "expected 'segments.sqlite' in status output, got: {combined}",
    );
}
