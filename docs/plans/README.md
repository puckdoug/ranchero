# Ranchero Implementation Plan

This directory breaks the Rust reimplementation from
[`../ARCHITECTURE-AND-RUST-SPEC.md`](../ARCHITECTURE-AND-RUST-SPEC.md) into an
ordered sequence of steps. Each step lives in its own `STEP-NN-*.md` file with:

- **Goal** — the user-visible or internal capability delivered at the end.
- **Tests first** — the failing tests to write before any production code.
- **Implementation outline** — the minimum surface area to make those tests pass.
- **Acceptance criteria** — how we know the step is finished.
- **Deferred** — anything explicitly left for a later step.

## Workflow (applies to every step)

1. Write tests that fail (`cargo test` shows them red).
2. Implement the smallest code that turns them green.
3. Refactor. Re-run tests. Commit only on green.
4. Update this README's status column when the step lands.

## Step index

Status legend: ☐ planned · ◐ in progress · ☑ complete

| #   | Status | Step | File |
|----:|:------:|:-----|:-----|
|  01 | ☑ | Base CLI (subcommands + options + config-file flag) | [STEP-01-cli-base.md](STEP-01-cli-base.md) |
|  02 | ☑ | Configuration file + interactive TUI (`ranchero configure`) | [STEP-02-configuration.md](STEP-02-configuration.md) |
| 02.1 | ☑ | TUI keybindings: vi mode (priority) + emacs mode; `~/.editrc` detection; ratatui 0.30 upgrade | [STEP-02.1-configuration-keybindings.md](STEP-02.1-configuration-keybindings.md) |
| 02.2 | ☑ | Vi outer navigation: `j/k/h/l`, `i/a`, `:wq`/`:q!`/`ZZ`, vi-aware status bar and help | [STEP-02.2-vi-navigation.md](STEP-02.2-vi-navigation.md) |
|  03 | ☑ | Daemon lifecycle (`start` / `stop` / `status`, PID file, foreground vs background) | [STEP-03-daemon-lifecycle.md](done/STEP-03-daemon-lifecycle.md) |
|  04 | ☐ | Structured logging & verbose/debug flags | [STEP-04-logging.md](STEP-04-logging.md) |
|  05 | ☐ | Credential storage in OS keyring | [STEP-05-credentials.md](STEP-05-credentials.md) |
|  06 | ☐ | `zwift-proto` crate — prost-build against vendored zwift-offline proto tree (`crates/zwift-proto/proto/*.proto`, proto2) | [STEP-06-proto-crate.md](STEP-06-proto-crate.md) |
|  07 | ☐ | `zwift-api` — OAuth2 password grant + token refresh + REST client | [STEP-07-auth-and-rest.md](STEP-07-auth-and-rest.md) |
|  08 | ☐ | `zwift-relay` codec — header flags, `RelayIv`, AES-128-GCM-4 wire format | [STEP-08-relay-codec.md](STEP-08-relay-codec.md) |
|  09 | ☐ | Relay login (`/api/users/login`) + session refresh supervisor | [STEP-09-relay-session.md](STEP-09-relay-session.md) |
|  10 | ☐ | UDP channel with 25-shot hello handshake and world-time offset sync | [STEP-10-udp-channel.md](STEP-10-udp-channel.md) |
|  11 | ☐ | TCP channel with exponential backoff reconnect and watchdog | [STEP-11-tcp-channel.md](STEP-11-tcp-channel.md) |
| 11.5 | ☐ | Wire capture & replay — `ranchero start --capture <path>` + `ranchero replay`; produces the fixtures STEPS 08/18/19 consume | [STEP-11.5-wire-capture.md](STEP-11.5-wire-capture.md) |
|  12 | ☐ | GameMonitor orchestration — server pool selection, watched-athlete routing | [STEP-12-game-monitor.md](STEP-12-game-monitor.md) |
|  13 | ☐ | `zwift-stats` rolling primitives — `RollingAverage`, `RollingPower`, NP, TSS | [STEP-13-rolling-stats.md](STEP-13-rolling-stats.md) |
|  14 | ☐ | Per-athlete `AthleteData` + `DataBucket`/`DataCollector` + peak periods | [STEP-14-athlete-data.md](STEP-14-athlete-data.md) |
|  15 | ☐ | Groups / laps / segments / W' balance / zones | [STEP-15-groups-segments.md](STEP-15-groups-segments.md) |
|  16 | ☐ | SQLite persistence — KV store, athletes DB, segment cache | [STEP-16-persistence.md](STEP-16-persistence.md) |
|  17 | ☐ | HTTP + WebSocket server compatible with `webserver.mjs` | [STEP-17-web-server.md](STEP-17-web-server.md) |
|  18 | ☐ | v1/v2 payload formatters (field-for-field parity) | [STEP-18-format-payloads.md](STEP-18-format-payloads.md) |
|  19 | ☐ | Compatibility test battery (AES vector, header roundtrip, metric parity, widget smoke) | [STEP-19-compatibility-tests.md](STEP-19-compatibility-tests.md) |

Later steps may be renumbered or split as we learn more. Steps 01–03 are
elaborated up-front; 04 onward are currently light sketches and will be
fleshed out as we approach them.

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
workspace root the moment STEP 01 needs more than one module.
