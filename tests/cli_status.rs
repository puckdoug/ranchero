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

    // Minimal byte sequences so file sizes are non-zero but we don't need a
    // live connection — status reads sizes only.
    std::fs::write(dir.path().join("store.sqlite"), b"SQLite format 3\x00").unwrap();
    std::fs::write(dir.path().join("athletes.sqlite"), b"SQLite format 3\x00").unwrap();
    std::fs::write(dir.path().join("segments.sqlite"), b"SQLite format 3\x00").unwrap();

    let out = Command::new(binary_path())
        .env("RANCHERO_DATA_DIR", dir.path())
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
