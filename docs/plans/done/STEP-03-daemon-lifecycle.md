# Step 03 — Daemon lifecycle (`start` / `stop` / `status`)

## Goal

Turn the three stubbed subcommands into real process-management commands:

- `ranchero start` launches the long-running daemon. Defaults to
  background; stays in foreground if `--foreground` or `--debug` is set.
- `ranchero stop` signals the running daemon to shut down cleanly (or
  reports that nothing is running).
- `ranchero status` prints structured stats about the running daemon, or
  reports shutdown state, via a small local IPC channel.

Nothing in this step yet connects to Zwift. The daemon at this point is
a placeholder event loop that publishes counters we can interrogate; the
real relay/stats plumbing arrives in later steps (07+).

## Design sketch

- **PID file** — `daemon.pidfile` from STEP 02's config. Written on
  daemon start, deleted on clean shutdown. Stale PID files detected via
  `kill -0` (Unix) / `OpenProcess` (Windows) and reported distinctly
  from "no daemon running."
- **Backgrounding** — on Unix, use a small double-fork (via the
  `daemonize` crate, or hand-rolled; hand-rolled keeps dependency
  surface low). On Windows, keep `--foreground` as the only supported
  mode for STEP 03 and log a clear error if the user omits it.
- **Control socket** — a Unix domain socket at
  `~/.local/state/ranchero/ranchero.sock` (or a TCP loopback on
  Windows). Protocol is length-prefixed JSON request/response:
  `{ "cmd": "status" }` → `{ "uptime_ms": …, "state": "running", … }`;
  `{ "cmd": "shutdown" }` → `{ "ok": true }`.
- **Shutdown** — control-socket shutdown preferred; SIGTERM honored as a
  fallback on Unix. Daemon traps SIGINT/SIGTERM and exits cleanly,
  removing PID file and socket.

## Tests first

Unit tests (pure):

1. `pid_file_encoder_writes_pid_and_newline`.
2. `pid_file_reader_returns_pid_or_none`.
3. `pid_alive_check_unix_stubbed` — trait `ProcessProbe` with an
   in-memory impl; asserts the lifecycle module consults it rather than
   calling `kill(2)` directly, so the logic is testable.
4. `control_request_status_serializes_round_trip`.
5. `control_response_is_human_printable` — the formatter that turns a
   status response into the user-facing text for `ranchero status`.

Integration tests (spawning the binary, in `tests/daemon_lifecycle.rs`):

6. `start_writes_pid_file_and_status_reports_running` — spawn
   `ranchero start --foreground &`, wait for readiness file, run
   `ranchero status`, expect "running (uptime …ms)".
7. `stop_clears_pid_file_and_status_reports_shutdown` — after #6, run
   `ranchero stop`, then `ranchero status` → "not running".
8. `stop_when_not_running_reports_no_daemon` — fresh state; `ranchero
   stop` exits non-zero with a clear message, no stack trace.
9. `status_when_not_running_reports_no_daemon`.
10. `start_when_already_running_refuses` — second `start` detects the
    live PID and exits non-zero; first daemon unaffected.
11. `stale_pid_file_is_cleaned_up_on_start` — plant a PID file holding a
    PID whose probe reports "not alive"; `start` removes the stale file
    and continues.
12. `debug_flag_keeps_process_in_foreground` — `start -D` does not fork;
    a pipe-redirect on stdout captures daemon output directly.

These integration tests require a writable HOME/XDG directory and a
unique `--config` fixture per test so they can run in parallel.

## Implementation outline

- New module `src/daemon/mod.rs`:
  - `Pidfile` (write/read/remove with atomic rename on write).
  - `ProcessProbe` trait + `OsProcessProbe` impl.
  - `ControlSocket` — server side + client side, sharing the JSON
    request/response enum.
  - `Daemon::run()` — the actual event loop. For STEP 03 it's a
    `tokio::select!` over `ctrl_c`, SIGTERM (via `tokio::signal::unix`),
    and control-socket connections. On shutdown, drops the PID file
    and socket.
- `src/cli.rs` dispatch:
  - `Command::Start` → `daemon::start(resolved_config, foreground)`.
  - `Command::Stop`  → `daemon::stop(resolved_config)`.
  - `Command::Status`→ `daemon::status(resolved_config)`.
- Platform split: `cfg(unix)` for double-fork + UDS; `cfg(windows)` for
  loopback TCP + an early-exit error when backgrounding is requested.
- Crates added: `tokio` (rt-multi-thread + net + signal + macros),
  `serde_json`, `nix` (just for `kill(0)`) or implement via a small
  syscall helper.

## Acceptance criteria

- All tests above pass, including parallel execution.
- `ranchero start` in foreground (`-D` or `--foreground`) prints a
  "started" line and blocks; Ctrl-C produces a clean "stopped" line and
  exit 0.
- `ranchero start` backgrounded on Unix returns control to the shell
  within ~100 ms; `ranchero status` reports "running"; `ranchero stop`
  reports "stopped" and shell regains the PID-less state.
- On Windows, omitting `--foreground` exits non-zero with a message
  pointing the user at `--foreground`.

## Deferred

- Log rotation / stderr-stdout redirection for the backgrounded daemon
  → STEP 04.
- Real status counters (connected relay, athletes seen, packets/sec) →
  STEP 12 onward; for STEP 03 the daemon only reports uptime + pid +
  state.
- Windows service integration.
- Privileged capabilities drop.
