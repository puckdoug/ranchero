# Step 29 — Motion-based idle suspension (D5 / V2)

Source: `review.md` findings **D5** and **V2** (decided by Q2).
Order-of-work item 9 (first of three). Connects the already-written
`IdleFSM` so UDP suspension follows rider motion as the spec describes.

## Goal

The daemon suspends its UDP channel when the watched rider is genuinely
idle — speed, cadence, and power all zero for about 60 seconds — and resumes
the moment any of them is non-zero, matching the spec and sauce.

## Decision in force

**Q2 (2026-06-12): follow the spec as sauce does.** Connect `IdleFSM`;
suspension is motion-based. Where sauce also runs a time-based polling
slowdown, match sauce rather than inventing a ranchero-specific combination.

## Background the implementer needs

- `IdleFSM` is fully implemented (`src/daemon/relay.rs:1056-1151`) with
  `observe_motion(speed, cadence, power)` and a 60 s window
  (`IDLE_WINDOW`), but is called **only** from tests.
- Production suspension today is time-based: no fresh self-state for 15 s
  (`relay.rs:650-657`), set by the state-refresher and cleared by the
  recv-loop (`relay.rs:3729`).
- Suspending the UDP channel is the existing mechanism (`suspended` flag +
  `resume_udp`); this step changes only *what decides* suspension, not the
  suspend/resume plumbing.
- Spec §4.13 / §7.7 is the contract; sauce's `_refreshStates`
  (`zwift.mjs:1977-1982`, resume at `:2237`) is the reference.

## Tests first

- [ ] **29.1-T** Feed the watched athlete's states into the production recv
      path: with all-zero motion for 60 s, the UDP channel suspends; the
      first non-zero field resumes it. Assert on the `suspended` flag /
      suspend+resume trace events.
- [ ] **29.1-I** Call `IdleFSM::observe_motion` from the recv path for the
      watched athlete's states; drive the existing suspend/resume on its
      output.
- [ ] **29.2-T** A rider stopped but still emitting zero-value states (the
      case the time-based rule missed) now suspends after 60 s.
- [ ] **29.2-I** Covered by 29.1-I; lock with the test.
- [ ] **29.3-T** A brief telemetry gap mid-ride (states stop then resume
      while motion is non-zero) does **not** suspend — no suspend/resume
      churn. (This is the regression the old time-based rule caused.)
- [ ] **29.3-I** Ensure suspension is driven by motion, not by data absence;
      reconcile or remove the 15 s no-state trigger per Q2 (match sauce — if
      sauce keeps a data-absence behavior, keep it aligned; otherwise drop
      it).
- [ ] **29.4-T** No dead code remains: `IdleFSM` is reachable from production
      (a `cargo` build with `-W dead_code` does not flag it).
- [ ] **29.4-I** Remove any now-unused time-based remnant or the
      `#[allow(dead_code)]` on `IdleFSM`.

## Acceptance criteria

- Suspension is decided by rider motion (all-zero for ~60 s), not by data
  absence; resume on any non-zero field.
- No suspend/resume churn during brief telemetry gaps while riding.
- `IdleFSM` is production-reachable; no stale `#[allow(dead_code)]`.
- Fast suite green.

## Dependencies

- None hard. Independent of the other order-9 items (Steps 30, 31).

## Deferred

- None.
