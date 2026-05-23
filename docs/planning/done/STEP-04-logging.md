# Step 04 — Structured logging & verbose/debug flags

## Goal

Wire `tracing` and `tracing-subscriber` so every later step has a
uniform way to emit diagnostics, and make `-v` and `-D` change
behaviour.

## Behaviour

Defaults differ by sink so backgrounded daemons always record their
lifecycle events to the configured logfile, independent of any
verbosity flag; operators rely on the logfile as the post-mortem
record of when the daemon ran.

- `--verbose` → `info` on ranchero crates, `warn` on dependencies.
- `--debug`   → `debug` on ranchero crates, `info` on dependencies;
  also keeps the process in foreground (already wired in STEP 01).
- Foreground, no flags → `warn` everywhere (clean stderr).
- Backgrounded, no flags → `info` on ranchero crates, `warn` on
  dependencies (so that `started` and `stopped` always reach the
  log file).
- `RUST_LOG` always takes precedence if set (passed directly to
  `EnvFilter`).
- Logging sink: stderr when foreground, `logging.file` when backgrounded.
- Rolling file appender: deferred (see Deferred section).

## Design

A small set of pure functions sits at the testable core; a thin
`install()` wires them into a global `tracing_subscriber` registry and
returns a guard so the non-blocking appender flushes on drop.

```rust
pub struct LogOpts { pub verbose: bool, pub debug: bool }

pub enum LogSink { Stderr, File(PathBuf) }

pub fn filter_directive(opts: LogOpts, foreground: bool, rust_log: Option<&str>) -> String;
pub fn select_sink(foreground: bool, log_file: &Path) -> LogSink;
pub fn open_log_for_append(path: &Path) -> io::Result<File>;
```

`install()` builds the EnvFilter from `filter_directive`, picks a writer
according to `select_sink`, wraps the file writer (when used) in
`tracing_appender::rolling::Builder` for size-based rotation, and
returns a `Guard` that flushes the non-blocking appender on drop.

## Emission contract

The daemon emits these tracing events as part of STEP 04 so the
integration tests have a stable surface to search against. Existing
user-facing `println!` lines on stdout are preserved.

| Level   | Target / message                          | Site                                                          |
| :------ | :---------------------------------------- | :------------------------------------------------------------ |
| `info`  | `"ranchero started"` (`pid` field)        | `daemon::runtime::start`, after pidfile is written            |
| `info`  | `"ranchero stopped"`                      | `daemon::runtime::start`, after the event loop exits          |
| `debug` | `"control request received"` (`req` field)| `daemon::runtime::handle_unix_connection`, on each request    |

Later steps (07 and later) introduce their own per-domain events;
STEP 04 only owns the three lifecycle events above.

## Tests-first outline

Unit tests in `src/logging/mod.rs` exercise the three pure helpers:

1. `foreground_defaults_to_warn` — foreground + no flags + no env → `"warn"`.
2. `background_defaults_promote_ranchero_to_info` — backgrounded + no
   flags → directive must include `ranchero=info` so lifecycle events
   reach the logfile.
3. `subscriber_respects_verbose_flag` — verbose yields `ranchero=info`
   on a `warn` default for dependencies.
4. `subscriber_respects_debug_flag` — debug yields `ranchero=debug` plus
   `info` for dependencies.
5. `debug_overrides_verbose_when_both_set`.
6. `rust_log_env_wins_over_flags`, `rust_log_env_wins_for_background_too`,
   `rust_log_env_wins_with_complex_directive`.
7. `empty_rust_log_falls_back_to_flags`.
8. `select_sink_foreground_is_stderr`, `select_sink_background_is_logfile`.
9. `logfile_is_opened_for_append_when_backgrounded` — second open of the
   same path appends rather than truncates, and missing parent
   directories are created.
10. `open_log_for_append_creates_missing_parent_directories`.

Integration tests in `tests/logging.rs` spawn the binary and assert on
the live subscriber against the emission contract above:

- `verbose_flag_emits_startup_info_to_stderr` — `-v --foreground start`
  → stderr contains the `started` and `stopped` info events.
- `default_silences_info_on_stderr` — no flags plus foreground → stderr
  carries no `INFO` lines during a clean lifecycle.
- `debug_flag_emits_control_debug_to_stderr` — `-D start` followed by
  `ranchero status` → stderr contains a `DEBUG` line for the control
  request.
- `rust_log_env_overrides_default_filter` — `RUST_LOG=ranchero=info`
  with no flags → the `started` event reaches stderr.
- `backgrounded_daemon_writes_lifecycle_to_logfile_without_flags` —
  `ranchero start` with no flags (no `-v`, no `-D`) → the configured
  logfile contains `started` and `stopped`. This is a regression test
  for the empty-logfile bug.
- `logfile_is_appended_across_two_runs` — two no-flag start and stop
  cycles → both `started` events are present in the logfile.

Tests are written first (TDD); the helpers are added as `todo!()`
stubs and the emission points are not yet wired, so the suite fails
red until the STEP 04 implementation is complete.

## Implementation outline (deferred until tests are able to pass)

- New module `src/logging/mod.rs` with the three pure helpers and the
  `install()` wrapper.
- Add `tracing`, `tracing-subscriber` (with `env-filter`) and
  `tracing-appender` to `Cargo.toml` once the implementation begins.
- `cli::dispatch` calls `logging::install` after resolving the
  configuration but before entering the subcommand body, threading the
  resolved `log_file` and the foreground flag through.

## Acceptance criteria

- All unit tests above pass.
- Running `ranchero -v start --foreground` emits `info`-level events
  from ranchero modules to stderr; `-D` adds `debug`; neither flag
  yields silent stderr by default.
- Backgrounded daemon writes to `logging.file`, with a previous run's
  log preserved (append, not truncate).
- `RUST_LOG=trace ranchero start` overrides flag-based defaults.

## Deferred

- **Log rotation**: the implementation uses a plain append-mode file
  via `tracing_appender::non_blocking`. A long-running daemon will grow
  a single logfile without bound. Add `tracing-appender::rolling`
  (daily or size-based) in a follow-up.
- Per-module level overrides (`zwift_relay=trace,zwift_api=debug`) are
  added with the workspace split in STEPS 06–08; STEP 04 only targets
  the `ranchero` crate root.
- JSON or structured log output for downstream tooling.
- Log shipping to external collectors.
