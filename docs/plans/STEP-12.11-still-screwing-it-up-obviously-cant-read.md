# Step 12.11 — Production daemon wires the wrong constructor; UDP factory still a stub

**Status:** investigation (2026-05-02).

After STEP-12.10 closed the TCP-port and athlete-identity gaps, the
operator workflow

```
ranchero start --debug --capture output.cap
```

was exercised again with real monitor credentials. The login pipeline now
runs cleanly and TCP connects to `:3025` as expected, but the connection
is dropped by the server roughly ten seconds after `relay.tcp.established`
and nothing happens between those two log lines:

```
ranchero start --debug --capture output.cap
ranchero started (pid 66337)
2026-05-02T09:27:01.163010Z  INFO ranchero::daemon::runtime: ranchero started pid=66337
2026-05-02T09:27:01.166596Z  INFO ranchero::relay: relay.capture.opened
2026-05-02T09:27:03.953065Z  INFO ranchero::relay: relay.login.ok email="doug+sauce@mhost.com" athlete_id=5213306
2026-05-02T09:27:05.297735Z  INFO ranchero::relay: relay.tcp.connecting addr=16.146.39.255:3025
2026-05-02T09:27:05.503111Z  INFO ranchero::relay: relay.tcp.established addr=16.146.39.255:3025
2026-05-02T09:27:15.703624Z  WARN ranchero::relay: relay.tcp.recv_error error=TCP peer closed
```

There is no `relay.tcp.hello.sent`, no `relay.udp.established`, no
`relay.heartbeat.started`. Ten seconds is the documented Zwift idle-drop
window; the server is closing a connection that never authenticated
itself with a hello packet.

## Diagnosis

Two independent defects combine to produce this trace.

### Defect A — `start_with_writer` dispatches to the wrong inner

`src/daemon/runtime.rs:245` calls
`RelayRuntime::start_with_writer(&cfg, writer)`. That entry point
(`src/daemon/relay.rs:690-732`) constructs `DefaultAuthLogin`,
`DefaultSessionLogin`, `DefaultTcpTransportFactory`, then calls
`Self::start_inner(...)`.

`start_inner` (around `src/daemon/relay.rs:1200-1354`) performs only:

1. credential validation
2. `auth.login` + `auth.athlete_id`
3. `session_factory.login` (single-shot, no supervisor)
4. TCP connect
5. wait for `TcpChannelEvent::Established`
6. spawn the recv loop and return

It does **not** send a TCP hello, does **not** open a UDP socket, does
**not** start the 1 Hz heartbeat scheduler, and does **not** subscribe to
session-supervisor events.

The complete pipeline already exists in
`src/daemon/relay.rs:start_all_inner` (around lines 925-1105). It runs
all of the steps above plus:

- step 8: send the TCP hello (`ClientToServer { server_realm: 1,
  player_id: athlete_id, world_time: Some(0), seqno: Some(1), ... }`)
- step 9: connect UDP and call `UdpChannel::establish`
- step 10: spawn the heartbeat scheduler
- step 11: subscribe to supervisor events

`start_all_inner` is reachable only via `start_with_all_deps` /
`start_with_all_deps_and_writer`, both of which are called solely by the
integration tests. The production daemon never touches it.

### Defect B — `DefaultUdpTransportFactory::connect` is a stub

`src/daemon/relay.rs:1644-1659`:

```rust
pub struct DefaultUdpTransportFactory;

impl UdpTransportFactory for DefaultUdpTransportFactory {
    type Transport = zwift_relay::TokioUdpTransport;

    fn connect(
        &self,
        _addr: std::net::SocketAddr,
    ) -> impl std::future::Future<Output = std::io::Result<Self::Transport>> + Send {
        async move {
            Err(std::io::Error::other(
                "Defect 4: UDP connection not yet implemented",
            ))
        }
    }
}
```

Even after Defect A is corrected, step 9 of `start_all_inner` will
immediately fail with this error. The real implementation is one line:
`zwift_relay::TokioUdpTransport::connect(addr).await`, which already
exists at `crates/zwift-relay/src/udp.rs:45`.

## Implementation plan

Two items, both small. Defect B must land before Defect A is observable
end-to-end (otherwise correcting the dispatch only moves the failure to
a UDP-connect error). Order them B → A.

Each follows the project's red-then-green TDD discipline.

### Order

1. **Item 1** — Replace `DefaultUdpTransportFactory::connect` with a
   real `TokioUdpTransport::connect` call.
2. **Item 2** — Route the production daemon through `start_all_inner`,
   wiring `DefaultSessionSupervisorFactory` and
   `DefaultUdpTransportFactory` alongside the existing auth and TCP
   factories.

### Item 1 — Real `DefaultUdpTransportFactory`

#### Pinned decisions

- `DefaultUdpTransportFactory::connect(addr)` calls
  `zwift_relay::TokioUdpTransport::connect(addr).await` and returns the
  `std::io::Result` unchanged.
- The struct stays a unit struct; no configuration fields are needed
  today.
- The "Defect 4" doc comment block above the impl is removed; replace
  it with a one-line description noting that this is the production
  factory used by `start_all_inner`.

#### Files to touch

- `src/daemon/relay.rs:1640-1659` — replace stub body, update doc
  comment.

#### Red-state tests

Add to `tests/relay_runtime.rs` (or a small new
`tests/udp_factory.rs` if you prefer keeping the surface focused):

- **T1-A** — `default_udp_transport_factory_connects_to_bound_socket`.
  Bind a `tokio::net::UdpSocket` to `127.0.0.1:0`, read its local
  address, then call `DefaultUdpTransportFactory.connect(addr).await`
  and assert `is_ok()`. With the stub in place this fails with the
  "not yet implemented" error.

#### Green-state implementation

- **G1-1** — Edit `DefaultUdpTransportFactory::connect` to delegate
  to `TokioUdpTransport::connect(addr).await`.
- **G1-2** — Run `cargo test -p ranchero --test relay_runtime
  default_udp_transport_factory_connects_to_bound_socket` (or the
  equivalent invocation if you placed the test elsewhere) and confirm
  it passes.

### Item 2 — Production daemon uses the full pipeline

#### Pinned decisions

- `RelayRuntime::start_with_writer` is rewritten to call
  `Self::start_all_inner` directly, supplying production defaults for
  every dependency:
  - `auth`: `DefaultAuthLogin::new(auth.clone())`
  - `sf`: `DefaultSessionSupervisorFactory::new(auth.clone(),
    session_config)`
  - `tcp_factory`: `DefaultTcpTransportFactory`
  - `udp_factory`: `DefaultUdpTransportFactory`
- The capture-writer error-path block in `start_with_writer` (the
  `match` that flushes and emits `relay.capture.closed` on failure) is
  preserved. `start_all_inner` already accepts an optional pre-opened
  writer via its `preopen_writer` parameter, so the existing
  flush-on-error contract is reproduced by passing the writer through
  and keeping the surrounding `match` arms.
- `RelayRuntime::start` (the path-based entry) is rewritten the same
  way, so both production entry points converge on `start_all_inner`.
- `start_inner` and the `start_with_deps*` family stay in place for
  now — the existing integration tests in `crates/zwift-relay/tests`
  and `tests/relay_runtime.rs` depend on them. A follow-up step can
  retire the duplicated path once the integration tests have been
  re-pointed at `start_with_all_deps*`.

#### Files to touch

- `src/daemon/relay.rs:663-732` — rewrite both `start` and
  `start_with_writer` bodies to construct the four production
  factories and dispatch into `start_all_inner`.
- No changes to `src/daemon/runtime.rs:245`; the call site keeps the
  same signature.

#### Red-state tests

The cheapest observable assertion is at the daemon log surface: a
production-style start must emit the full event sequence. Add to
`tests/relay_runtime.rs`:

- **T2-A** — `start_with_writer_emits_full_lifecycle_event_sequence`.
  Build a `ResolvedConfig` with credentials, mount wiremock for the
  auth + session endpoints, bind a local TCP listener that accepts
  the connection and reads the hello frame, bind a local UDP socket
  for the UDP establish path, then call
  `RelayRuntime::start_with_writer(&cfg, None).await` and assert the
  tracing log contains `relay.tcp.hello.sent`,
  `relay.udp.established`, and `relay.heartbeat.started`. With Defect
  A in place the call returns `Ok` but none of those records appear
  (the recv loop sits on the idle TCP socket).

  Note: this test requires routing the production endpoints through
  the wiremock URL. If `ResolvedConfig` does not yet expose enough
  knobs to do that without monkey-patching, fall back to a narrower
  assertion: spy on which inner function `start_with_writer`
  dispatches to. The simplest form is a compile-time assertion —
  delete `start_inner` after Item 2 lands and let the build fail if
  anything still references it. Track that deletion as a follow-up;
  do not block Item 2 on it.

#### Green-state implementation

- **G2-1** — Rewrite `start_with_writer` to construct the four
  production factories and call `Self::start_all_inner(cfg, None,
  capture_writer, auth_login, supervisor_factory, tcp_factory,
  udp_factory, game_events_tx)`. Preserve the existing
  capture-flush-on-error `match` arms.
- **G2-2** — Apply the same rewrite to `RelayRuntime::start` (the
  path-based entry).
- **G2-3** — Run the full workspace test suite and confirm all
  existing tests still pass. The integration tests that already cover
  `start_all_inner` should keep passing unchanged.
- **G2-4** — Re-run T2-A and confirm it passes.
- **G2-5** — Run `ranchero start --debug --capture output.cap`
  against live Zwift and confirm the trace now includes
  `relay.tcp.hello.sent`, `relay.udp.established`, and
  `relay.heartbeat.started`, and that the connection survives past
  the previous ten-second drop window.

## Acceptance

- The trace from `ranchero start --debug --capture output.cap` shows
  the full lifecycle (`relay.capture.opened` → `relay.login.ok` →
  `relay.tcp.connecting` → `relay.tcp.established` →
  `relay.tcp.hello.sent` → `relay.udp.established` →
  `relay.heartbeat.started`) and does not drop after ten seconds.
- `cargo test` is clean across the workspace.
- T1-A and T2-A both pass.

## Out of scope

- Retiring `start_inner` and the `start_with_deps*` family. Deferred
  until the integration tests in `crates/zwift-relay/tests` and
  `tests/relay_runtime.rs` are migrated to `start_with_all_deps*`.
- Sticky TCP server selection across reconnects (still tracked from
  STEP-12.10).
- Any change to the `DefaultSessionSupervisorFactory` red-state
  behaviour (it currently delegates to the single-shot
  `zwift_relay::login`). The real supervisor lands with Defect 7
  green state, which is its own future step.
