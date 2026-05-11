# Step 19 — Compatibility test battery (stub)

## Goal

Pull the whole pipeline together and verify spec §7.11:

1. **AES-GCM interop.** Fixed `(key, iv, aad, plaintext)` → byte-identical
   ciphertext+tag between JS and Rust (already exercised in STEP 08; here
   it is pinned in a reproducible top-level test).
2. **Header codec round-trip.** Fuzz all 8 flag combinations (same note).
3. **Login.** Against a near-live environment (or the captured replay),
   the Rust monitor must produce a `ServerToClient` on TCP and receive
   one UDP packet within 5 s of `establish()`.
4. **Metric parity.** Feed a recorded `ServerToClient` trace through
   both engines; compare published metrics per tick. ≤ 1e-6 drift on
   sums, exact match on counts and zones.
5. **WebSocket parity.** Point ranchero's vendored widget pages (the
   `pages/` tree copied in at the time of the port) at the Rust
   daemon; widgets render correctly (manual verification plus
   golden-snapshot). The golden snapshots are captured once against
   the original JS server during the port and then frozen in
   ranchero's repository; the test must not require a live
   sauce4zwift checkout.

## Tests-first outline

- Add a `compat/` fixtures tree:
  `compat/fixtures/server_to_client/{name}.bin` captured streams.
- `compat/expected/{name}.metrics.json` with the JS reference outputs.
- `tests/compat_metric_parity.rs` iterates every fixture.

To be fully elaborated when work on this step begins.

## Inputs deferred from STEP 14

STEP 14 made two implementation choices in the `DataCollector`
periodized-clone fan-out that trade memory and per-push CPU for
implementation simplicity. Neither is a correctness concern; both
were deferred to this step because measurement under realistic load
is the right way to decide whether to revisit them.

- **Peak-snapshot memory footprint.** Each periodized peak stores a
  full clone of the rolling window at the moment of the peak
  (matches JS `stats.mjs:185-189`). Worst-case sizing: a 3600 s
  window at 1 Hz is approximately 3600 `f64` pairs ≈ 58 KB per
  snapshot. With 100 athletes × 5 signals × 6 periods that is
  approximately 174 MB just in peak snapshots. The published
  metrics only need `{snap_value, snap_time}` from each snapshot
  (spec §5.4); the cloned roll is read by analysis-page features
  ranchero v1 does not implement. STEP 19 measures the actual
  footprint under a recorded multi-rider trace and decides whether
  to (a) keep the full clone (matches JS, simplest to reason
  about), (b) downgrade to a `(snap_value, snap_time)`-only
  snapshot, or (c) keep the full clone behind a feature flag for
  future analysis tooling. Capture the decision in this step's
  as-built notes; if (b) or (c) is chosen, the change lives in
  `zwift-stats::collector` (`PeakSnapshot` / `NpPeakSnapshot`) and
  is mechanically small.
- **Independent-clone fan-out CPU cost.** STEP 13 chose to copy
  `_times` / `_values` on `RollingAverage::clone` rather than
  share them via `Arc<Vec<f64>>` with copy-on-write semantics. As
  a result, STEP 14's `DataCollector` pushes each flushed sample
  into the primary roll plus every periodized clone independently
  — gap-fill runs N+1 times per push (one primary + N periodized).
  For power that is 7 runs per push; for the other signals 5 runs.
  STEP 14 expected this to be negligible at 1 Hz but explicitly
  deferred confirmation: STEP 19 measures the per-push wall clock
  against a recorded trace, and if the per-tick budget across all
  active athletes is uncomfortably tight, switches to the
  `Arc<Vec<f64>>` shared-backing-store design (matches JS clone
  semantics more closely). Decision rule: if the parity battery's
  wall clock exceeds 10× the JS reference's wall clock on the same
  fixture, revisit. Otherwise the duplication stays.

These two items are measurement-driven, not correctness-driven; the
acceptance criterion is "measured, decided, recorded", not "fixed".

## Inputs deferred from STEP 14 (parity fixtures to fold in)

STEP 14 ships `tests/stream_parity.rs` and the
`athlete_stream.json` fixture inside `crates/zwift-stats/`. That
fixture exercises one captured ride end-to-end through
`DataBucket::ingest_*`. STEP 19's broader compatibility battery
should:

- Promote the STEP 14 fixture (or a regenerated equivalent) into
  the `compat/fixtures/server_to_client/{name}.bin` tree so the
  same trace exercises the pipeline from raw bytes (UDP / TCP
  capture) through proto decode, stats ingest, and published-metric
  formatting — not just the stats-engine slice STEP 14 tests in
  isolation.
- Add the per-period peak and NP-peak values from
  `tests/fixtures/athlete_stream.json` to the
  `compat/expected/{name}.metrics.json` oracle so the broader
  parity test catches regressions in the orchestration layer that
  the unit test in STEP 14 would also flag.
- Confirm tolerance: STEP 14 uses ≤ 1e-6; STEP 19 should retain
  the same `f64` tolerance for sums and exact-match for counts /
  zones / peak times (matches the existing description above).
