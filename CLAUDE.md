# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Ranchero is a Rust reimplementation of the live-data core of
[sauce4zwift](https://github.com/SauceLLC/sauce4zwift): a daemon that
logs into Zwift, joins the relay mesh as an independent client (TCP/3025
and UDP/3024 with AES-128-GCM-4 encrypted protobuf), and computes/serves
live telemetry. Licensed AGPL-3.0-only.

The authoritative architectural reference is
`docs/ARCHITECTURE-AND-RUST-SPEC.md`. Implementation is staged through
`docs/plans/STEP-NN-*.md` files; completed steps move to
`docs/plans/done/`. Look at `docs/plans/README.md` for the step index
and status table before starting new work.

A `sauce4zwift` symlink at the repo root points at the upstream
JavaScript implementation — it is a **porting reference only**. No
build, test, or runtime path may resolve through it.

## Workspace layout

Cargo workspace with the binary at the root and supporting crates under
`crates/`:

- Root crate `ranchero` (`src/`) — the `ranchero` binary, CLI dispatch,
  config + keyring, daemon lifecycle, TUI for `ranchero configure`,
  replay/follow printers.
- `crates/zwift-proto` — `prost-build` types from a vendored copy of
  the [zoffline/zwift-offline](https://github.com/zoffline/zwift-offline)
  proto2 tree under `crates/zwift-proto/proto/`. Not the sauce4zwift
  merged proto3 schema.
- `crates/zwift-api` — OAuth2 (Keycloak password grant) + authenticated
  REST client.
- `crates/zwift-relay` — relay codec (`Header`, `RelayIv`, AES-GCM),
  session login/refresh supervisor, TCP and UDP channels, world-time
  sync, and wire capture/replay (`CaptureWriter`/`CaptureReader`/
  `CaptureFollower`).
- `crates/zwift-stats` — rolling primitives (`RollingAverage`,
  `RollingPower`, NP, TSS), `DataBucket`/`DataCollector`,
  `AthleteData`/`AthleteRegistry`, W' balance, zones, groups, segments.

## Commands

### Tests

The default run is fast; slow tests are gated by `#[ignore]` with a
reason starting `slow:` (see `README.md` for the full convention).

| Command | What it runs |
|---|---|
| `cargo test` | Fast tests only — inner dev loop. |
| `cargo test -- --ignored` | Only the slow set. |
| `cargo test -- --include-ignored` | Everything; use before merging. |
| `cargo test -p zwift-relay` | One workspace crate. |
| `cargo test -p zwift-stats --test athlete_data` | A single integration test file. |
| `cargo test -p zwift-stats athlete_data::name_of_test` | A single test by path. |

When marking a new test slow, the reason must lead with `slow:` and
include enough context (root cause, plan/comment reference) for a
future reader to decide whether the marker still applies.

### Build / run

`cargo build` and `cargo run -- <subcommand>`. The CLI subcommands are
`configure`, `start`, `stop`, `status`, `auth-check`, `replay <path>`,
`follow <path>`. `-D`/`--debug` implies `--foreground`. `--capture
<path>` on `start` writes a wire-capture file consumed by `replay`/
`follow` and by the fixtures under `crates/zwift-relay/tests/fixtures/`.

## Domain notes that aren't obvious from the code

- **Two-account model.** Zwift relay telemetry is scoped by the
  *watched* athlete, not by the logged-in account, so the daemon
  authenticates a `main` account (the rider) and a `monitor` account
  (used purely to receive the relay stream). Both flow through the
  config + keyring; CLI overrides are `--mainuser`/`--mainpassword`/
  `--monitoruser`/`--monitorpassword`.
- **`auth-check` opens no sockets.** It renders the exact HTTP request
  that `ZwiftAuth::login()` would send (password redacted). Use it as a
  pre-flight gate — Zwift locks the account after a few bad Keycloak
  logins.
- **No JavaScript replay path.** Wire capture is a ranchero addition;
  you cannot drive sauce4zwift against a recorded ride, so parity
  schemes that assume this are not feasible.
- **Field visibility convention.** POD/snapshot types expose `pub`
  fields; stateful aggregators (with accumulators, buffers, invariants)
  keep fields private and expose accessor methods.
- **Outbound TCP capture quirk.** Outbound TCP records in a capture
  file include the 2-byte BE length prefix (`frame_tcp` output); every
  other direction/transport stores the frame body directly. See the
  branch in `src/cli.rs::print_follow_to` for the parsing rule.

## Workflow

- Test-first. Write a failing test, then the smallest code that turns
  it green. The plan files under `docs/plans/` typically open with a
  "Tests first" section listing the failing cases.
- Do not run `git commit`, `merge`, `rebase`, or `push`. Plan-file
  moves into `docs/plans/done/` are managed by Doug, not the agent.
- Lifecycle/operational events (daemon start/stop) must reach the
  logfile by default; do not gate them behind `-v`.
