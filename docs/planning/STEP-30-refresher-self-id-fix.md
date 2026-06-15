# Step 30 — State refresher must not poll the monitor account (D6)

Source: `review.md` finding **D6** and the standalone bug report
[`refresher-self-id-bug.md`](refresher-self-id-bug.md) (2026-05-26).
Order-of-work item 9 (dedicated). Small, self-contained, and already has a
failing test in the tree.

## Goal

The state refresher polls the **watched** athlete, never the monitor account.
The existing failing test
(`tests/relay_runtime.rs::state_refresh_polls_get_player_state_on_self_tuning_interval`)
turns green.

## Background the implementer needs

- `start` resolves `athlete_id` from the monitor account's login
  (`src/daemon/relay.rs:1676`, `auth.athlete_id().await`) and passes it to
  `run_state_refresher` as `self_id` (`relay.rs:2311`).
- Inside the refresher, `self_id != watched_id` is true (monitor id ≠ watched
  id), so the `if self_id != watched_id` branch (`relay.rs:604-617`) polls
  `get_player_state(self_id)` — the monitor, which is not a rider and has no
  player state (probably a 404 against real Zwift).
- This contradicts the project rule that "self" is the watched athlete, never
  the monitor (`self_athlete_id = cfg.watched_athlete_id`).
- The failing test asserts every poll targets the watched id (54321), not the
  monitor's (12345). The stub returns `athlete_id() = 12345`; config sets
  `watched_athlete_id = Some(54321)`.

## Fix shape (from the bug report)

Two options; the report prefers option 2 for intent, option 1 for smallest
diff:

1. Pass the watched id where `athlete_id` is passed at the call site
   (`relay.rs:2311`), so `self_id == watched_id` in the single-account
   configuration and the self-poll branch is correctly skipped.
2. Replace `auth.athlete_id().await` at `relay.rs:1676` with the resolved
   `cfg.watched_athlete_id` for refresher use, expressing the intent that
   "self" is the watched athlete.

Either is acceptable. Note for the future two-account case (a distinct `main`
rider account watching a different athlete on the relay): `self_id` should
then be the **main rider's** id, not the monitor's — but that account is not
represented in the current `RelayRuntime` start path, so do not build for it
now; just leave the seam clear.

## Tests first

The red test already exists — confirm it fails, then make it pass.

- [ ] **30.1-T** Confirm
      `tests/relay_runtime.rs::state_refresh_polls_get_player_state_on_self_tuning_interval`
      fails for the documented reason (poll targets 12345). No new test
      needed to start; this is the red.
- [ ] **30.1-I** Apply the fix (option 1 or 2). Every refresher poll now
      targets the watched id; the test passes.
- [ ] **30.2-T** Add a guard test: the refresher never calls
      `get_player_state` with the monitor's id under the single-account
      configuration (assert the monitor id appears in zero polls).
- [ ] **30.2-I** Covered by 30.1-I; lock with the test so a future regression
      is caught.

## Acceptance criteria

- The named test passes; the new guard test passes.
- No poll targets the monitor account.
- Fast suite green.

## Dependencies

- None. Can be done at any point; independent of the larger steps.

## Deferred

- Representing a distinct `main` rider account in the start path (the real
  two-account case) is out of scope; leave the comment seam noted above.
