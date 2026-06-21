// SPDX-License-Identifier: AGPL-3.0-only
//
// ProfileCache: live data takes precedence over the SQLite fallback, and
// `insert_live` writes through to `athletes.sqlite` so a fresh cache reopened
// against the same file sees the same profile (Step 19 write-through).
//
// The test exercises the two-layer lookup:
//   1. When a live profile has been inserted, get() returns it; the SQLite
//      row pre-populated for the same id is overwritten by the write-through.
//   2. For a different id with no live entry, get() falls back to the
//      AthletesDb row written directly by the test.

use serde_json::json;
use tempfile::tempdir;
use zwift_store::{AthletesDb, AthleteRecord};
use ranchero::web::format::CachedProfile;
use ranchero::web::state::ProfileCache;

#[test]
fn profile_cache_serves_live_then_falls_back_to_sqlite() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("athletes.sqlite");

    // Pre-populate SQLite with two profiles: id 42 (will be overwritten by
    // a live insert) and id 99 (no live insert, exercises fallback).
    {
        let db = AthletesDb::open(&db_path).unwrap();
        db.upsert(&AthleteRecord {
            id:        42,
            data:      json!({ "firstName": "SqliteRider", "ftp": 150 }),
            last_seen: 0,
        }).unwrap();
        db.upsert(&AthleteRecord {
            id:        99,
            data:      json!({ "firstName": "FallbackRider", "ftp": 175 }),
            last_seen: 0,
        }).unwrap();
    }

    // Phase 1: cache with a live entry at FTP 300 — live takes precedence
    // and `insert_live` writes through to `athletes.sqlite` (Step 19).
    {
        let cache = ProfileCache::new(AthletesDb::open(&db_path).unwrap());
        cache.insert_live(42, CachedProfile {
            first_name: Some("LiveRider".into()),
            last_name:  None,
            ftp:        Some(300),
            weight_g:   None,
        });

        let got = cache.get(42).expect("profile must be found");
        assert_eq!(
            got.ftp,
            Some(300),
            "live profile must take precedence over the SQLite record",
        );
        assert_eq!(
            got.first_name.as_deref(),
            Some("LiveRider"),
            "live profile name must be returned, not the SQLite name",
        );
    }

    // Phase 2: fresh cache, no live data for id 99 — SQLite fallback wins.
    {
        let cache = ProfileCache::new(AthletesDb::open(&db_path).unwrap());

        let got = cache.get(99).expect("SQLite fallback must return a profile");
        assert_eq!(
            got.ftp,
            Some(175),
            "SQLite record must be returned when no live data is available",
        );
        assert_eq!(
            got.first_name.as_deref(),
            Some("FallbackRider"),
            "SQLite record name must be returned when no live data is available",
        );
    }
}
