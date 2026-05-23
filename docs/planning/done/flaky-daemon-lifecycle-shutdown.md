# Flaky test: stop_clears_pid_file_and_status_reports_shutdown — RESOLVED

`tests/daemon_lifecycle.rs::stop_clears_pid_file_and_status_reports_shutdown`
intermittently fails when `cargo test` runs the full suite in parallel.

## Symptom

```
thread 'stop_clears_pid_file_and_status_reports_shutdown' panicked at tests/daemon_lifecycle.rs:321:5:
pidfile should be removed after stop
```

`wait_for_pidfile_gone()` times out after `SHUTDOWN_TIMEOUT = 5 s`. The test
passes in isolation (passes in ~5 s when run alone), so the daemon does shut
down correctly; it just takes longer when the machine is under full suite load.

## Root cause

`SHUTDOWN_TIMEOUT` (5 s) is too short for a loaded CI machine. Under parallel
test pressure the daemon's OS process scheduling is delayed, causing the pidfile
removal to exceed 5 s.

## Proposed fix

Increase `SHUTDOWN_TIMEOUT` from 5 s to 15 s in `tests/daemon_lifecycle.rs`
(matching `READY_TIMEOUT`, which is already 15 s). This preserves the guard
against a hung shutdown while giving the daemon enough time to exit cleanly
under load.

Alternatively, serialize the four "start/stop a real daemon" tests using the
`serial_test` crate so they don't compete for process scheduling during the run.
That requires a new dev-dependency, so the timeout increase is the lower-cost
option.

## Fix (applied 2026-05-23)

Increased `SHUTDOWN_TIMEOUT` from 5 s to 15 s in `tests/daemon_lifecycle.rs`
(line 13), matching `READY_TIMEOUT` which was already 15 s. This preserves the
guard against a hung shutdown while giving the daemon sufficient time to exit
cleanly under full suite parallel load.

The flaky test now passes consistently under full parallel suite runs.
