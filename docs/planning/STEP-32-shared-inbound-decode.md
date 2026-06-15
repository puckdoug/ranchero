# Step 32 — Extract the shared TCP/UDP inbound-decode path (K1)

Source: `review.md` finding **K1** (parked as STEP-20 item 20.2; its
"revisit when" condition has been met). Order-of-work item 10. A refactor,
not a feature — but do it before the next change to the receive loops.

## Goal

The inbound-decode logic that the TCP and UDP receive loops now share lives
in one place, so a future change cannot be applied to one transport and
forgotten on the other.

## Background the implementer needs

- The two recv-loop arms carry near-identical logic: NINJA-powerup drop,
  watched-position capture, `decode_world_update`, pool updates, and
  `GameEvent`/proto emission. TCP arm around `src/daemon/relay.rs:3476+`;
  UDP arm around `relay.rs:3697+`.
- They are genuinely live (both run in production), so divergence would
  produce transport-dependent behavior — the hardest kind to notice.
- The only intended differences between the arms are transport mechanics
  (framing, the seqno/header handling), not the post-decode handling of a
  `ServerToClient`.

## Tests first

This is a behavior-preserving refactor, so the existing recv-loop tests are
the safety net. Add a focused test first that pins the shared behavior, then
extract.

- [ ] **32.1-T** A single test feeds an identical `ServerToClient` (with a
      player state, a NINJA state to drop, and a chat `WorldUpdate`) through
      both the TCP and UDP inbound paths and asserts identical observable
      output (same emitted events, same dropped state, same pool update).
      This pins parity before the extraction.
- [ ] **32.1-I** No production change yet; just the test, which must pass
      against the current duplicated code.
- [ ] **32.2-T** After extraction, the same test still passes, plus the full
      existing recv-loop test set (`tests/relay_runtime.rs` and inline relay
      tests) stays green.
- [ ] **32.2-I** Extract the post-decode handling into one shared function
      (taking the decoded `ServerToClient` plus the per-transport context it
      needs) and call it from both arms. Keep the transport-specific framing
      in each arm.

## Acceptance criteria

- One shared function handles inbound `ServerToClient` processing; both arms
  call it.
- No behavior change: every existing recv-loop test plus the new parity test
  stays green.
- The two arms differ only in transport mechanics.

## Dependencies

- Best done **after** the steps that touch the recv loops (Steps 21, 28, 29)
  so the extraction captures their additions rather than racing them. If a
  recv-loop change lands first, extend the shared function rather than
  re-duplicating.

## Deferred

- None.
