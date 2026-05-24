// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 2 — failing test for ProfileCache live-then-SQLite fallback.
//
// Red state: ProfileCache does not exist. The test will fail to compile
// until Step 2 implementation adds it to src/web/state.rs.
//
// The test exercises the two-layer lookup:
//   1. When a live profile has been inserted, get() returns it.
//   2. When no live profile is present, get() falls back to the AthletesDb.

use serde_json::json;
use tempfile::tempdir;
use zwift_store::{AthletesDb, AthleteRecord};
use ranchero::web::format::CachedProfile;
use ranchero::web::state::ProfileCache;

// S2-F: live data takes precedence; when live data is absent the SQLite
// record is returned instead.
#[test]
fn profile_cache_serves_live_then_falls_back_to_sqlite() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("athletes.sqlite");

    // Pre-populate SQLite with a profile at FTP 150.
    {
        let db = AthletesDb::open(&db_path).unwrap();
        db.upsert(&AthleteRecord {
            id:        42,
            data:      json!({ "firstName": "SqliteRider", "ftp": 150 }),
            last_seen: 0,
        }).unwrap();
    }

    // Phase 1: cache with a live entry at FTP 300 — live takes precedence.
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

    // Phase 2: fresh cache with no live data — SQLite fallback is used.
    {
        let cache = ProfileCache::new(AthletesDb::open(&db_path).unwrap());

        let got = cache.get(42).expect("SQLite fallback must return a profile");
        assert_eq!(
            got.ftp,
            Some(150),
            "SQLite record must be returned when no live data is available",
        );
        assert_eq!(
            got.first_name.as_deref(),
            Some("SqliteRider"),
            "SQLite record name must be returned when no live data is available",
        );
    }
}
