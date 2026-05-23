# Flaky test: https_conditional under parallel load — RESOLVED

`tests/https_conditional.rs` (`https_bound_when_certs_present` /
`https_not_bound_when_certs_absent`) intermittently failed when the full
suite ran in parallel with `cargo test -- --include-ignored`. Observed on
2026-05-22.

## Root cause

The HTTPS port was derived as `http_port + 1`, but only the HTTP port is
guaranteed free when binding port 0.  Under parallel load, `http_port + 1`
could already be bound by another test or process, causing the HTTPS bind to
fail and `start()` to return an error before the test could even probe the
port.

## Fix (applied 2026-05-23)

`src/web/server.rs` — when `cfg.server_port == 0` (test use), bind the HTTPS
listener on port 0 too so the OS assigns an independent free port.  For
non-zero `server_port` (production), the `server_port + 1` convention is
unchanged.

`WebServerHandle` gained an `https_addr() -> Option<SocketAddr>` method that
returns the actual bound HTTPS address, and the test was updated to use it
instead of computing `http_port + 1`.  The "certs absent" test now asserts
`handle.https_addr().is_none()` rather than probing a derived port number.

`cargo test --test https_conditional -- --include-ignored` passes cleanly
under full parallel load.
