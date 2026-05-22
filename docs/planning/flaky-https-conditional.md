# Flaky test: https_conditional under parallel load

`tests/https_conditional.rs` (`https_bound_when_certs_present` /
`https_not_bound_when_certs_absent`) intermittently fails when the full
suite runs in parallel with `cargo test -- --include-ignored`. Observed on
2026-05-22: the suite reported `1 passed; 1 failed` for this test binary,
but both tests pass cleanly when the binary is run in isolation
(`cargo test --test https_conditional -- --include-ignored` → 2 passed).

## Symptom

One of the two tests fails under load; in isolation both pass. No panic
message was captured from the parallel run (only the suite-level
`test result: FAILED. 1 passed; 1 failed`).

## Root cause

The HTTPS port is derived, not OS-assigned. Each test asks the OS for a
free HTTP port by binding port 0, then computes the HTTPS port as
`http_port + 1` (`tests/https_conditional.rs:77` and `:103`). Port 0 only
guarantees the *HTTP* port is free — `http_port + 1` may already be bound
by another test or process running concurrently. Under the full parallel
suite this collision becomes likely, so the HTTPS listener either fails to
bind or is not yet listening at the instant `port_is_listening(https_port)`
is probed.

This is the same class of load-sensitive flakiness as
`flaky-daemon-lifecycle-shutdown.md`: correct in isolation, racy under
parallel pressure.

## Proposed fix

The robust fix is to stop deriving the HTTPS port from the HTTP port.
Options, lowest-cost first:

1. **Bind-and-retry.** If the HTTPS bind on `http_port + 1` fails because
   the port is taken, retry the whole `start()` with a fresh OS-assigned
   HTTP port. This keeps the `+1` convention but tolerates collisions.
2. **Readiness wait.** Replace the single `port_is_listening(https_port)`
   probe with a short retry loop (e.g. up to 1 s) so a listener that is
   merely slow to come up under load is not reported as absent.
3. **Serialize the HTTPS tests** with `serial_test` so they do not compete
   for adjacent ports during the run (adds a dev-dependency).

Note: not caused by the STEP-18 work — the STEP-18 changes touch only
`src/web/subs`, `src/web/ws`, `src/web/mod.rs`, and `src/web/format.rs`,
none of which is on the TLS/port-binding path.
