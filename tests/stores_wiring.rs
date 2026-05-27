// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 19 — Persistence wiring audit.
//
// `Stores::open` runs at daemon start, but today `run_daemon` binds the
// result as `_stores` and nothing in the runtime/relay/web layers calls
// `upsert`/`get`/`put`/`evict_expired` on those handles. This test pins
// the wiring at the `WebState` boundary: once Step 19 lands, `WebState`
// holds the opened `ProfileCache` (over `AthletesDb`) and `SegmentsDb`,
// production callers reach the DBs through those fields, and a live
// profile inserted through the cache reaches `athletes.sqlite` on disk.
//
// Red state today:
//   - `WebState` has no `profile_cache` accessor / no `with_stores` builder.
//   - `ProfileCache::insert_live` does not write through to `AthletesDb`.
// Both must be wired so the assertion below passes against the on-disk file.
//
// See docs/planning/STEP-20-additional-considerations.md, Step 19 / 20.28
// item 3 / QE3.

use tempfile::tempdir;
use zwift_store::{AthletesDb, ATHLETES_FILENAME};
use ranchero::daemon::Stores;
use ranchero::web::WebState;
use ranchero::web::format::CachedProfile;

#[test]
fn stores_read_and_written_in_production() {
    let dir = tempdir().expect("tempdir");

    // Production wiring: open Stores, attach them to WebState the same
    // way `run_daemon` should once Step 19 lands.
    let stores = Stores::open(dir.path()).expect("Stores::open");
    let state  = WebState::new().with_stores(stores);

    // Drive a production-shaped write: a freshly-fetched profile lands
    // in the live layer, and the cache writes through to the SQLite
    // fallback so the row survives a daemon restart.
    state.profile_cache().insert_live(42, CachedProfile {
        first_name: Some("Wired".into()),
        last_name:  Some("Through".into()),
        ftp:        Some(275),
        weight_g:   Some(70_000),
    });

    // Drop the state so the SQLite handle flushes before we re-open
    // the file from disk for the assertion.
    drop(state);

    let db_path = dir.path().join(ATHLETES_FILENAME);
    let db = AthletesDb::open(&db_path).expect("reopen athletes.sqlite");
    let row = db
        .get(42)
        .expect("get must not error")
        .expect("athletes.sqlite must contain the row written through the cache");

    assert_eq!(
        row.data["firstName"].as_str(),
        Some("Wired"),
        "write-through must persist the live profile's firstName",
    );
    assert_eq!(
        row.data["ftp"].as_u64(),
        Some(275),
        "write-through must persist the live profile's FTP",
    );
}
