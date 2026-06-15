# Step 31 — Settings persistence + cadence clamp (G9 + D1)

Source: `review.md` findings **G9** and **D1**. Order-of-work item 9 (last of
three). Two small, independent fixes grouped because both are short.

## Goal

1. Daemon settings survive a restart (the KV store is no longer inert).
2. Lag-burst cadence values are clamped, so they cannot poison the cadence
   rolling windows.

## Part A — Settings persistence (G9)

### Background

- `Stores::open` runs and the handles reach `WebState` (`with_stores`), but
  the KV handle is discarded — `kv: _` at `src/web/state.rs:223`.
- `getSetting`/`setSetting` use an in-memory `HashMap`
  (`src/web/state.rs:108`), so settings are lost on restart.
- The KV store is `store(id TEXT PRIMARY KEY, data ...)`, WAL mode, tested in
  isolation.

### Tests first

- [ ] **31.A1-T** `setSetting` writes through to the KV store and a fresh
      `WebState` reading the same store sees the value (simulates restart).
- [ ] **31.A1-I** Keep the KV handle in `WebState` (stop discarding it); make
      `setSetting` write through and `getSetting` read-through. Do the SQLite
      work off the async runtime (K2).
- [ ] **31.A2-T** Settings load on boot: a store pre-populated with a value
      is visible from `getSetting` immediately after construction.
- [ ] **31.A2-I** Read existing settings from the KV store at startup into
      the in-memory map (or read-through on each get).
- [ ] **31.A3-T** `setSetting` emits `setting-change` on the `app` source; a
      subscriber receives it.
- [ ] **31.A3-I** Wire the `app` source producer to emit `setting-change`
      (depends on the Step 21 event path being in place).

## Part B — Cadence clamp (D1)

### Background

- Spec §4.11 / §7.12: cadence above `240 × 1e6 / 60` (4,000,000 µrev/s) is a
  Zwift lag-burst artifact and must be clamped or dropped (sauce treats it as
  1).
- No clamp exists; `src/web/proto_to_stats.rs` converts `cadence_u_hz` to RPM
  unguarded, and no such constant exists in the workspace.

### Tests first

- [ ] **31.B1-T** A `PlayerState` with `cadence_u_hz` above the limit
      produces a clamped (sauce: treated-as-1) cadence, not a garbage RPM;
      the cadence rolling window is not poisoned.
- [ ] **31.B1-I** Clamp at the conversion boundary (proto-view or
      `proto_to_stats`): values above `240 × 1e6 / 60` follow sauce's
      `cadenceMax` rule (`zwift.mjs:57`).
- [ ] **31.B2-T** A normal cadence value is unaffected (regression guard).
- [ ] **31.B2-I** Covered by 31.B1-I; lock with the test.

## Acceptance criteria

- Settings set via `setSetting` survive a daemon restart; `setting-change`
  fires on the `app` source.
- Over-limit cadence is clamped per sauce; normal values pass through.
- Fast suite green.

## Dependencies

- Part A's `setting-change` emission depends on **Step 21**. Part B is fully
  independent.

## Deferred

- None.
