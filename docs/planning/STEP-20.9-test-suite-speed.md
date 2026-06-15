# Step 20.9 — Restore a fast, contention-resilient test suite

**Runs before Step 21.** Source: the test-suite addendum in
[`review.md`](review.md). The default `cargo test` set has become slow enough
to be unusable during normal editing. This step makes it fast again and keeps
it that way, before the parity work (Steps 21–33) starts adding more tests.

## Goal

A clean default `cargo test` completes **well under one minute — target ~10–15
seconds** — and does **not** balloon when the editor's `rust-analyzer` is
active. No loss of test coverage: every test that runs today still runs.

"Complete test suite" here means the **default set** (`cargo test`). The
`--include-ignored` set contains ~50 deliberately slow real-socket / daemon
tests and is not expected to meet the one-minute bound; it stays excluded as
it is today.

## Root cause (measured 2026-06-12)

The same default set was measured at **363 s, then 95 s, then 30 s** on one
tree. The variance is the point — the wall time is dominated by **waiting on
the shared build lock and by the overhead of running each binary separately**,
not by the tests themselves:

- A built test binary run **directly** executes in **~0.05 s**; via `cargo
  test --test X` it takes **~1 s** (cargo's per-invocation overhead).
- A clean `cargo test` (nothing else touching cargo) is **30 s**, of which
  only ~4.4 s is CPU. The rest is cargo managing the binaries and waiting
  on the shared `target/debug/.cargo-lock`.
- The 95 s and 363 s runs happened while **other cargo processes ran
  concurrently** — during the review, measurement loops; in normal use,
  `rust-analyzer` (seen holding the build lock from the Zed editor). Every
  concurrent `cargo` blocks on that one lock.
- The slow-marker convention is healthy: **50 tests are already `#[ignore =
  "slow: …"]`**, and the binaries that first looked like 40–80 s offenders all
  run in ~1 s when run alone — they were slow because of the shared lock, not slow tests. So this is **not**
  a "mark more slow tests" problem like the 2026-05-23 regression was.

The structural cost is the **number of separate integration-test binaries**:
**~182 across the workspace** — 92 under the root `tests/`, plus 90 under the
crates (`zwift-stats` alone has 51, `zwift-store` 14, `zwift-relay` 11). Each
is its own executable that links the full dependency tree at build time and is
run separately.

## Approach

Three tracks. A is the structural improvement; B stops the editor from competing for the lock; C keeps it from regressing.

### A. Consolidate integration-test binaries (the main improvement)

Adopt the single-binary integration-test pattern (one test executable per
crate's `tests/` directory instead of one per file). Each former
`tests/foo.rs` becomes a module included from one `tests/main.rs`:

```
tests/
  main.rs        // mod foo; mod bar; mod common; ...
  it/
    foo.rs
    bar.rs
    common.rs    // shared helpers (test_config(), fixtures) — deduplicated
```

This cuts ~182 binaries toward ~8 (one per crate plus the root). One binary to
link and run instead of dozens cuts both incremental link time
(what you feel on rebuild) and the overhead of running the test phase. Test names and `cargo test
<path>` filters are preserved because the module paths carry them.

Trade-off to note: tests in one binary share a process, so an `abort()`/
segfault in one would stop the binary (the normal harness already isolates
panics across threads, so ordinary `assert!` failures are unaffected). For
this suite that trade is clearly worth it; it is the widely-used Rust pattern.

### B. Give `rust-analyzer` its own target directory

`rust-analyzer` runs background `cargo check`/builds that take the same
`target/debug/.cargo-lock` that `cargo test` also needs, so they block each other.
Point the editor at a separate target directory so the two never compete for the same lock:

- Add a checked-in editor setting (Zed `.zed/settings.json`:
  `"rust-analyzer": { "cargo": { "targetDir": "target/ra" } }`, or the VS Code
  equivalent `rust-analyzer.cargo.targetDir`), **or**
- document the global editor setting if a per-repo file is not wanted.

This alone stops the "unusable while editing" problem, even before
the binary consolidation is done.

## Tests first

This is infrastructure, so the "tests" are measurements and coverage-parity
checks. `-T` is the failing check; `-I` is the change that turns it green.

- [x] **20.9.1-T** Record the baseline as an explicit assertion in the plan
      PR description: clean `cargo test` wall time and the integration-binary
      count (`ls tests/*.rs crates/*/tests/*.rs | wc -l` ≈ 182). This is the
      red the step drives down.
      **Measured 2026-06-12:** binary count = **182** (92 root + 90 crates);
      wall time = **~69 minutes** (1:09:13) at 0% CPU — entirely spent
      waiting on the lock held by rust-analyzer; CPU was only
      ~15 s. Running the same binaries directly takes ~0.05 s each. One
      pre-existing failure: `relay_runtime::state_refresh_polls_…` (D6, fixed
      in Step 30). The `udp` test that also appeared failed was a timing
      failure caused by the shared lock being held — it passes cleanly when run alone.
- [x] **20.9.2-T** A coverage-parity snapshot: capture the full list of test
      names the default set runs today (`cargo test -- --list` across the
      workspace, normalized). Any consolidation must reproduce this list
      exactly.
      **Done 2026-06-15:** 1246 test names saved to
      `docs/planning/test-name-baseline.txt` (sorted, one name per line).
- [x] **20.9.2-I** Keep this list as the reference each consolidation step
      checks against. Saved at `docs/planning/test-name-baseline.txt`.
- [ ] **20.9.3-T** Consolidate the **root `tests/`**: move the 92 files under
      `tests/it/`, add `tests/main.rs` with one `mod` per file, lift the
      duplicated `test_config()`/fixtures into `tests/it/common.rs`. Assert the
      binary count for the root crate drops to 1 and the test-name list
      (20.9.2) is unchanged.
- [ ] **20.9.3-I** Do the move with `git mv` so history is preserved (this is
      a plan-file-only repo rule exception — source moves are fine). Fix
      `mod`/path references; run the suite green.
- [ ] **20.9.4-T** Consolidate **`zwift-stats/tests`** (51 files — the
      largest) the same way; assert one binary, same test names.
- [ ] **20.9.4-I** Implement; green.
- [ ] **20.9.5-T** Consolidate the remaining crates' `tests/` (`zwift-store`,
      `zwift-relay`, `zwift-api`, `zwift-routes`, `zwift-worlds`,
      `zwift-proto`); assert one binary each, same test names.
- [ ] **20.9.5-I** Implement; green.
- [ ] **20.9.6-T** Re-measure clean `cargo test`: assert wall time is **under
      60 s** (target ~10–15 s) on an otherwise-idle machine, with the
      20.9.2 test-name list intact.
- [ ] **20.9.6-I** If still over target, profile which consolidated binary
      dominates and split only that one, or move pure-logic tests into
      `#[cfg(test)]` unit modules inside their crate (no separate binary at
      all).
- [ ] **20.9.7-T** Editor-contention check: with `rust-analyzer` pointed at a
      separate target dir, a `cargo test` run started while the editor is
      indexing does **not** block on the build lock (wall time stays near the
      idle figure).
- [ ] **20.9.7-I** Add the checked-in editor target-dir setting (or document
      the global one).
- [ ] **20.9.8-T** Regression guard: a small check (script or
      `cargo test`-wrapper note in `README.md`) that flags if the
      integration-binary count climbs back up or the default set crosses a
      time threshold, so Steps 21–33 do not silently re-bloat it.
- [ ] **20.9.8-I** Add the guard and document it next to the existing
      slow-marker convention in `README.md`.

## Acceptance criteria

- Clean `cargo test` is under one minute (target ~10–15 s) and the test-name
  list is byte-for-byte the same as before consolidation.
- A `cargo test` started while `rust-analyzer` is active does not wait
  behind the editor's build lock.
- The slow-marker convention still holds; the guard (20.9.8) is in place.
- The one pre-existing failure (D6, the refresher self-id bug) is unrelated to
  this step and is fixed separately by **Step 30** — do not let it mask a
  consolidation regression; run with `--no-fail-fast` while validating.

## Dependencies

- None. This is the first step in the post-review sequence. It should be
  done before more tests are added.

## Deferred

- Adopting `cargo-nextest` (parallel cross-binary execution, lower per-binary
  overhead) is a reasonable further speed-up but is a tooling addition; note
  it as an option, do not require it here. Consolidation + target-dir
  isolation should already meet the target.
