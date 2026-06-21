// SPDX-License-Identifier: AGPL-3.0-only
//! 17.33-T — The GC tick interval matches the value sauce4zwift uses for
//! `_gcAthleteData`: 62 768 milliseconds (62.768 seconds).
//!
//! A 10-second interval was considered and rejected: it would run GC
//! seventeen times as often as the upstream implementation for no benefit,
//! and would make the paused-time test in 17.32-T require a smaller advance
//! value that diverges from the documented source cadence.
//!
//! The doc-comment that names `_gcAthleteData` and quotes the `62768 ms`
//! literal is added by 17.33-I — it is a code-review item, not a runtime
//! assertion.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.33-T.

use zwift_stats::GC_TICK_INTERVAL_SECS;

#[test]
fn gc_tick_interval_matches_sauce4zwift_gc_athlete_data() {
    assert_eq!(
        GC_TICK_INTERVAL_SECS,
        62.768,
        "GC_TICK_INTERVAL_SECS must equal 62.768 s (62 768 ms), \
         matching sauce4zwift's _gcAthleteData setInterval literal; \
         a 10-second interval was considered and rejected",
    );
}
