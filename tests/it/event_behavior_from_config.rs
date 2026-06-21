// SPDX-License-Identifier: AGPL-3.0-only
//! 17.31-T — `EventBehavior` is surfaced on `ResolvedConfig` and its
//! defaults match sauce4zwift's `!!app.getSetting(...)` behaviour (both
//! `false` when the setting has never been stored).
//!
//! Two assertions:
//! 1. A `ConfigFile` with no `[stats]` table produces
//!    `EventBehavior { auto_reset: false, auto_lap: false }`.
//! 2. A `ConfigFile` with `[stats] auto_reset_events = true` and
//!    `auto_lap_events = false` produces
//!    `EventBehavior { auto_reset: true, auto_lap: false }`.
//!
//! Fails to compile until `StatsConfig` is added to `ConfigFile` and
//! `event_behavior: EventBehavior` is added to `ResolvedConfig`
//! (item 17.31-I).
//!
//! See docs/plans/STEP-17-web-server.md, item 17.31-T.

use ranchero::cli::GlobalOpts;
use ranchero::config::{ConfigFile, OsEnv, ResolvedConfig};
use ranchero::credentials::InMemoryKeyringStore;
use zwift_stats::EventBehavior;

fn resolve(file: ConfigFile) -> ResolvedConfig {
    ResolvedConfig::resolve(
        &GlobalOpts::default(),
        &OsEnv,
        &InMemoryKeyringStore::default(),
        Some(file),
    )
    .expect("resolve must not fail for a valid ConfigFile")
}

#[test]
fn absent_stats_table_gives_both_defaults_false() {
    let cfg = resolve(ConfigFile::default());
    assert_eq!(
        cfg.event_behavior,
        EventBehavior { auto_reset: false, auto_lap: false },
        "both event-behavior flags must default to false when [stats] is absent",
    );
}

#[test]
fn stats_table_overrides_are_respected() {
    let mut file = ConfigFile::default();
    file.stats.auto_reset_events = true;
    file.stats.auto_lap_events   = false;
    let cfg = resolve(file);
    assert_eq!(
        cfg.event_behavior,
        EventBehavior { auto_reset: true, auto_lap: false },
        "auto_reset must be true and auto_lap false per the TOML values",
    );
}
