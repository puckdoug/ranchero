// SPDX-License-Identifier: AGPL-3.0-only
//! Step 16 — RPC read-only getter surface.
//!
//! Each test exercises one handler group via `RpcRegistry::dispatch` and
//! asserts the response is registered, succeeds, and carries the expected
//! shape.  Write-facing RPCs (`setFollowing`, `giveRideon`, `updateAthlete`,
//! …) must NOT be registered — this is enforced explicitly by
//! `write_rpcs_not_registered`, matching the QA1 decision in
//! `docs/planning/STEP-20-additional-considerations.md` (Step 16 plan).
//!
//! These tests fail to compile until Step 16's implementation lands a
//! constructor that builds an `RpcRegistry` populated with the read-only
//! handler set from a shared `WebState`.  The proposed shape is
//! `RpcRegistry::with_state(state: Arc<WebState>)`; the implementer is
//! free to pick a different factory name so long as it produces an
//! `RpcRegistry` whose handlers resolve against the supplied state.
//!
//! No real socket, no daemon — these are fast tests.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde_json::{json, Value};

use ranchero::web::{RpcRegistry, WebState};
use zwift_stats::athlete::EventSubgroup;
use zwift_stats::AthleteRegistry;

// ---------------------------------------------------------------------------
// Test fixture
// ---------------------------------------------------------------------------

const ATHLETE_ID:  u32 = 1001;
const OTHER_ID:    u32 = 1002;
const SUBGROUP_ID: u32 = 4242;

/// Build a `WebState` with one athlete in the registry, the watched+self ids
/// set to that athlete, and a single event subgroup in the cache.
fn seeded_state() -> Arc<WebState> {
    let mut reg = AthleteRegistry::new();
    let _ = reg.upsert(ATHLETE_ID, 5, 0, 0.0, 0.0);
    let _ = reg.upsert(OTHER_ID,   5, 0, 0.0, 0.0);

    let state = WebState::with_registry(reg, Some(ATHLETE_ID), Some(ATHLETE_ID));

    // Pre-populate the event-subgroup cache so event getters have data.
    {
        let mut m: std::sync::RwLockWriteGuard<HashMap<u32, EventSubgroup>> =
            state.event_subgroups.write().unwrap();
        m.insert(SUBGROUP_ID, EventSubgroup {
            id:        SUBGROUP_ID,
            course_id: 5,
            ..Default::default()
        });
    }

    // Pre-populate the game-state snapshot so `getGameState` has content.
    *state.game_state.lock().unwrap() = Some(json!({"playerId": ATHLETE_ID}));

    Arc::new(state)
}

/// Build the RPC registry for a state.  Tests bind to this single seam so the
/// implementer can rename the factory without touching every test body.
fn build_registry(state: Arc<WebState>) -> RpcRegistry {
    RpcRegistry::with_state(state)
}

async fn must_dispatch(rpc: &RpcRegistry, name: &str, args: Vec<Value>) -> Value {
    let res = rpc.dispatch(name, args).await
        .unwrap_or_else(|| panic!("rpc handler `{name}` is not registered"));
    res.unwrap_or_else(|e| panic!("rpc handler `{name}` returned error: {e}"))
}

fn _silence_rwlock_unused() {
    // Suppress "unused import" if RwLock isn't otherwise referenced after
    // the type-annotated lock above.
    let _: Option<RwLock<()>> = None;
}

// ---------------------------------------------------------------------------
// Athlete getters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_athlete_returns_profile_shape() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getAthlete", vec![json!(ATHLETE_ID)]).await;
    assert!(v.is_object() || v.is_null(),
        "getAthlete must return an object (profile) or null on miss; got {v}");
}

#[tokio::test]
async fn get_athletes_returns_array_in_request_order() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getAthletes",
        vec![json!([ATHLETE_ID, OTHER_ID])]).await;
    let arr = v.as_array().expect("getAthletes must return an array");
    assert_eq!(arr.len(), 2, "getAthletes must return one entry per id; got {v}");
}

#[tokio::test]
async fn get_athlete_data_returns_v1_record() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getAthleteData", vec![json!(ATHLETE_ID)]).await;
    let obj = v.as_object().expect("getAthleteData must return an object");
    for key in ["athleteId", "stats", "lap", "lapCount"] {
        assert!(obj.contains_key(key),
            "getAthleteData missing key `{key}`: {v}");
    }
}

#[tokio::test]
async fn get_athletes_data_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getAthletesData", vec![]).await;
    let arr = v.as_array().expect("getAthletesData must return an array");
    assert!(!arr.is_empty(),
        "getAthletesData must include seeded athletes; got {v}");
}

#[tokio::test]
async fn get_athlete_laps_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getAthleteLaps", vec![json!(ATHLETE_ID)]).await;
    assert!(v.is_array(),
        "getAthleteLaps must return an array; got {v}");
}

#[tokio::test]
async fn get_athlete_segments_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getAthleteSegments", vec![json!(ATHLETE_ID)]).await;
    assert!(v.is_array(),
        "getAthleteSegments must return an array; got {v}");
}

#[tokio::test]
async fn get_athlete_streams_returns_stream_object() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getAthleteStreams", vec![json!(ATHLETE_ID)]).await;
    let obj = v.as_object().expect("getAthleteStreams must return an object");
    for key in ["time", "power", "speed", "hr", "cadence", "draft", "distance", "altitude"] {
        assert!(obj.contains_key(key),
            "getAthleteStreams missing key `{key}`: {v}");
    }
}

#[tokio::test]
async fn get_player_state_returns_state_or_null() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getPlayerState", vec![json!(ATHLETE_ID)]).await;
    assert!(v.is_object() || v.is_null(),
        "getPlayerState must return a state object or null; got {v}");
}

#[tokio::test]
async fn get_power_zones_returns_array_of_zones() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getPowerZones", vec![json!(250.0)]).await;
    let arr = v.as_array().expect("getPowerZones must return an array");
    assert!(arr.len() >= 6,
        "getPowerZones must return ≥ 6 Coggan-style zones; got {v}");
    let first = arr[0].as_object()
        .expect("each power zone must be an object");
    assert!(first.contains_key("zone") || first.contains_key("label"),
        "power zone needs a `zone`/`label` field; got {v}");
}

#[tokio::test]
async fn get_power_profile_returns_object() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getPowerProfile", vec![json!(ATHLETE_ID)]).await;
    assert!(v.is_object() || v.is_array() || v.is_null(),
        "getPowerProfile must return an object/array/null; got {v}");
}

// ---------------------------------------------------------------------------
// Nearby / Groups
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_nearby_data_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getNearbyData", vec![]).await;
    assert!(v.is_array(),
        "getNearbyData must return a sorted array; got {v}");
}

#[tokio::test]
async fn get_groups_data_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getGroupsData", vec![]).await;
    assert!(v.is_array(),
        "getGroupsData must return an array; got {v}");
}

// ---------------------------------------------------------------------------
// Event getters
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_cached_events_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getCachedEvents", vec![]).await;
    assert!(v.is_array(),
        "getCachedEvents must return an array; got {v}");
}

#[tokio::test]
async fn get_cached_event_returns_object_or_null() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getCachedEvent", vec![json!(SUBGROUP_ID)]).await;
    assert!(v.is_object() || v.is_null(),
        "getCachedEvent must return an object or null; got {v}");
}

#[tokio::test]
async fn get_event_subgroup_returns_seeded_entry() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getEventSubgroup", vec![json!(SUBGROUP_ID)]).await;
    let obj = v.as_object()
        .expect("getEventSubgroup must return the seeded subgroup as an object");
    assert_eq!(obj.get("id").and_then(|v| v.as_u64()), Some(SUBGROUP_ID as u64),
        "getEventSubgroup must echo the requested id; got {v}");
}

#[tokio::test]
async fn get_event_returns_object_or_null() {
    let rpc = build_registry(seeded_state());
    // No live REST fetcher in this test setup; cache miss → null is acceptable.
    let v = must_dispatch(&rpc, "getEvent", vec![json!(99u64)]).await;
    assert!(v.is_object() || v.is_null(),
        "getEvent must return an object or null; got {v}");
}

#[tokio::test]
async fn get_event_subgroup_entrants_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getEventSubgroupEntrants", vec![json!(SUBGROUP_ID)]).await;
    assert!(v.is_array(),
        "getEventSubgroupEntrants must return an array; got {v}");
}

#[tokio::test]
async fn get_event_subgroup_results_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getEventSubgroupResults", vec![json!(SUBGROUP_ID)]).await;
    assert!(v.is_array(),
        "getEventSubgroupResults must return an array; got {v}");
}

// ---------------------------------------------------------------------------
// Segment leaderboards
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_segment_results_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getSegmentResults", vec![json!(1u64)]).await;
    assert!(v.is_array(),
        "getSegmentResults must return an array (possibly empty); got {v}");
}

// ---------------------------------------------------------------------------
// Chat / game state
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_chat_history_returns_array() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getChatHistory", vec![]).await;
    assert!(v.is_array(),
        "getChatHistory must return an array (possibly empty); got {v}");
}

#[tokio::test]
async fn get_game_state_returns_seeded_snapshot() {
    let rpc = build_registry(seeded_state());
    let v = must_dispatch(&rpc, "getGameState", vec![]).await;
    let obj = v.as_object()
        .expect("getGameState must return the seeded snapshot");
    assert_eq!(obj.get("playerId").and_then(|v| v.as_u64()), Some(ATHLETE_ID as u64),
        "getGameState must echo the seeded snapshot; got {v}");
}

// ---------------------------------------------------------------------------
// Geometry getters (data may be stubbed until Steps 17/18 land — null/empty
// returns are acceptable for now; the registration is the contract).
// ---------------------------------------------------------------------------

#[tokio::test]
async fn geometry_getters_are_registered() {
    let rpc = build_registry(seeded_state());
    for name in ["getWorldMetas", "getCourseId", "getRoad", "getRoute", "getSegment"] {
        let res = rpc.dispatch(name, vec![json!(1u64)]).await;
        assert!(res.is_some(),
            "geometry getter `{name}` must be registered (returned None from dispatch)");
        // A handler that finds nothing may legitimately return null/empty;
        // a hard error indicates a wiring bug, not absent data.
        if let Some(Err(e)) = res {
            panic!("geometry getter `{name}` returned error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Settings / connection info
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_set_setting_round_trip() {
    let rpc = build_registry(seeded_state());

    // setSetting (write to in-memory settings store, NOT a Zwift write RPC)
    let stored = must_dispatch(&rpc, "setSetting",
        vec![json!("uiMode"), json!("dark")]).await;
    // Sauce returns the new value (or null); accept either.
    assert!(stored == json!("dark") || stored.is_null(),
        "setSetting return must be the value or null; got {stored}");

    let got = must_dispatch(&rpc, "getSetting", vec![json!("uiMode")]).await;
    assert!(got == json!("dark") || got.is_null(),
        "getSetting must return the value just written or null; got {got}");
}

#[tokio::test]
async fn info_getters_are_registered() {
    let rpc = build_registry(seeded_state());
    for name in ["getDebugInfo", "getWebServerURL", "getZwiftLoginInfo", "getZwiftConnectionInfo"] {
        let res = rpc.dispatch(name, vec![]).await;
        assert!(res.is_some(),
            "info getter `{name}` must be registered (dispatch returned None)");
        if let Some(Err(e)) = res {
            panic!("info getter `{name}` returned error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Write-RPC exclusion (QA1)
// ---------------------------------------------------------------------------

/// Sauce4Zwift exposes write RPCs that mutate Zwift-side state
/// (`setFollowing`, `giveRideon`, `updateAthlete`, etc.).  Ranchero is
/// read-only by decision QA1 in STEP-20.  These names MUST NOT be
/// registered so a misbehaving widget cannot mutate Zwift state through
/// ranchero.
#[tokio::test]
async fn write_rpcs_not_registered() {
    let rpc = build_registry(seeded_state());
    let write_names = [
        "setFollowing",
        "giveRideon",
        "updateAthlete",
        "saveAthleteFTP",
        "addAthleteToMarked",
        "removeAthleteFromMarked",
        "unfollow",
        "follow",
    ];
    for name in write_names {
        let res = rpc.dispatch(name, vec![]).await;
        assert!(res.is_none(),
            "write RPC `{name}` must NOT be registered (QA1); dispatch returned {res:?}");
    }
}
