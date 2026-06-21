// SPDX-License-Identifier: AGPL-3.0-only
//! 17.37-I — The relay must surface the full `PlayerState` proto to the
//! relay-to-web bridge, not only the scalar fields that the
//! `GameEvent::PlayerState` variant carries.
//!
//! `route_player_state` reads `proto.world` (course), `proto.sport`,
//! `proto.distance`, `proto.z` (altitude), `proto.draft`, `proto.heartrate`
//! — and, through `ProtoView`, also `proto.aux3` (road id), `proto.road_time`,
//! `proto.f19` (direction), `proto.group_id`, and `proto.time`. None of those
//! last five are present on the scalar `GameEvent::PlayerState` variant, so the
//! relay exposes the whole proto on a dedicated broadcast stream
//! (`RelayRuntime::player_states`) for the bridge to consume.
//!
//! This file pins that **public API contract** at compile time: the relay must
//! expose `player_states()` returning a `broadcast::Receiver` of the full
//! `zwift_proto::PlayerState`. The end-to-end *behaviour* (that the recv-loop
//! actually publishes every field with full fidelity) is proven by the unit
//! test `player_state_proto_surfaced_on_inbound_with_full_fidelity` inside
//! `src/daemon/relay.rs`, where the `#[cfg(test)]` injection helpers
//! (`start_with_deps`, `inject_event`) are available — they are not reachable
//! from an integration test.
//!
//! See docs/plans/STEP-17-web-server.md item 17.37-I and
//! docs/plans/STEP-17-relay-web-bridge-design.md Step A/B.

use ranchero::daemon::relay::RelayRuntime;
use tokio::sync::broadcast;

/// Compile-time assertion that `RelayRuntime::player_states` yields a receiver
/// of the *full* proto. This function is never called; it exists only so the
/// type checker verifies the contract. If `player_states` were removed or
/// retyped to a scalar projection, this file would stop compiling.
#[allow(dead_code)]
fn _assert_player_states_stream_carries_full_proto(runtime: &RelayRuntime) {
    let rx: broadcast::Receiver<zwift_proto::PlayerState> = runtime.player_states();
    // Reference a field that only the full proto carries (and that the scalar
    // `GameEvent::PlayerState` variant omits) to prove the stream is the whole
    // proto, not a reduced view.
    let _carries_road_id = |proto: zwift_proto::PlayerState| proto.aux3;
    let _ = rx;
}

/// One `#[test]` entry point so cargo counts this file in the inventory. The
/// real verification is the compile-time check above plus the relay unit test.
#[test]
fn relay_exposes_full_proto_stream() {
    // Compile-time work is done by the helper above; nothing to assert here.
}
