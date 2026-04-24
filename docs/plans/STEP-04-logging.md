# Step 04 — Structured logging & verbose/debug flags (stub)

## Goal

Wire `tracing` + `tracing-subscriber` so every later step has a uniform
way to emit diagnostics, and make `-v` / `-D` actually change behaviour.

## Sketch

- `--verbose` → `info` level on ranchero crates.
- `--debug`   → `debug` level on ranchero crates, `info` on deps; also
  keeps process in foreground (already wired in STEP 01).
- Neither flag → `warn`.
- `RUST_LOG` always wins if set.
- Logging sink: stderr when foreground, `logging.file` when backgrounded.
- Rolling file appender (`tracing-appender`) with size-based rotation.

## Tests-first outline

- `subscriber_respects_verbose_flag`, `subscriber_respects_debug_flag`,
  `rust_log_env_wins_over_flags`, `logfile_is_opened_for_append_when_backgrounded`.

To be fully elaborated when we start work on this step.
