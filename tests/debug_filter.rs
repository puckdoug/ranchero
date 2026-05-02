//! STEP-12.12 Phase 0a — `--debug` must enable `debug`-level events for
//! the underlying crates (`zwift_relay::*`, `zwift_api::*`), not just
//! the daemon target. See section "Cross-cutting work needed before
//! the events are useful" in `docs/plans/STEP-12.12-log-shit-properly.md`.
//!
//! Today `filter_directive` returns `"info,ranchero=debug"` when
//! `--debug` is set. That admits `debug` events for `ranchero::*` only;
//! events emitted by `zwift_relay::*` and `zwift_api::*` (every TCP /
//! UDP / auth tracing site added by phases 1–5) are filtered to `info`
//! and disappear. Phase 0b must extend the directive so all three
//! crates are admitted at `debug`. This test fails red until then.

use std::sync::{Arc, Mutex};

use ranchero::logging::{LogOpts, filter_directive};
use tracing::Level;
use tracing_subscriber::{Layer, layer::SubscriberExt};

#[derive(Default, Clone)]
struct CaptureLayer {
    events: Arc<Mutex<Vec<(Level, String)>>>,
}

impl<S> Layer<S> for CaptureLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let metadata = event.metadata();
        self.events
            .lock()
            .expect("capture mutex")
            .push((*metadata.level(), metadata.target().to_string()));
    }
}

#[test]
fn debug_flag_enables_debug_for_underlying_crates() {
    // 1. Directive content — Phase 0b must add zwift_relay=debug and
    //    zwift_api=debug alongside the existing ranchero=debug rule.
    let directive =
        filter_directive(LogOpts { verbose: false, debug: true }, true, None, None);

    assert!(
        directive.contains("ranchero=debug"),
        "STEP-12.12 Phase 0a: --debug must keep admitting ranchero=debug; got {directive:?}",
    );
    assert!(
        directive.contains("zwift_relay=debug"),
        "STEP-12.12 Phase 0a: --debug must enable debug for zwift_relay::*; got {directive:?}",
    );
    assert!(
        directive.contains("zwift_api=debug"),
        "STEP-12.12 Phase 0a: --debug must enable debug for zwift_api::*; got {directive:?}",
    );

    // 2. Behaviour — install the EnvFilter the daemon would build and
    //    confirm it actually admits debug events for each target. This
    //    catches a 0b that puts the right substring in the directive
    //    but somehow gets the level wrong.
    let env_filter = tracing_subscriber::EnvFilter::try_new(&directive)
        .expect("filter directive must parse");
    let capture = CaptureLayer::default();
    let subscriber = tracing_subscriber::registry()
        .with(env_filter)
        .with(capture.clone());

    tracing::subscriber::with_default(subscriber, || {
        tracing::debug!(target: "ranchero::relay", "ranchero-debug-event");
        tracing::debug!(target: "zwift_relay::tcp", "zwift-relay-debug-event");
        tracing::debug!(target: "zwift_api::auth", "zwift-api-debug-event");
    });

    let events = capture.events.lock().expect("capture mutex");
    let saw_ranchero = events
        .iter()
        .any(|(_, t)| t.starts_with("ranchero"));
    let saw_zwift_relay = events
        .iter()
        .any(|(_, t)| t.starts_with("zwift_relay"));
    let saw_zwift_api = events
        .iter()
        .any(|(_, t)| t.starts_with("zwift_api"));

    assert!(
        saw_ranchero,
        "STEP-12.12 Phase 0a: filter must admit debug events for ranchero::*; \
         captured = {events:?}",
    );
    assert!(
        saw_zwift_relay,
        "STEP-12.12 Phase 0a: filter must admit debug events for zwift_relay::*; \
         captured = {events:?}",
    );
    assert!(
        saw_zwift_api,
        "STEP-12.12 Phase 0a: filter must admit debug events for zwift_api::*; \
         captured = {events:?}",
    );
}
