// SPDX-License-Identifier: AGPL-3.0-only

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use zwift_store::{AthletesDb, KvStore, SegmentsDb,
                  ATHLETES_FILENAME, SEGMENTS_FILENAME, STORE_FILENAME};

pub use zwift_store::Error as StoreError;

/// Default interval between `SegmentsDb::evict_expired` runs in the
/// daemon's scheduled-eviction task. Picked to be far longer than any
/// single segment leaderboard's TTL while still bounding the disk
/// footprint of expired rows; the exact value is not load-bearing.
pub const SEGMENTS_EVICT_TICK_INTERVAL_SECS: u64 = 300;

/// Periodically call [`SegmentsDb::evict_expired`] so the on-disk leaderboard
/// cache does not accumulate rows whose TTL has elapsed. Mirrors the
/// `gc_tick_loop` shape: the first tick is skipped so the evictor does not
/// run at startup before any rows have been written, and the eviction
/// `now` is read from `SystemTime::now()` to match the wall-clock the
/// fetchers use when computing `expires_at`.
pub async fn segments_evict_tick_loop(db: Arc<SegmentsDb>, interval: Duration) {
    let mut ticker = tokio::time::interval(interval);
    ticker.tick().await; // skip the immediate first tick
    loop {
        ticker.tick().await;
        let now = SystemTime::now();
        match db.evict_expired(now) {
            Ok(n) => tracing::debug!(
                target: "ranchero::stores",
                evicted = n,
                "segments_evict_tick",
            ),
            Err(e) => tracing::warn!(
                target: "ranchero::stores",
                error = %e,
                "segments_evict_tick: evict_expired failed",
            ),
        }
    }
}

/// Holds open handles to the three on-disk SQLite databases for the daemon's
/// lifetime. Opening runs migrations; all three files are created on first use.
pub struct Stores {
    pub kv:       KvStore,
    pub athletes: AthletesDb,
    pub segments: SegmentsDb,
}

impl Stores {
    pub fn open(data_dir: &Path) -> zwift_store::Result<Self> {
        let kv       = KvStore::open(&data_dir.join(STORE_FILENAME))?;
        let athletes = AthletesDb::open(&data_dir.join(ATHLETES_FILENAME))?;
        let segments = SegmentsDb::open(&data_dir.join(SEGMENTS_FILENAME))?;
        Ok(Stores { kv, athletes, segments })
    }
}
