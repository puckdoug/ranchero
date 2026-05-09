# STEP-12.20 — Deferred and out-of-scope item summary

**Generated:** 2026-05-09. **Updated:** 2026-05-09 — every previously-missing item has been assigned a future home; STEP-20 was extended to cover items that did not fit any earlier numbered step.

Survey of every "Deferred" or "Out of scope" item declared across past plans, classified by current state.

## Method

Every plan in `docs/plans/` and `docs/plans/done/` was scanned for sections titled "Deferred" or "Out of scope". Each enumerated item was classified by:

- **implemented** — feature now exists in the codebase (verified by code search).
- **implemented (partial)** — symbol and trace events exist, but the underlying behaviour is a placeholder. Completion is tracked separately under the "deferred until" target shown.
- **deferred until STEP-N (§N.M)** — scheduled in a future numbered plan, with a section reference where the entry has explicit text.

Implementation evidence cites concrete file and line references in `src/` or `crates/`. Scheduling evidence cites the relevant section of the future plan.

## State breakdown

- 19 implemented.
- 4 implemented (partial); completion tracked under STEP-20 §20.14.
- 45 deferred to a numbered future plan, of which:
  - 9 to STEPs 13-19 (the data, stats, persistence, web, formatter, and compatibility-test stack) — these were already scheduled before this audit.
  - 36 to STEP-20 — of which 2 (items 44, 45) were already at §20.3 before this audit, and 34 were newly assigned during this audit so that no item is left unscheduled.

There are no items in the "missing" state after this update.

## Summary table

| #  | Item | Source plan | State |
| -- | ---- | ----------- | ----- |
| 1  | Honouring CLI options at runtime | STEP-01 | implemented |
| 2  | Interactive `ranchero configure` TUI | STEP-01 | implemented |
| 3  | Workspace split (`zwift-proto` etc.) | STEP-01 | implemented |
| 4  | Environment-variable fallbacks (`RANCHERO_*`) | STEP-01 | implemented |
| 5  | Real keyring backend | STEP-02 | implemented |
| 6  | Schema-version migrations (v2+) | STEP-02 | deferred until STEP-20 §20.4 |
| 7  | Mouse support and resize handling beyond ratatui defaults | STEP-02 | deferred until STEP-20 §20.5 |
| 8  | Configuration categories beyond v1 schema (mods, route overrides) | STEP-02 | deferred until STEP-20 §20.4 |
| 9  | `--editing-mode` command-line flag | STEP-02.1 | deferred until STEP-20 §20.4 |
| 10 | Visual-mode selector for log-level | STEP-02.1 | deferred until STEP-20 §20.5 |
| 11 | Mouse cursor positioning within fields | STEP-02.1 | deferred until STEP-20 §20.5 |
| 12 | Syntax highlighting in Review screen TOML preview | STEP-02.1 | deferred until STEP-20 §20.6 |
| 13 | `gg` and `G` (first/last screen) | STEP-02.2 | deferred until STEP-20 §20.5 |
| 14 | `0`, `$`, `^` line motions in outer Normal mode | STEP-02.2 | deferred until STEP-20 §20.5 |
| 15 | Numeric prefix counts (`3j`, `5l`, `2dd`) | STEP-02.2 | deferred until STEP-20 §20.5 |
| 16 | `c{motion}` and `s` change/substitute | STEP-02.2 | deferred until STEP-20 §20.5 |
| 17 | Cross-field paste from edtui clipboard | STEP-02.2 | deferred until STEP-20 §20.5 |
| 18 | Mouse click to focus a field | STEP-02.2 | deferred until STEP-20 §20.5 |
| 19 | Custom `:` commands beyond `:w`, `:wq`, `:x`, `:q`, `:q!`, `:u`, `:undo` | STEP-02.2 | deferred until STEP-20 §20.5 |
| 20 | Redo (`Ctrl-R` / `:redo`) | STEP-02.2 | deferred until STEP-20 §20.5 |
| 21 | Daemon log rotation and stdout/stderr redirection | STEP-03 | implemented |
| 22 | Status counters (relay state, athletes seen, packets/s) | STEP-03 | deferred until STEP-14 |
| 23 | Windows service integration | STEP-03 | deferred until STEP-20 §20.8 |
| 24 | Privileged-capabilities drop | STEP-03 | deferred until STEP-20 §20.8 |
| 25 | Log rotation (size/daily) | STEP-04 | deferred until STEP-20 §20.7 |
| 26 | Per-module log-level overrides | STEP-04 | implemented |
| 27 | JSON / structured log output | STEP-04 | deferred until STEP-20 §20.7 |
| 28 | Log shipping to external collectors | STEP-04 | deferred until STEP-20 §20.7 |
| 29 | Wire `Arc<CaptureWriter>` into channel configs | STEP-11.6 | implemented |
| 30 | Establish session and channels from the daemon | STEP-11.6 | implemented |
| 31 | `flush_and_close()` on supervisor shutdown path | STEP-11.6 | implemented |
| 32 | Decoding `ServerToClient` into per-athlete data model | STEP-12 | deferred until STEP-14 |
| 33 | Rolling-window statistics (NP, TSS, peak power) | STEP-12 | deferred until STEP-13 |
| 34 | W' balance, segment matching, group detection | STEP-12 | deferred until STEP-15 |
| 35 | SQLite persistence of athlete history | STEP-12 | deferred until STEP-16 |
| 36 | HTTP and WebSocket server compatible with `webserver.mjs` | STEP-12 | deferred until STEP-17 |
| 37 | v1 / v2 payload formatters | STEP-12 | deferred until STEP-18 |
| 38 | Compatibility test battery against captured fixtures | STEP-12 | deferred until STEP-19 |
| 39 | File-system event notification (`inotify`/`kqueue`) for follower | STEP-12.2 | deferred until STEP-20 §20.9 |
| 40 | Capture-file rotation support | STEP-12.2 | deferred until STEP-20 §20.9 |
| 41 | JSON output mode and selective field formatting for `--decode` | STEP-12.2 | deferred until STEP-20 §20.9 |
| 42 | Filter flags (direction, transport, message type) | STEP-12.2 | deferred until STEP-20 §20.9 |
| 43 | "From offset" or "from timestamp" follower mode | STEP-12.2 | deferred until STEP-20 §20.9 |
| 44 | `reqwest::Client` (or HTTP-trait) injection into `ZwiftAuth` | STEP-12.5 §F.5 | deferred until STEP-20 §20.3 |
| 45 | Surface `source` and `user_agent` to operator configuration | STEP-12.5 §F.5 | deferred until STEP-20 §20.3 |
| 46 | Retire `start_inner` / `start_with_deps*` family | STEP-12.11 | deferred until STEP-20 §20.10 |
| 47 | Sticky TCP server selection across reconnects | STEP-12.11 | implemented (partial); completion deferred until STEP-20 §20.14 |
| 48 | Real `RelaySessionSupervisor` (replacing default factory) | STEP-12.11 | implemented |
| 49 | L1 — `_refreshStates` polling fallback | STEP-12.14 | implemented |
| 50 | L2 — Auto-suspend / auto-resume on idle | STEP-12.14 | implemented |
| 51 | L4 — TCP server pinning across reconnects | STEP-12.14 | implemented (partial); completion deferred until STEP-20 §20.14 |
| 52 | L5 — Connect retry with exponential backoff | STEP-12.14 | implemented |
| 53 | L6 — Multi-UDP-channel with grace-shutdown swap | STEP-12.14 | implemented (partial); completion deferred until STEP-20 §20.14 |
| 54 | 12.13 §6 — Mid-session pool updates wired into `recv_loop` | STEP-12.14 | implemented |
| 55 | 12.13 §4 — Per-watched-athlete `find_best_udp_server` integration | STEP-12.14 | implemented |
| 56 | 12.14 k3 — Portal-pool handling | STEP-12.14 | deferred until STEP-20 §20.11 |
| 57 | 12.14 M3 / k1 — TCP non-hello flag=0 / hello SEQNO=0 cosmetic cleanup | STEP-12.14 | deferred until STEP-20 §20.11 |
| 58 | Sauce `_processIncomingPlayerState` flag-bit decoding | STEP-12.14 | deferred until STEP-14 |
| 59 | Supervisor L4 pinned-trace firing once F3 lands | STEP-12.15 | implemented |
| 60 | TUI watched-athlete-ID configure path | STEP-12.15 | implemented |
| 61 | Proto-fork items N1, N12, C11 | STEP-12.15 | deferred until STEP-20 §20.11 |
| 62 | Auth and session-login retry across all error categories | STEP-12.16 | deferred until STEP-20 §20.12 |
| 63 | TCP server pinning across reconnects (12.16 restatement) | STEP-12.16 | implemented (partial); completion deferred until STEP-20 §20.14 |
| 64 | UDP error-count threshold reconnect | STEP-12.16 | deferred until STEP-20 §20.12 |
| 65 | TCP server pool refresh on re-login | STEP-12.16 | implemented |
| 66 | Changing the state refresher's cadence | STEP-12.16 | deferred until STEP-20 §20.15 (acknowledged out of scope) |
| 67 | Reuse of resume code path for mid-ride course transitions | STEP-12.16 | deferred until STEP-20 §20.13 |
| 68 | Reconnecting at the auth or session layer beyond F3 / F4 | STEP-12.16 | deferred until STEP-20 §20.12 |

## Detail

### 1 — Honouring CLI options at runtime (STEP-01)

The deferral was that STEP-01 only stubbed CLI parsing. All subsequent steps consume the resolved options. Verified by `src/cli.rs` and the dispatch path in `src/cli.rs:181-187` plus the consumer wiring in `src/daemon/mod.rs`. **Implemented.**

### 2 — Interactive `ranchero configure` TUI (STEP-01)

Delivered in STEP-02 and elaborated through STEP-02.2. Evidence: `src/tui/` contains `mod.rs`, `model.rs`, `view.rs`, `driver.rs`, `backend.rs`. **Implemented.**

### 3 — Workspace split (STEP-01)

`crates/zwift-api`, `crates/zwift-proto`, and `crates/zwift-relay` exist as separate crates. **Implemented.**

### 4 — Environment-variable fallbacks (STEP-01)

`RANCHERO_MAIN_USER`, `RANCHERO_MONITOR_USER`, `RANCHERO_SERVER_PORT`, `RANCHERO_LOG_FILE`, and others are honoured in `src/config/mod.rs:392-445`. **Implemented.**

### 5 — Real keyring backend (STEP-02)

Delivered by STEP-05 (now under `src/credentials/`). **Implemented.**

### 6 — Schema-version migrations (STEP-02)

The configuration schema has not advanced past v1, so no migration code exists. **Deferred until STEP-20 §20.4.**

### 7 — Mouse support and resize handling beyond ratatui defaults (STEP-02)

No mouse-event handling code exists in `src/tui/`. **Deferred until STEP-20 §20.5.**

### 8 — Configuration categories beyond v1 schema (STEP-02)

No mods or route-overrides sections appear in `src/config/`. **Deferred until STEP-20 §20.4.**

### 9 — `--editing-mode` command-line flag (STEP-02.1)

A search for `editing-mode`/`editing_mode` shows no command-line flag in `src/cli.rs`. The configuration file value remains the only way to choose. **Deferred until STEP-20 §20.4.**

### 10 — Visual-mode selector widget for log-level (STEP-02.1)

No selector widget in `src/tui/`. **Deferred until STEP-20 §20.5.**

### 11 — Mouse cursor positioning within fields (STEP-02.1)

No mouse-event handling. **Deferred until STEP-20 §20.5.**

### 12 — Syntax highlighting in TOML preview (STEP-02.1)

No `tree-sitter`, `syntect`, or grammar reference in `src/tui/`. **Deferred until STEP-20 §20.6.**

### 13 — `gg` and `G` (first/last screen) (STEP-02.2)

`src/tui/driver.rs` shows no bindings for `gg`/`G`. **Deferred until STEP-20 §20.5.**

### 14 — `0`, `$`, `^` line motions (STEP-02.2)

Not implemented in outer Normal mode. **Deferred until STEP-20 §20.5.**

### 15 — Numeric prefix counts (STEP-02.2)

No `repeat_count` / `pending_count` handling in `src/tui/`. **Deferred until STEP-20 §20.5.**

### 16 — `c{motion}` and `s` (STEP-02.2)

Only edtui's in-Insert behaviour applies. **Deferred until STEP-20 §20.5.**

### 17 — Cross-field paste (STEP-02.2)

`paste_buffer` exists in `src/tui/model.rs:261`, but the deferred case (a `dw` inside edtui populating the outer paste buffer) is not handled. **Deferred until STEP-20 §20.5.**

### 18 — Mouse click to focus a field (STEP-02.2)

No mouse event handling. **Deferred until STEP-20 §20.5.**

### 19 — Custom `:` commands beyond the documented set (STEP-02.2)

No new `:` commands have been added. **Deferred until STEP-20 §20.5.**

### 20 — Redo (STEP-02.2)

No `Redo` / `:redo` references in `src/tui/`. **Deferred until STEP-20 §20.5.**

### 21 — Daemon log rotation and stdout/stderr redirection (STEP-03)

stdout/stderr redirection for the backgrounded daemon is implemented through `src/logging/mod.rs` (the `non_blocking` appender plus the daemonisation logic in `src/daemon/`). The narrow "log rotation" item is tracked as #25 below. **Implemented** (the broader redirection part; the rotation sub-item is deferred to STEP-20 §20.7).

### 22 — Status counters (relay state, athletes seen, packets/s) (STEP-03)

Per-athlete data and counters belong to STEP-14. Confirmed at `docs/plans/STEP-14-athlete-data.md` (the `DataCollector` per signal). **Deferred until STEP-14.**

### 23 — Windows service integration (STEP-03)

No Windows-service code. **Deferred until STEP-20 §20.8.**

### 24 — Privileged-capabilities drop (STEP-03)

No `setuid` / capability-drop code. **Deferred until STEP-20 §20.8.**

### 25 — Log rotation, daily or size-based (STEP-04)

`src/logging/mod.rs` uses `tracing_appender::non_blocking` with `OpenOptions::new().create(true).append(true)` (line 82). No `tracing_appender::rolling` consumer exists. **Deferred until STEP-20 §20.7.**

### 26 — Per-module level overrides (STEP-04)

`src/logging/mod.rs` uses `EnvFilter::try_new(&directive)`, which natively accepts `zwift_relay=trace,zwift_api=debug` style directives via `RUST_LOG`. **Implemented.**

### 27 — JSON / structured log output (STEP-04)

Only `fmt::layer()` is configured; no JSON layer. **Deferred until STEP-20 §20.7.**

### 28 — Log shipping to external collectors (STEP-04)

Not present. **Deferred until STEP-20 §20.7.**

### 29 — Wire `Arc<CaptureWriter>` into channel configs (STEP-11.6)

Wired through `src/daemon/relay.rs` where the supervisor passes the writer into both UDP and TCP channel configurations. **Implemented.**

### 30 — Establish session and channels from the daemon (STEP-11.6)

`src/daemon/relay.rs` implements `RelayRuntime::start_with_all_deps`, the supervisor, the TCP/UDP channels, and the recv loop. **Implemented.**

### 31 — `flush_and_close()` on supervisor shutdown path (STEP-11.6)

Called from the runtime error and shutdown paths in `src/daemon/relay.rs:1300-1310` (capture writer closure on error) and on shutdown in the same file. **Implemented.**

### 32 — Decoding `ServerToClient` into per-athlete data model (STEP-12)

Covered by `docs/plans/STEP-14-athlete-data.md` (`AthleteData`, `DataBucket`, `DataCollector`). **Deferred until STEP-14.**

### 33 — Rolling-window statistics (STEP-12)

Covered by `docs/plans/STEP-13-rolling-stats.md` (`RollingAverage`, `RollingPower`, NP/TSS). **Deferred until STEP-13.**

### 34 — W' balance, segment matching, group detection (STEP-12)

Covered by `docs/plans/STEP-15-groups-segments.md`. **Deferred until STEP-15.**

### 35 — SQLite persistence (STEP-12)

Covered by `docs/plans/STEP-16-persistence.md` (`store.sqlite`, `athletes.sqlite`, `segments.sqlite`). **Deferred until STEP-16.**

### 36 — HTTP / WebSocket server (STEP-12)

Covered by `docs/plans/STEP-17-web-server.md`. **Deferred until STEP-17.**

### 37 — v1 / v2 payload formatters (STEP-12)

Covered by `docs/plans/STEP-18-format-payloads.md`. **Deferred until STEP-18.**

### 38 — Compatibility test battery (STEP-12)

Covered by `docs/plans/STEP-19-compatibility-tests.md`. **Deferred until STEP-19.**

### 39 — File-system event notification for follower (STEP-12.2)

The follower polls. No `inotify` / `kqueue` / `notify` crate dependency. **Deferred until STEP-20 §20.9.**

### 40 — Capture-file rotation support (STEP-12.2)

No rotation logic in `crates/zwift-relay/src/capture.rs`. **Deferred until STEP-20 §20.9.**

### 41 — JSON output mode for `--decode` (STEP-12.2)

No JSON formatter on the follower path. **Deferred until STEP-20 §20.9.**

### 42 — Filter flags for follower (STEP-12.2)

No filter logic. **Deferred until STEP-20 §20.9.**

### 43 — "From offset" / "from timestamp" follower mode (STEP-12.2)

No corresponding flags or read positioning logic. **Deferred until STEP-20 §20.9.**

### 44 — Higher-level HTTP-client trait into `ZwiftAuth` (STEP-12.5 §F.5)

Carried into `docs/plans/STEP-20-additional-considerations.md` §20.3 with explicit decision rule. **Deferred until STEP-20 §20.3.**

### 45 — Operator configuration of `source` and `user_agent` (STEP-12.5 §F.5)

Same parking-lot entry, §20.3. **Deferred until STEP-20 §20.3.**

### 46 — Retire `start_inner` and `start_with_deps*` family (STEP-12.11)

`src/daemon/relay.rs` still defines `start_inner` and the related entry points. **Deferred until STEP-20 §20.10.**

### 47 — Sticky TCP server selection across reconnects (STEP-12.11)

Pinned IP is tracked at `src/daemon/relay.rs:1693` and the reconnect path checks for it (line 1714-1724). However, this only emits `relay.runtime.tcp_server_pinned` and does not actually re-establish a TCP channel on the pinned IP — full channel recreation on `LoggedIn` is also incomplete. **Implemented (partial); completion deferred until STEP-20 §20.14.**

### 48 — Real `RelaySessionSupervisor` (STEP-12.11)

`crates/zwift-relay/src/session.rs:259` defines `RelaySessionSupervisor` with full event broadcasting (`SessionEvent::LoggedIn` / `Refreshed` / `RefreshFailed` / `LoginFailed`), and `src/daemon/relay.rs:1573-1798` consumes it. **Implemented.**

### 49 — L1: `_refreshStates` polling fallback (STEP-12.14)

`run_state_refresher` at `src/daemon/relay.rs:578-696` polls `auth.get_player_state(watched_id)` with the 3 s minimum / 30 s expanding / 5 min cap cadence. **Implemented.**

### 50 — L2: Auto-suspend / auto-resume on idle (STEP-12.14)

`inner.suspended` flag at `src/daemon/relay.rs:486`, with `relay.runtime.suspended_idle` and `relay.runtime.resumed` events emitted (lines 575-635) and the heartbeat scheduler observing the flag (`relay.heartbeat.suspended` at lines 825-839). **Implemented.**

### 51 — L4: TCP server pinning across reconnects (STEP-12.14)

See item 47. The pinned IP is tracked and the trace event fires, but the actual reconnect-with-pinned-IP behaviour is not exercised because TCP channel recreation on `LoggedIn` is not wired. **Implemented (partial); completion deferred until STEP-20 §20.14.**

### 52 — L5: Connect retry with exponential backoff (STEP-12.14)

`start_with_retry` at `src/daemon/relay.rs:1472-1313` retries on `TcpConnect`, `NoUdpConfig`, and `EstablishedTimeout` with `1000 ms × 1.2^attempt` backoff, capped at 5 min, up to 50 attempts. **Implemented.**

### 53 — L6: Multi-UDP-channel with grace-shutdown swap (STEP-12.14)

`recompute_udp_selection` at `src/daemon/relay.rs:519-565` emits `relay.udp.channel.grace_shutdown` and broadcasts `GameEvent::PoolSwap`. The 60-second `tokio::spawn` body explicitly contains a "Placeholder: actual channel transfer is implemented in L6" comment (line 556). The pool router updates and selection logic exist, but the actual channel swap (spawn new channel, close old one after 60 s) is not done. **Implemented (partial); completion deferred until STEP-20 §20.14.**

### 54 — 12.13 §6: Mid-session pool updates wired into `recv_loop` (STEP-12.14)

`src/daemon/relay.rs:3261-3278` calls `extract_udp_pools` on inbound `Inbound(stc)` and applies the result to `inner.pool_router`, then calls `recompute_udp_selection`. **Implemented.**

### 55 — 12.13 §4: Per-watched-athlete `find_best_udp_server` integration (STEP-12.14)

`recompute_udp_selection` (line 519) uses `find_best_udp_server` against `(watched.realm, watched.course_id)` with a fallback to `(0, 0)`. **Implemented.**

### 56 — k3: Portal-pool handling (STEP-12.14)

`tests/relay_runtime.rs:2622` defines `portal_pool_handled_via_portal_key`, but a search for `portal` in `src/daemon/relay.rs` and `crates/zwift-relay/src/lib.rs` returns no matches. The test is presumably red or is a placeholder. **Deferred until STEP-20 §20.11.**

### 57 — M3 / k1: TCP non-hello flag=0 cleanup and hello SEQNO=0 omission (STEP-12.14)

These are flagged in the source plan as cosmetic ("Server tolerates both"). **Deferred until STEP-20 §20.11.**

### 58 — Sauce `_processIncomingPlayerState` flag-bit decoding (STEP-12.14)

Explicitly noted in the source plan as belonging to "the per-athlete data-model STEP (13+), not here". Covered by `docs/plans/STEP-14-athlete-data.md` (per-athlete `PlayerState` decoding). **Deferred until STEP-14.**

### 59 — Supervisor L4 pinned-trace firing once F3 lands (STEP-12.15)

The trace fires from `src/daemon/relay.rs:1719-1724`. **Implemented.**

### 60 — TUI watched-athlete-ID configure path (STEP-12.15)

The TUI already exposes the watched-athlete-ID field; no work was needed. **Implemented.**

### 61 — Proto-fork items N1, N12, C11 (STEP-12.15)

Marked in the source plan as "fix only if C5 + C6/7/8 don't unblock the trace, and they did". **Deferred until STEP-20 §20.11.**

### 62 — Auth and session-login retry across all error categories (STEP-12.16)

`start_with_retry` only retries `TcpConnect`, `NoUdpConfig`, and `EstablishedTimeout` (line 1296-1298). Auth-layer retry is not wired. **Deferred until STEP-20 §20.12.**

### 63 — TCP server pinning across reconnects (12.16 restatement)

Same status as item 47/51 — pinned-IP trace fires, but no actual reconnect-with-pinned-IP is exercised. **Implemented (partial); completion deferred until STEP-20 §20.14.**

### 64 — UDP error-count threshold reconnect (STEP-12.16)

No `inc_error_count` or equivalent in `src/daemon/relay.rs`. **Deferred until STEP-20 §20.12.**

### 65 — TCP server pool refresh on re-login (STEP-12.16)

The supervisor emits `LoggedIn { tcp_servers, … }` on every re-login, and the supervisor-event handler at `src/daemon/relay.rs:1704-1738` checks for the pinned IP in the new server list. **Implemented.**

### 66 — Changing the state refresher's cadence (STEP-12.16)

Acknowledged as explicitly out of scope in STEP-12.16 §7. The current cadence is preserved at `src/daemon/relay.rs:584-587`; STEP-20 §20.15 records the decision rule for revisiting it. **Deferred until STEP-20 §20.15** (acknowledged out of scope).

### 67 — Resume code path for mid-ride course transitions (STEP-12.16)

The resume logic only handles "first course on entering a game". A search for course-transition handling in `src/daemon/relay.rs` shows only the initial entry path (`suspended_no_course` at line 1619). **Deferred until STEP-20 §20.13.**

### 68 — Reconnecting at the auth or session layer beyond F3 / F4 (STEP-12.16)

No additional reconnect plumbing. **Deferred until STEP-20 §20.12.**

## Items newly assigned to STEP-20 in this audit

The original audit (earlier in the day) classified 34 items as "missing": neither implemented nor scheduled. STEP-20 was extended on 2026-05-09 to cover all of them. The grouping below shows which item went into which new STEP-20 subsection:

- **§20.4 — Configuration extensibility:** items #6, #8, #9 (schema migrations, v1+ categories, `--editing-mode` flag).
- **§20.5 — TUI vi-mode completeness and mouse support:** items #7, #10, #11, #13, #14, #15, #16, #17, #18, #19, #20 (mouse, motions, prefix counts, change/substitute, cross-field paste, custom `:` commands, redo, log-level selector).
- **§20.6 — Syntax highlighting in the Review-screen TOML preview:** item #12.
- **§20.7 — Daemon log rotation, structured output, and shipping:** items #25, #27, #28.
- **§20.8 — Cross-platform daemon (Windows service, Linux capability drop):** items #23, #24.
- **§20.9 — `ranchero follow` enhancements:** items #39, #40, #41, #42, #43.
- **§20.10 — `RelayRuntime::start_*` consolidation:** item #46.
- **§20.11 — Relay-protocol cosmetic and niche items:** items #56, #57, #61.
- **§20.12 — Auth and session resilience: broader retry, error counting:** items #62, #64, #68.
- **§20.13 — Mid-ride course transitions and resume reuse:** item #67.
- **§20.14 — Completion of partial implementations:** items #47, #51, #53, #63 (these were already classified as "implemented (partial)"; STEP-20 §20.14 is now their explicit completion target).
- **§20.15 — State-refresher cadence (acknowledged out of scope):** item #66.

None of the missing items fit naturally inside STEPs 13-19 — those steps are all about the data model, rolling stats, persistence, web server, formatters, and compatibility tests. The missing items are TUI polish, operational logging, follower enhancements, cross-platform daemon work, and relay-runtime resilience gaps; STEP-20 is the correct home.

## Cross-references

- `docs/plans/STEP-13-rolling-stats.md` — covers item 33.
- `docs/plans/STEP-14-athlete-data.md` — covers items 22, 32, 58.
- `docs/plans/STEP-15-groups-segments.md` — covers item 34.
- `docs/plans/STEP-16-persistence.md` — covers item 35.
- `docs/plans/STEP-17-web-server.md` — covers item 36.
- `docs/plans/STEP-18-format-payloads.md` — covers item 37.
- `docs/plans/STEP-19-compatibility-tests.md` — covers item 38.
- `docs/plans/STEP-20-additional-considerations.md`:
  - §20.3 — items 44, 45 (HTTP-client trait, source/user_agent).
  - §20.4 — items 6, 8, 9 (configuration extensibility).
  - §20.5 — items 7, 10, 11, 13, 14, 15, 16, 17, 18, 19, 20 (TUI vi/mouse).
  - §20.6 — item 12 (TOML syntax highlighting).
  - §20.7 — items 25, 27, 28 (logging operations).
  - §20.8 — items 23, 24 (Windows / capability drop).
  - §20.9 — items 39, 40, 41, 42, 43 (follower enhancements).
  - §20.10 — item 46 (`start_*` consolidation).
  - §20.11 — items 56, 57, 61 (relay-protocol cosmetic).
  - §20.12 — items 62, 64, 68 (auth/session resilience).
  - §20.13 — item 67 (mid-ride course transitions).
  - §20.14 — items 47, 51, 53, 63 (partial-implementation completion).
  - §20.15 — item 66 (state-refresher cadence).
