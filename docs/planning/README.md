# Ranchero Implementation Plan

This directory breaks the Rust reimplementation from
[`../ARCHITECTURE-AND-RUST-SPEC.md`](../ARCHITECTURE-AND-RUST-SPEC.md) into an
ordered sequence of steps. Each step is described in its own `STEP-NN-*.md`
file with:

- **Goal** — the user-visible or internal capability delivered at the end.
- **Tests first** — the failing tests to write before any production code.
- **Implementation outline** — the minimum surface area to make those tests pass.
- **Acceptance criteria** — the conditions that indicate the step is finished.
- **Deferred** — anything explicitly left for a later step.

## Workflow (applies to every step)

1. Write tests that fail (`cargo test` shows them red).
2. Implement the smallest code that turns them green.
3. Refactor. Re-run tests. Commit only on green.
4. Update this README's status column when the step is committed.

## Step index

Status legend: ☐ planned · ◐ in progress · ☑ complete

| #   | Status | Step | File |
|----:|:------:|:-----|:-----|
|  01 | ☑ | Base CLI (subcommands + options + config-file flag) | [STEP-01-cli-base.md](done/STEP-01-cli-base.md) |
|  02 | ☑ | Configuration file + interactive TUI (`ranchero configure`) | [STEP-02-configuration.md](done/STEP-02-configuration.md) |
| 02.1 | ☑ | TUI keybindings: vi mode (priority) + emacs mode; `~/.editrc` detection; ratatui 0.30 upgrade | [STEP-02.1-configuration-keybindings.md](done/STEP-02.1-configuration-keybindings.md) |
| 02.2 | ☑ | Vi outer navigation: `j/k/h/l`, `i/a`, `:wq`/`:q!`/`ZZ`, vi-aware status bar and help | [STEP-02.2-vi-navigation.md](done/STEP-02.2-vi-navigation.md) |
|  03 | ☑ | Daemon lifecycle (`start` / `stop` / `status`, PID file, foreground vs background) | [STEP-03-daemon-lifecycle.md](done/STEP-03-daemon-lifecycle.md) |
|  04 | ☑ | Structured logging & verbose/debug flags | [STEP-04-logging.md](done/STEP-04-logging.md) |
|  05 | ☑ | Credential storage in OS keyring | [STEP-05-credentials.md](done/STEP-05-credentials.md) |
|  06 | ☑ | `zwift-proto` crate — prost-build against vendored zwift-offline proto tree (`crates/zwift-proto/proto/*.proto`, proto2) | [STEP-06-proto-crate.md](done/STEP-06-proto-crate.md) |
|  07 | ☑ | `zwift-api` — OAuth2 password grant + token refresh + REST client | [STEP-07-auth-and-rest.md](done/STEP-07-auth-and-rest.md) |
|  08 | ☑ | `zwift-relay` codec — header flags, `RelayIv`, AES-128-GCM-4 wire format | [STEP-08-relay-codec.md](done/STEP-08-relay-codec.md) |
|  09 | ☑ | Relay login (`/api/users/login`) + session refresh supervisor | [STEP-09-relay-session.md](done/STEP-09-relay-session.md) |
|  10 | ☑ | UDP channel with 25-shot hello handshake and world-time offset sync | [STEP-10-udp-channel.md](done/STEP-10-udp-channel.md) |
|  11 | ☑ | TCP channel with exponential backoff reconnect and watchdog | [STEP-11-tcp-channel.md](done/STEP-11-tcp-channel.md) |
| 11.5 | ☑ | Wire capture & replay — `ranchero start --capture <path>` + `ranchero replay`; produces the fixtures STEPS 08/18/19 consume | [STEP-11.5-wire-capture.md](done/STEP-11.5-wire-capture.md) |
| 11.6 | ☑ | Capture & stream-logging consistency review | [STEP-11.6-capture-consistency-review.md](done/STEP-11.6-capture-consistency-review.md) |
|  12 | ☑ | GameMonitor orchestration — sustainable end-to-end connectivity: auth + relay session + TCP + UDP + 1 Hz heartbeat + `udpConfigVOD` pool routing + idle suspension + watched-athlete switching + capture and tracing log. Internal sub-steps 12.1, 12.3, 12.4, 12.5 within the file. Corrective sub-steps 12.5–12.17, 12.20, 12.30 also in `done/`. | [STEP-12-game-monitor.md](done/STEP-12-game-monitor.md) |
| 12.2 | ☑ | `ranchero follow <file>` command for live capture-file tailing; reads a wire-capture file as it is written and prints each record (optionally decoded) to stdout. Independent of STEP-12 despite the digit overlap; to be implemented after STEP-12 is complete. | [STEP-12.2-follow-command.md](done/STEP-12.2-follow-command.md) |
|  13 | ☑ | `zwift-stats` rolling primitives — `RollingAverage`, `RollingPower`, NP, TSS | [STEP-13-rolling-stats.md](done/STEP-13-rolling-stats.md) |
|  14 | ☑ | Per-athlete `AthleteData` + `DataBucket`/`DataCollector` + peak periods | [STEP-14-athlete-data.md](done/STEP-14-athlete-data.md) |
|  15 | ☑ | Groups / laps / segments / W' balance / zones | [STEP-15-groups-segments.md](done/STEP-15-groups-segments.md) |
|  16 | ☑ | SQLite persistence — KV store, athletes DB, segment cache | [STEP-16-persistence.md](done/STEP-16-persistence.md) |
|  17 | ☑ | HTTP + WebSocket server compatible with `webserver.mjs` | [STEP-17-web-server.md](done/STEP-17-web-server.md) |
|  18 | ☑ | v1/v2 payload formatters (field-for-field parity) | [STEP-18-format-payloads.md](done/STEP-18-format-payloads.md), [STEP-18-parity-ledger.md](done/STEP-18-parity-ledger.md) |
|  19 | ☑ | Compatibility test battery (AES vector, header roundtrip, metric parity, widget smoke) | [STEP-19-compatibility-tests.md](done/STEP-19-compatibility-tests.md) |
|  20 | — | Additional considerations (parking lot) — deferred items from earlier steps plus the final-review gap analysis (20.21–20.28), with a revisit rule for each | [STEP-20-additional-considerations.md](done/STEP-20-additional-considerations.md) |

### Post-review parity phase (Steps 20.9, 21–33)

These steps come from the 2026-06-12 implementation review
([review.md](review.md)), which found that several STEP-20 items were marked
complete while their production wiring was missing. The steps below are
ordered by the review's "Suggested order of work"; each closes the review
findings named in its row. All design questions (Q1–Q8) were answered
2026-06-12, so every step is ready to build. **Step 20.9 runs first** — it
restores a fast test suite before the parity work adds more tests.

| #   | Status | Step | Closes | File |
|----:|:------:|:-----|:-------|:-----|
| 20.9 | ☐ | Restore a fast, contention-resilient test suite | test-suite addendum | [STEP-20.9-test-suite-speed.md](STEP-20.9-test-suite-speed.md) |
|  21 | ☐ | Forward relay game events to the web layer | G1, K4 | [STEP-21-relay-web-event-bridge.md](STEP-21-relay-web-event-bridge.md) |
|  22 | ☐ | Athlete profile fetch driver + W′/zones configuration | G3, G4 | [STEP-22-profile-fetch-and-wprime-zones.md](STEP-22-profile-fetch-and-wprime-zones.md) |
|  23 | ☐ | 1 Hz nearby/groups processor | G2, D2 | [STEP-23-nearby-groups-processor.md](STEP-23-nearby-groups-processor.md) |
|  24 | ☐ | Event-metadata producer | G5 | [STEP-24-event-metadata-producer.md](STEP-24-event-metadata-producer.md) |
|  25 | ☐ | `streams/*` WebSocket push | D3 | [STEP-25-streams-websocket-push.md](STEP-25-streams-websocket-push.md) |
|  26 | ☐ | Segment leaderboards: fetch, store, evict, serve | G6 | [STEP-26-segment-leaderboards.md](STEP-26-segment-leaderboards.md) |
|  27 | ☐ | Route progress + geometry RPC getters | G7, D4 | [STEP-27-route-progress-and-geometry-rpcs.md](STEP-27-route-progress-and-geometry-rpcs.md) |
|  28 | ☐ | Watched-athlete switching + real game-state event | G8, V4 | [STEP-28-watched-switching-and-game-state.md](STEP-28-watched-switching-and-game-state.md) |
|  29 | ☐ | Motion-based idle suspension | D5, V2 | [STEP-29-motion-based-idle-suspension.md](STEP-29-motion-based-idle-suspension.md) |
|  30 | ☐ | State refresher must not poll the monitor account | D6 | [STEP-30-refresher-self-id-fix.md](STEP-30-refresher-self-id-fix.md) |
|  31 | ☐ | Settings persistence + cadence clamp | G9, D1 | [STEP-31-settings-persistence-and-cadence-clamp.md](STEP-31-settings-persistence-and-cadence-clamp.md) |
|  32 | ☐ | Extract the shared TCP/UDP inbound-decode path | K1 | [STEP-32-shared-inbound-decode.md](STEP-32-shared-inbound-decode.md) |
|  33 | ☐ | Spec amendments for decided deviations | V1, V3 | [STEP-33-spec-amendments.md](STEP-33-spec-amendments.md) |

Later steps may be renumbered or split as the project progresses. Steps
01–19 are complete and their files have moved to `done/`. STEP 20 (now in
`done/`) is the parking lot; its final-review additions (20.21–20.28) record
the gaps between the verified payload shapes / isolated math and the
end-to-end production data path. Steps 21–33 are the planned response to
those gaps. STEP 12 also has a series of corrective sub-steps (12.5–12.17,
12.20, 12.30) in `done/` that are not listed individually in the index
above.

## Crate layout (target)

Per spec §7.2. The workspace will grow into:

```
ranchero/
  Cargo.toml           # workspace
  crates/
    ranchero-cli/      # the `ranchero` binary — STEP 01+
    zwift-proto/       # prost-generated types — STEP 06
    zwift-api/         # REST + OAuth2 — STEP 07
    zwift-relay/       # protocol core — STEPS 08-12
    zwift-stats/       # rolling windows, NP, TSS, W' — STEPS 13-15
    zwift-routes/      # static world/route tables — on demand
    zwift-daemon/      # the long-running service binary — STEPS 03, 17+
```

The current layout (single-crate `ranchero`) is temporary; it becomes a
workspace root once STEP 01 requires more than one module.
