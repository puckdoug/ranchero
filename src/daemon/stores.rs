// SPDX-License-Identifier: AGPL-3.0-only

use std::path::Path;
use zwift_store::{AthletesDb, KvStore, SegmentsDb,
                  ATHLETES_FILENAME, SEGMENTS_FILENAME, STORE_FILENAME};

pub use zwift_store::Error as StoreError;

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
