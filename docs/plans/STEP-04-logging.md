# Step 04 — Structured logging & verbose/debug flags

## Goal

Wire `tracing` + `tracing-subscriber` so every later step has a uniform
way to emit diagnostics, and make `-v` / `-D` actually change behaviour.

## Behaviour

- `--verbose` → `info` level on ranchero crates, `warn` on deps.
- `--debug`   → `debug` level on ranchero crates, `info` on deps; also
  keeps process in foreground (already wired in STEP 01).
- Neither flag → `warn` everywhere.
- `RUST_LOG` always wins if set (handed straight to `EnvFilter`).
- Logging sink: stderr when foreground, `logging.file` when backgrounded.
- Rolling file appender (`tracing-appender`) with size-based rotation.

## Design

A small set of pure functions sits at the testable core; a thin
`install()` wires them into a global `tracing_subscriber` registry and
returns a guard so the non-blocking appender flushes on drop.

```rust
pub struct LogOpts { pub verbose: bool, pub debug: bool }

pub enum LogSink { Stderr, File(PathBuf) }

pub fn filter_directive(opts: LogOpts, rust_log: Option<&str>) -> String;
pub fn select_sink(foreground: bool, log_file: &Path) -> LogSink;
pub fn open_log_for_append(path: &Path) -> io::Result<File>;
```

`install()` builds the EnvFilter from `filter_directive`, picks a writer
according to `select_sink`, wraps the file writer (when used) in
`tracing_appender::rolling::Builder` for size-based rotation, and
returns a `Guard` that flushes the non-blocking appender on drop.

## Emission contract

The daemon ships these tracing events as part of STEP 04 so the
integration tests have a stable surface to grep against. Existing
user-facing `println!` lines on stdout are preserved.

| Level   | Target / message                          | Site                                                          |
| :------ | :---------------------------------------- | :------------------------------------------------------------ |
| `info`  | `"ranchero started"` (`pid` field)        | `daemon::runtime::start`, after pidfile is written            |
| `info`  | `"ranchero stopped"`                      | `daemon::runtime::start`, after the event loop exits          |
| `debug` | `"control request received"` (`req` field)| `daemon::runtime::handle_unix_connection`, on each request    |

Later steps (07+) bring their own per-domain events; STEP 04 only owns
the lifecycle three above.

## Tests-first outline

Unit tests in `src/logging/mod.rs` exercise the three pure helpers:

1. `subscriber_respects_verbose_flag` — verbose alone yields a directive
   that promotes `ranchero` to `info` while leaving the default at
   `warn`.
2. `subscriber_respects_debug_flag` — debug yields `ranchero=debug` plus
   an `info` default for dep crates.
3. `rust_log_env_wins_over_flags` — a non-empty `RUST_LOG` is returned
   verbatim, even when both flags are set.
4. `defaults_to_warn` — neither flag, no env → `"warn"`.
5. `select_sink_foreground_is_stderr`, `select_sink_background_is_logfile`.
6. `logfile_is_opened_for_append_when_backgrounded` — second open of the
   same path appends rather than truncates, and missing parent
   directories are created.

Integration tests in `tests/logging.rs` spawn the binary and assert on
the live subscriber against the emission contract above:

- `verbose_flag_emits_startup_info_to_stderr` — `-v --foreground start`
  → stderr contains the `started` and `stopped` info events.
- `default_silences_info_on_stderr` — no flags → stderr carries no
  `INFO` lines during a clean lifecycle.
- `debug_flag_emits_control_debug_to_stderr` — `-D start` followed by a
  `ranchero status` → stderr contains a `DEBUG` line for the control
  request.
- `rust_log_env_overrides_default_filter` — `RUST_LOG=ranchero=info`
  with no flags → `started` event reaches stderr.
- `backgrounded_daemon_writes_lifecycle_to_logfile` — `-v start`
  (backgrounded) → configured logfile contains `started` and `stopped`.
- `logfile_is_appended_across_two_runs` — two start/stop cycles → both
  `started` events present in the logfile.

Tests are written first (TDD); the helpers ship as `todo!()` stubs and
the emission points are not yet wired, so the suite fails red until
STEP 04 implementation lands.

## Implementation outline (deferred until tests are green-able)

- New module `src/logging/mod.rs` with the three pure helpers and the
  `install()` wrapper.
- Add `tracing`, `tracing-subscriber` (with `env-filter`) and
  `tracing-appender` to `Cargo.toml` once the implementation begins.
- `cli::dispatch` calls `logging::install` after resolving config but
  before entering the subcommand body, threading the resolved `log_file`
  and the foreground bit through.

## Acceptance criteria

- All unit tests above pass.
- Running `ranchero -v start --foreground` emits `info`-level events
  from ranchero modules to stderr; `-D` adds `debug`; neither flag
  yields silent stderr by default.
- Backgrounded daemon writes to `logging.file`, with a previous run's
  log preserved (append, not truncate).
- `RUST_LOG=trace ranchero start` overrides flag-based defaults.

## Deferred

- Per-module level overrides (`zwift_relay=trace,zwift_api=debug`) ship
  with the workspace split in STEPS 06–08; STEP 04 only targets the
  `ranchero` crate root.
- JSON / structured log output for downstream tooling.
- Log shipping to external collectors.
