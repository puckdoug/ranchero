// SPDX-License-Identifier: AGPL-3.0-only
//
// `WorldTimer` — local clock aligned to Zwift's "world time" epoch
// (`ZWIFT_EPOCH_MS`, spec §4.3). The offset is adjusted by the UDP
// channel's SNTP-style sync (and optionally by a one-shot coarse
// correction at relay-login time per `zwift.mjs:1644-1648`).
//
// Mirrors `class WorldTimer` at `zwift.mjs:89-123`. Cloneable
// handle: the adjustable state lives behind `Arc<Mutex<…>>` so
// clones share one corrected clock across multiple channels and
// downstream consumers (STEP 12 GameMonitor, STEP 13+ stats).
//
// This file currently exposes the public surface as stubs so
// `tests/world_timer.rs` compiles. Implementation lands in green
// state. See `docs/plans/STEP-10-udp-channel.md`.

use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct WorldTimer {
    inner: Arc<Mutex<State>>,
}

#[allow(dead_code)]
struct State {
    offset_ms: i64,
}

impl WorldTimer {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(State { offset_ms: 0 })),
        }
    }

    /// Milliseconds since the Zwift world-time epoch
    /// (`ZWIFT_EPOCH_MS`). What protocol `worldTime` fields use.
    pub fn now(&self) -> i64 {
        unimplemented!("STEP-10: SystemTime::now() + offset - ZWIFT_EPOCH_MS")
    }

    /// Milliseconds since the Unix epoch, with the local offset
    /// applied. Useful for log timestamps that should reflect the
    /// corrected wall clock.
    pub fn server_now(&self) -> i64 {
        unimplemented!("STEP-10: SystemTime::now() + offset")
    }

    /// Shift the cumulative offset by `diff_ms`. Logs a warning at
    /// `tracing::warn` if `|diff_ms| > 5000`, mirroring sauce's
    /// `zwift.mjs:119-121`.
    pub fn adjust_offset(&self, _diff_ms: i64) {
        unimplemented!("STEP-10: lock state, add diff to offset_ms, log if large")
    }

    /// Current cumulative offset in milliseconds (for tests /
    /// observability).
    pub fn offset_ms(&self) -> i64 {
        unimplemented!("STEP-10: read offset_ms from state lock")
    }
}

impl Default for WorldTimer {
    fn default() -> Self {
        Self::new()
    }
}
