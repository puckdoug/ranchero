# Missing fixture: zwift-proto server_to_client_basic.bin — RESOLVED

## Resolution (2026-05-23)

`crates/zwift-proto/tests/fixtures/server_to_client_basic.bin` is now generated
by `cargo run --bin sanitize_capture` as part of the STEP-19 sanitiser workflow.

The fixture is **synthetic** rather than extracted from a live capture. The
recorded ride in `tmp/output.cap` was a monitor-only session: the server
delivered only configuration and world-time frames (no `PlayerState` records in
`ServerToClient.states` or `ServerToClient.player_states`). The sanitiser detects
this case and falls back to a minimal synthetic `ServerToClient` with one
`PlayerState` entry (id=10001, power=200 W, heartrate=140 bpm, speed=10 km/h,
distance=5 000 m, cadence=80 rpm).

The synthetic fixture is sufficient to confirm the full encode/decode path for
`PlayerState` fields. The `fixture_basic_packet_decodes` test is no longer marked
`#[ignore]` and runs in the default `cargo test` pass.

If a real wire capture with `PlayerState` data becomes available:

1. Run `cargo run -- start --capture /tmp/zwift_with_riders.cap` in a session
   where other riders are in the same world.
2. Run `cargo run --bin sanitize_capture -- /tmp/zwift_with_riders.cap <output> <basic>`.
   The sanitiser will prefer a real frame over the synthetic fallback.
3. Commit the new `server_to_client_basic.bin`.

---

## Original problem (historical)

`crates/zwift-proto/tests/server_to_client.rs::fixture_basic_packet_decodes` failed
when run with `cargo test -- --include-ignored` because the fixture file was absent.
The test was originally marked `#[ignore = "requires tests/fixtures/server_to_client_basic.bin
(real Zwift wire capture)"]`.

An early sanitiser run produced a 1053-byte fixture (player_id=10001, UdpConfigVOD
data, no PlayerState). That file caused the test to fail when the ignore marker was
removed because the assertion `!msg.states.is_empty() || !msg.player_states.is_empty()`
was not satisfied. The sanitiser was updated to detect this case and synthesise a
valid fixture instead.
