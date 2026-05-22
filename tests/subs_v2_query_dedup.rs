// SPDX-License-Identifier: AGPL-3.0-only
//! 18.15-T — v2 subscription delegation is keyed by (source, event, query).
//!
//! Today the delegation key is `source/event` (`src/web/subs/mod.rs:98`).
//! After 18.15-I it is `source/event/canonical_query`, so two subscribers
//! with different `(resources, stats)` pairs have distinct delegations and
//! each formats once per emission rather than sharing one formatting pass.
//!
//! Two tests:
//! 1. `identical_v2_queries_share_one_delegation` — the happy-path: same
//!    (source, event, query) → same `Arc<DelegationHandle>`, one upstream
//!    broadcast receiver.
//! 2. `different_v2_queries_have_distinct_delegations` — the key new
//!    behaviour: different queries → distinct `Arc<DelegationHandle>`s and
//!    two upstream receivers.  Currently FAILS because the key ignores the
//!    query, so both subscriptions get the same delegation.
//!
//! Neither test uses a real socket; `DelegationMap::subscribe` is called
//! directly through a `WebState` that carries a live broadcast sender.
//!
//! Fails to compile until 18.15-I adds `AthleteV2Query` to
//! `ranchero::web::subs` and adds a `query` parameter to
//! `DelegationMap::subscribe`.
//!
//! See docs/plans/STEP-18-format-payloads.md, item 18.15-T.

use std::sync::Arc;
use tokio::sync::broadcast;
use ranchero::daemon::relay::GameEvent;
use ranchero::web::{AthleteRegistry, WebState};
use ranchero::web::subs::{AthleteV2Query, DelegationMap};

fn game_state() -> (broadcast::Sender<GameEvent>, Arc<WebState>) {
    let (tx, _) = broadcast::channel::<GameEvent>(4);
    let mut registry = AthleteRegistry::new();
    registry.upsert(1001, 5, 0, 0.0, 0.0);
    let state = Arc::new(
        WebState::with_registry(registry, Some(1001), Some(1001))
            .and_game_events(tx.clone()),
    );
    (tx, state)
}

/// Two subscribers with the same (source, event, query) must share one
/// `DelegationHandle` (same Arc) and one upstream broadcast receiver.
///
/// This is the existing deduplication guarantee extended to v2 queries.
#[tokio::test]
async fn identical_v2_queries_share_one_delegation() {
    let (tx, state) = game_state();
    let map   = DelegationMap::new();
    let query = AthleteV2Query { resources: vec!["stats".into()], stats: false };

    let sub_a = map.subscribe("stats", "athlete/1001", &query, &state)
        .expect("first subscribe must succeed");
    let sub_b = map.subscribe("stats", "athlete/1001", &query, &state)
        .expect("second subscribe must succeed");

    assert!(Arc::ptr_eq(&sub_a.delegation, &sub_b.delegation),
        "identical v2 queries must share one DelegationHandle");
    assert_eq!(tx.receiver_count(), 1,
        "identical v2 queries must produce one upstream broadcast receiver");
}

/// Two subscribers with the same (source, event) but different queries must
/// have distinct `DelegationHandle`s and two upstream broadcast receivers.
///
/// Currently fails: the delegation key is `source/event` only, so both
/// subscriptions get the same delegation regardless of their queries.
#[tokio::test]
async fn different_v2_queries_have_distinct_delegations() {
    let (tx, state) = game_state();
    let map     = DelegationMap::new();
    let query_a = AthleteV2Query { resources: vec!["stats".into()], stats: false };
    let query_b = AthleteV2Query { resources: vec!["state".into()], stats: false };

    let sub_a = map.subscribe("stats", "athlete/1001", &query_a, &state)
        .expect("first subscribe must succeed");
    let sub_b = map.subscribe("stats", "athlete/1001", &query_b, &state)
        .expect("second subscribe must succeed");

    assert!(!Arc::ptr_eq(&sub_a.delegation, &sub_b.delegation),
        "different v2 queries must not share a DelegationHandle");
    // Currently fails: key ignores the query, so both get the same delegation
    // and only one upstream receiver is created.
    assert_eq!(tx.receiver_count(), 2,
        "different v2 queries must each have their own upstream broadcast receiver");
}
