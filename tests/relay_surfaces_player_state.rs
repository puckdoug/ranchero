// SPDX-License-Identifier: AGPL-3.0-only
//! 17.37-T — The relay must surface the full `PlayerState` proto to downstream
//! consumers, not only the five scalar fields that the current
//! `GameEvent::PlayerState` variant carries.
//!
//! `route_player_state` (and the bridge task from 17.38-I) need to read
//! `proto.world` (course), `proto.sport`, `proto.distance`, `proto.z`
//! (altitude), `proto.draft`, and `proto.heartrate` to fully populate the
//! athlete registry.  None of those fields are present on
//! `GameEvent::PlayerState { athlete_id, power_w, cadence_u_hz, speed_mm_h,
//! world_time_ms }`.
//!
//! **This test fails to compile** until 17.37-I enriches `GameEvent::PlayerState`
//! to carry the full `zwift_proto::PlayerState` (or adds a dedicated proto
//! broadcast alongside the scalar event).  The compile error is the failing
//! test; no runtime assertion is needed.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.37-T.

use ranchero::daemon::relay::GameEvent;

/// Assert at compile time that `GameEvent::PlayerState` carries the proto
/// fields that `route_player_state` needs.
///
/// This function is never called at runtime.  It exists only as a
/// compile-time structural check.  Fails to compile until 17.37-I adds the
/// missing fields to `GameEvent::PlayerState`.
fn _assert_player_state_variant_carries_full_proto(event: GameEvent) {
    // Destructure with every field that route_player_state reads from the proto.
    // error[E0026] is emitted for each field that the variant does not yet have.
    let GameEvent::PlayerState {
        athlete_id: _,
        // These five fields exist today:
        power_w: _,
        cadence_u_hz: _,
        speed_mm_h: _,
        world_time_ms: _,
        // These fields do not exist until 17.37-I:
        world: _,      // proto.world    → course_id for registry.upsert
        sport: _,      // proto.sport    → sport for registry.upsert
        distance: _,   // proto.distance → distance tracker
        z: _,          // proto.z        → raw altitude before world-meta adjust
        draft: _,      // proto.draft    → ingest_draft
        heartrate: _,  // proto.heartrate → ingest_hr
    } = event else {
        unreachable!()
    };
}

// Provide one `#[test]` entry point so cargo counts this file in the test
// inventory.  The function never executes while the compile error above
// persists; once 17.37-I lands and the compile error is resolved, this test
// will pass trivially.
#[test]
fn relay_player_state_event_carries_full_proto() {
    // All the work is done at compile time by the helper above.
    // No runtime assertion is needed once the compile error is resolved.
}
