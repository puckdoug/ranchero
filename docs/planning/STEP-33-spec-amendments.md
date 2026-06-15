# Step 33 — Spec amendments for decided deviations (V1 + V3)

Source: `review.md` findings **V1** and **V3** (decided by Q1 and Q3).
Order-of-work item 11. Documentation only — no code, and therefore no TDD
checklist. The code is correct as it stands; the spec is the document that
needs to change so it stops reading as a gap.

## Goal

`docs/ARCHITECTURE-AND-RUST-SPEC.md` describes what ranchero actually does in
two places where a deliberate decision diverged from the original sauce-derived
spec, so a future reader does not log these as bugs again.

## Amendment 1 — Keyring service name (V1, Q1)

- **Spec §7.10** currently says: use service
  `"Zwift Credentials - Sauce for Zwift"` so an existing Sauce install's
  credentials are picked up unchanged.
- **Decision (Q1, 2026-06-12): keep `"ranchero"`.** The separation is
  intentional (`src/credentials/mod.rs:6-18`).
- **Edit:** change §7.10 to describe the ranchero-owned keyring entry
  (service `"ranchero"`, account names for main and monitor). State plainly
  that credentials from an existing sauce4zwift install are **not** picked up,
  and that they are entered once through `ranchero configure`. Remove the
  "picked up unchanged" claim.

## Amendment 2 — State-refresher stale backoff (V3, Q3)

- **Spec §4.13** currently says: multiply the refresh interval by 1.02 on
  stale responses and 1.15 on errors.
- **Decision (Q3, 2026-06-12): keep ranchero's scheme.** The 1.15-on-error
  rule matches; staleness uses an adaptive expand/contract instead of 1.02
  (`src/daemon/relay.rs:658-660`).
- **Edit:** change the staleness description in §4.13 to: after 15 s with no
  fresh data, move the interval halfway toward 30 s on each poll; when fresh
  data resumes, move halfway back toward 3 s. Keep the 1.15-on-error rule as
  written. Note this is a deliberate ranchero divergence from sauce's 1.02
  multiplier.

## Acceptance criteria

- §7.10 and §4.13 read as descriptions of the current, intended behavior.
- Neither amendment changes code.
- A reader cross-checking the spec against the tree finds no contradiction at
  these two points.

## Dependencies

- None. Can be done at any time; independent of all other steps.

## Deferred

- Other spec touch-ups implied by the larger steps (for example, §5.6 once the
  `game-state` / `connection-state` split from Step 28 lands) are folded into
  those steps, not here. This step covers only the two pure-documentation
  decisions.
