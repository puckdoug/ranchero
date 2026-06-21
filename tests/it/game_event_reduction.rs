// SPDX-License-Identifier: AGPL-3.0-only
//
// Step 19 — `GameEvent::PlayerState` cleanup (QE3).
//
// The `PlayerState` variant on the relay-to-web broadcast carries eleven
// scalar fields today, but only `athlete_id` is ever read downstream: the
// stats fanout looks the athlete up in the registry, and the full proto
// travels separately on the dedicated `player_states` stream. Step 19
// reduces the variant to `{ athlete_id }`.
//
// Red state: the variant still has the ten vestigial scalars, so the
// struct-pattern below fails to compile.
//
// See docs/planning/STEP-20-additional-considerations.md, QE3.

use ranchero::daemon::relay::GameEvent;

#[test]
fn game_event_player_state_is_athlete_id_only() {
    // Constructing the variant with only `athlete_id` proves no other
    // fields remain on it.
    let ev = GameEvent::PlayerState { athlete_id: 42 };

    // Exhaustive struct-pattern: if any other field is present this
    // pattern will fail to compile with "missing field" errors.
    let GameEvent::PlayerState { athlete_id } = ev else {
        panic!("expected PlayerState; got a different variant");
    };
    assert_eq!(athlete_id, 42);
}
