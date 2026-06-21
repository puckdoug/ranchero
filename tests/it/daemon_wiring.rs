// SPDX-License-Identifier: AGPL-3.0-only

// 16.16-T: Daemon boot opens all three SQLite databases under a given data
// directory, creates the files if missing, and runs migrations.
//
// Fails to compile until 16.16-I adds ranchero::daemon::Stores.

use tempfile::tempdir;

#[test]
fn boot_opens_all_three_databases_and_creates_files() {
    let dir = tempdir().unwrap();

    // ranchero::daemon::Stores does not exist yet — compile error is the
    // expected RED state for this test.
    let _stores = ranchero::daemon::Stores::open(dir.path()).unwrap();

    assert!(
        dir.path().join("store.sqlite").exists(),
        "store.sqlite must exist after Stores::open",
    );
    assert!(
        dir.path().join("athletes.sqlite").exists(),
        "athletes.sqlite must exist after Stores::open",
    );
    assert!(
        dir.path().join("segments.sqlite").exists(),
        "segments.sqlite must exist after Stores::open",
    );
}
