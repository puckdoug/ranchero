# Pre-existing bug: state refresher passes monitor id as "self"

**Discovered:** 2026-05-26, during `cargo test --workspace` after Step 15.
**Failing test:** `tests/relay_runtime.rs::state_refresh_polls_get_player_state_on_self_tuning_interval`.
**Status as of discovery:** Step 11 was marked `[x][x]` done in
`STEP-20-additional-considerations.md`, but the test catches a real
production-code bug that the Step 11 implementation did not actually fix.

## Symptom

The test asserts that every `auth.get_player_state(id)` call made by
`run_state_refresher` targets the *watched* athlete id (`54321`), per
sauce's `_refreshStates` parity (`zwift.mjs:1998`, "Batch C §Ca"). The
test fails on the first poll: the refresher polls id `12345` (the
monitor account) instead of `54321`.

```
assertion `left == right` failed: Batch C §Ca: refresher must poll the
watched athlete's ID (54321), NOT the monitor's (12345); saw 12345
  left: 12345
 right: 54321
```

## Root cause

`src/daemon/relay.rs:1699`:

```rust
let athlete_id = auth.athlete_id().await.map_err(RelayRuntimeError::Auth)?;
```

`auth` here is the monitor account's `ZwiftAuth`, so `athlete_id` is the
monitor's id (12345). At line 2334 this value is then handed to
`run_state_refresher` as its `self_id` parameter:

```rust
run_state_refresher(auth_for_refresher, athlete_id, watched_id_i64, inner_for_refresher).await;
```

Inside the refresher, `self_id != watched_id` is true (12345 ≠ 54321), so
the refresher polls `auth.get_player_state(self_id)` — the monitor's id.

This contradicts the project memory rule [["self" is the watched
athlete]]: `self_athlete_id = cfg.watched_athlete_id`, never the monitor
account. The monitor is not a rider and has no `PlayerState` to fetch;
polling the monitor is both wrong by the parity contract and probably
returns `404` against real Zwift.

## Proposed fix

`run_state_refresher`'s `self_id` argument should be
`cfg.watched_athlete_id` (the watched athlete), not
`auth.athlete_id().await` (the monitor). Two reasonable shapes:

1. **Pass watched id twice.** Change the call site at
   `src/daemon/relay.rs:2334` from `athlete_id` to `watched_id_i64`.
   Then `self_id == watched_id` in the single-account configuration,
   and the `if self_id != watched_id { ... }` branch in the refresher is
   correctly skipped. In a future two-account configuration where the
   `main` rider account is distinct from the `watched` athlete (e.g.,
   watching a different rider on the relay), the `self_id` should be
   the main rider's id, not the monitor's — but that account is not
   represented in the current `RelayRuntime` start path.

2. **Plumb `cfg.watched_athlete_id` directly.** Replace
   `auth.athlete_id().await` at line 1699 with the resolved
   `cfg.watched_athlete_id` value, eliminating the intermediate
   `athlete_id` variable for refresher use. Keeps the variable for any
   path that genuinely needs the monitor's id (none exists today).

Option (1) is the smallest diff. Option (2) better expresses the intent.

## Out of scope

This is a Step 11 regression (or, more likely, a Step 11 item that was
marked done before the test was written / before the test was being
run). It is not caused by Step 15 and should not be fixed as part of
Step 15. Logging it here so the next pass through `STEP-20` plan items
can pick it up.
