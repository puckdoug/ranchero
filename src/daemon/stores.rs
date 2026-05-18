// SPDX-License-Identifier: AGPL-3.0-only

use std::path::Path;
use zwift_store::{AthletesDb, KvStore, SegmentsDb};

pub use zwift_store::Error as StoreError;

const STORE_FILE:    &str = "store.sqlite";
const ATHLETES_FILE: &str = "athletes.sqlite";
const SEGMENTS_FILE: &str = "segments.sqlite";

/// Holds open handles to the three on-disk SQLite databases for the daemon's
/// lifetime. Opening runs migrations; all three files are created on first use.
pub struct Stores {
    pub kv:       KvStore,
    pub athletes: AthletesDb,
    pub segments: SegmentsDb,
}

impl Stores {
    pub fn open(data_dir: &Path) -> zwift_store::Result<Self> {
        let kv       = KvStore::open(&data_dir.join(STORE_FILE))?;
        let athletes = AthletesDb::open(&data_dir.join(ATHLETES_FILE))?;
        let segments = SegmentsDb::open(&data_dir.join(SEGMENTS_FILE))?;
        Ok(Stores { kv, athletes, segments })
    }

    pub fn store_filename()    -> &'static str { STORE_FILE }
    pub fn athletes_filename() -> &'static str { ATHLETES_FILE }
    pub fn segments_filename() -> &'static str { SEGMENTS_FILE }
}
