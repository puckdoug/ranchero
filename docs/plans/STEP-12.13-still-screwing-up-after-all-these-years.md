# STEP-12.13 — Capture-and-UDP defects exposed by the first live `--debug --capture` run

**Status:** investigation (2026-05-03).

The first end-to-end run after STEP-12.12 (`ranchero start --debug
--capture output.cap`) confirmed the per-event tracing contract
holds — every event from the design appeared, in order, with the
right fields. The same trace surfaced three defects the test suite
did not catch. All three are in scope for STEP-12.13.

The reference trace lives in the conversation log (2026-05-03
`relay.start.failed error=UDP channel: I/O: Connection refused (os
error 61)` run, athlete `5213306`, capture file `output.cap`).

## Summary checklist

Each defect follows the STEP-12.12 pattern: an `Na` test pair
(failing assertions, written first) and an `Nb` implementation pair
that makes them pass.

- [x] **1a** — Tests for D1: `relay.capture.writer.closed` fires
  exactly once per shutdown; the bare `relay.capture.closed` event
  no longer appears in the daemon log.
- [x] **1b** — Implementation for D1: delete the obsolete
  `relay.capture.closed` emission from `recv_loop`'s shutdown
  branch.
- [x] **2a** — Tests for D2: `start_with_all_deps_and_writer` with
  a recording UDP factory + writer produces at least one outbound
  UDP capture record in the file.
- [x] **2b** — Implementation for D2: thread `capture_writer.clone()`
  into the `UdpChannelConfig` literal in `start_all_inner`.
- [ ] **3a** — Tests for D3: the daemon waits for the first
  `udp_config` push on the TCP `ServerToClient` stream before
  bringing UDP up; the chosen UDP target comes from that push, not
  from `session.tcp_servers[0]`; the pool-router selects the UDP
  target for the watched athlete.
- [ ] **3b** — Implementation for D3: introduce a small "wait for
  UDP server" step in `start_all_inner` that subscribes to the
  TCP stream, applies the first `UdpConfigVod` (or `UdpConfig`) to
  the pool router, and resolves the UDP target from there. Replace
  the `&session.tcp_servers[0]` read.

## Defect 1 — Duplicate capture-close events on shutdown

### Symptom

```
INFO ranchero::relay: relay.capture.writer.closed total_records=2 total_bytes=118
INFO ranchero::relay: relay.capture.closed dropped_count=0
```

Both fire on every clean shutdown. The first is the rollup emitted
by the writer task in `crates/zwift-relay/src/capture.rs`
(STEP-12.12 §3b). The second is the older daemon-side log line in
`src/daemon/relay.rs::recv_loop`'s shutdown branch (around line
1715), retained from a pre-3b implementation that needed
`dropped_count()` to surface the drop total.

### Why it is a defect

`relay.capture.writer.closed` carries strictly more information
(`total_records` and `total_bytes` describe the file's actual
contents). `relay.capture.closed` is now noise on the `--debug`
channel and an inconsistency in the namespace
(`writer.closed` vs `closed`).

### 1a — Test (red state)

Add to the daemon's inline test module (the same module that hosts
`udp_channel_subscriber_does_not_double_log_inbound`, around
`src/daemon/relay.rs:2580`):

```rust
#[tokio::test]
#[tracing_test::traced_test]
async fn shutdown_emits_writer_closed_exactly_once_and_no_legacy_closed() {
    // Bring up a runtime with a capture writer attached, drive a
    // graceful shutdown, then count the close-rollup events.
    // Pre-fix the log carries both `relay.capture.writer.closed`
    // and `relay.capture.closed`; post-fix only the former remains.
    let cfg = make_config(Some("rider@example.com"), Some("secret"));
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    let runtime = RelayRuntime::start_with_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth::ok(CallCounter::new()),
        StubSession::ok(CallCounter::new(), fixture_session(fixture_servers())),
        StubTcpFactory::ok(CallCounter::new()),
    )
    .await
    .expect("start");
    runtime.shutdown();
    let _ = runtime.join().await;

    let writer_closed = count_log_substr("relay.capture.writer.closed");
    assert_eq!(
        writer_closed, 1,
        "STEP-12.13 D1: relay.capture.writer.closed must fire exactly \
         once per shutdown; got {writer_closed}",
    );
    assert!(
        !logs_contain("relay.capture.closed dropped_count="),
        "STEP-12.13 D1: the legacy `relay.capture.closed` log line was \
         deliberately removed; recv_loop's shutdown branch must not \
         re-emit it",
    );
}
```

`count_log_substr` is a small new helper that reads the
`tracing-test` capture and returns the number of lines containing
the substring. If `tracing-test`'s public API doesn't expose this
directly, implement it inline by splitting
`tracing_test::internal::all_logs()` (or its closest equivalent) on
newlines and counting matches.

### 1b — Implementation (green state)

In `src/daemon/relay.rs`, edit `recv_loop`'s `_ = shutdown.notified()`
branch (around line 1707-1717). Delete the `dropped_count` read and
the `relay.capture.closed` log line; keep the `flush_and_close`
call so the writer task drains and emits its own
`relay.capture.writer.closed`.

Before:

```rust
_ = shutdown.notified() => {
    tracing::info!(target: "ranchero::relay", "relay.tcp.shutdown");
    channel.shutdown_and_wait().await;
    if let Some(writer) = capture_writer.as_ref() {
        let dropped_count = writer.dropped_count();
        if let Err(e) = writer.flush_and_close().await {
            tracing::warn!(target: "ranchero::relay", error = %e, "capture flush failed");
            return Err(RelayRuntimeError::CaptureIo(e));
        }
        tracing::info!(target: "ranchero::relay", dropped_count, "relay.capture.closed");
    }
    return Ok(());
}
```

After:

```rust
_ = shutdown.notified() => {
    tracing::info!(target: "ranchero::relay", "relay.tcp.shutdown");
    channel.shutdown_and_wait().await;
    if let Some(writer) = capture_writer.as_ref()
        && let Err(e) = writer.flush_and_close().await
    {
        tracing::warn!(target: "ranchero::relay", error = %e, "capture flush failed");
        return Err(RelayRuntimeError::CaptureIo(e));
    }
    return Ok(());
}
```

The writer task's own `relay.capture.writer.closed` rollup
(STEP-12.12 §3b) is the canonical close event. Drop counts now
surface in real time as `relay.capture.record.dropped` warns from
the producer-side path.

## Defect 2 — UDP capture is silently disabled in production

### Symptom

`output.cap` from the reference run contains exactly two records
(`total_records=2 total_bytes=118`) — one manifest record (66
bytes) and one outbound TCP hello frame record (52 bytes). The
trace shows **twenty** outbound UDP hellos firing
(`relay.udp.hello.sent hello_idx=1..20`); none appear in the file.

### Root cause

`src/daemon/relay.rs::start_all_inner` plumbs the writer into the
TCP channel directly:

```rust
let tcp_config = zwift_relay::TcpChannelConfig {
    athlete_id,
    conn_id: next_conn_id(),
    watchdog_timeout: zwift_relay::CHANNEL_TIMEOUT,
    capture: capture_writer.clone(),     // ← present
};
```

…but the UDP channel inherits `capture` from
`udp_factory.channel_config()`, which for
`DefaultUdpTransportFactory` returns the default
`UdpChannelConfig` (whose `capture` is `None`):

```rust
let udp_config = zwift_relay::UdpChannelConfig {
    athlete_id,
    conn_id: next_conn_id(),
    ..udp_factory.channel_config()        // ← capture: None
};
```

The four UDP-side `record_outbound` / `record_inbound` calls (Phase
2b's hello send / hello recv / steady-state send / steady-state
recv) execute with a `None` writer and silently no-op.

The `udp_channel_with_capture_records_outbound_hello` tests in
`crates/zwift-relay/tests/udp.rs` pass because those tests build
`UdpChannelConfig` themselves with `capture: Some(...)`. The
daemon's own integration tests never exercised the production
wiring with both a recording UDP factory **and** a capture writer.

### 2a — Tests (red state)

Add to `tests/relay_runtime.rs`. The existing
`RecordingUdpFactory` already gives us the recording transport;
the missing piece is asserting the file contents.

```rust
#[tokio::test]
async fn start_all_inner_writes_udp_outbound_to_capture_file() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let path = tempfile::NamedTempFile::new().expect("tempfile");
    let writer = zwift_relay::capture::CaptureWriter::open(path.path())
        .await
        .expect("open writer");
    let writer = Arc::new(writer);

    let (udp_factory, _connected, _udp_written) = RecordingUdpFactory::new();
    let runtime = RelayRuntime::start_with_all_deps_and_writer(
        &cfg,
        Arc::clone(&writer),
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        StubTcpFactory::new(),
        udp_factory,
    )
    .await
    .expect("start");
    // Give the UDP channel a moment to issue at least one hello
    // through the recording transport.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    runtime.shutdown();
    let _ = runtime.join().await;
    drop(writer);

    let mut reader = zwift_relay::capture::CaptureReader::open(path.path())
        .expect("reader");
    let mut udp_outbound = 0usize;
    while let Some(item) = reader.next_item() {
        if let Ok(zwift_relay::capture::CaptureItem::Frame(rec)) = item
            && rec.direction == zwift_relay::capture::Direction::Outbound
            && rec.transport == zwift_relay::capture::TransportKind::Udp
        {
            udp_outbound += 1;
        }
    }
    assert!(
        udp_outbound >= 1,
        "STEP-12.13 D2: start_all_inner must thread the capture writer \
         into UdpChannelConfig so at least one UDP outbound record \
         reaches the file (got {udp_outbound})",
    );
}
```

Optional second test: directly assert the resolved
`UdpChannelConfig.capture` is `Some(_)` whenever the runtime was
started with a writer. This requires either an internal accessor on
the runtime or routing the resolved config through a debug seam;
skip unless 2b's first test proves insufficient as a regression
guard.

### 2b — Implementation (green state)

`src/daemon/relay.rs::start_all_inner`, around line 1122:

```rust
let udp_config = zwift_relay::UdpChannelConfig {
    athlete_id,
    conn_id: next_conn_id(),
    capture: capture_writer.clone(),
    ..udp_factory.channel_config()
};
```

The TCP path already does this. UDP must too. Verify by re-running
`ranchero start --debug --capture output.cap` and confirming UDP
hello bytes show up in the resulting file.

## Defect 3 — UDP target taken from the TCP server pool

### Symptom

```
ERROR ranchero::relay: relay.start.failed error=UDP channel: I/O: Connection refused (os error 61)
```

20 outbound UDP hellos fire, none get acked, `Connection refused`
surfaces from the kernel (ICMP Port Unreachable bouncing off the
target host). `start_all_inner` shuts down cleanly. No
`relay.udp.established`, no heartbeat. The trace is informative
because STEP-12.12 §2 added the per-hello `relay.udp.hello.sent`
events; before that, the same failure was a silent timeout.

### Root cause

`src/daemon/relay.rs::start_all_inner` step 9:

```rust
let udp_server = &session.tcp_servers[0];
let udp_addr_str = format!("{}:{}", udp_server.ip, zwift_relay::UDP_PORT_SECURE);
```

The UDP target is the first **TCP** server. Zwift announces UDP
servers via the TCP `ServerToClient` stream after the TCP channel
is established, in three fields:

- `ServerToClient.udp_config: Option<UdpConfig>` — a flat list of
  `RelayAddress { lb_realm, lb_course, ip, port, ra_f5, ra_f6 }`.
- `ServerToClient.udp_config_vod_1: Option<UdpConfigVod>` — a list
  of `RelayAddressesVod { lb_realm, lb_course, relay_addresses,
  rav_f4 }` (the per-realm/course pool form).
- `ServerToClient.udp_config_vod_2: Option<UdpConfigVod>` — same
  shape as `udp_config_vod_1`.

The `LoginResponse` itself does **not** announce UDP servers; the
daemon must wait for the first TCP push that carries one of the
above before it knows where to send UDP. The existing daemon
shortcut — "use TCP servers as UDP targets" — works against
`zwift-offline` (which collocates services) and breaks against
production Zwift (which doesn't).

The `RuntimeInner.pool_router: Mutex<UdpPoolRouter>` and the
`apply_pool_update` test seam already exist; they were wired for
later pool-routing work but never connected to the live message
stream.

### 3a — Tests (red state)

These tests must compile but fail until 3b lands. Three are needed:

#### 3a.i — `udp_target_taken_from_first_udp_config_push_not_tcp_servers`

Build a TCP fixture that, immediately after `Established`, sends one
`ServerToClient` carrying a `UdpConfigVod` whose only
`RelayAddress` points at a recognisable IP that does **not** appear
in `tcp_servers`. Use the existing `RecordingUdpFactory` to capture
the address `connect()` was called with.

```rust
#[tokio::test]
async fn udp_target_taken_from_first_udp_config_push_not_tcp_servers() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let session = zwift_relay::RelaySession {
        tcp_servers: vec![zwift_relay::TcpServer { ip: "10.99.99.99".into() }],
        ..fixture_session()
    };
    let pushed_udp_ip = "10.55.55.55";
    let tcp_factory = ScriptedTcpFactory::pushing_udp_config(pushed_udp_ip, 3023);

    let (udp_factory, connected_to) = AddrCapturingUdpFactory::new();
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(session),
        tcp_factory,
        udp_factory,
    )
    .await
    .expect("start");
    runtime.shutdown();
    let _ = runtime.join().await;

    let target = connected_to.lock().unwrap().expect(
        "udp_factory.connect() must be called once start_all_inner \
         sees the udp_config push",
    );
    assert_eq!(
        target.ip().to_string(),
        pushed_udp_ip,
        "STEP-12.13 D3: UDP target must come from the first udp_config / \
         udp_config_vod push on the TCP stream, not from session.tcp_servers",
    );
    assert_ne!(
        target.ip().to_string(),
        "10.99.99.99",
        "STEP-12.13 D3: UDP must not fall back to tcp_servers when a \
         udp_config push is available",
    );
}
```

`ScriptedTcpFactory::pushing_udp_config(ip, port)` is a new test
helper. It builds a `MockTcpTransport` whose first `read_chunk()`
returns a single framed `ServerToClient` containing the requested
`UdpConfigVod`, then blocks. The test crate already has
`build_inbound_tcp` in `crates/zwift-relay/tests/tcp.rs`; lift it
into a small shared helper or reproduce the same logic in
`tests/relay_runtime.rs`.

`AddrCapturingUdpFactory` is a new stub: same shape as
`RecordingUdpFactory` but stores the `addr: SocketAddr` passed to
`connect()` so the test can read it back.

#### 3a.ii — `start_all_inner_waits_for_udp_config_before_udp_connect`

Build a TCP fixture that sends `Established` but no `udp_config`
push. Assert the runtime reports a clear timeout error rather than
silently using `tcp_servers[0]`.

```rust
#[tokio::test]
async fn start_all_inner_waits_for_udp_config_before_udp_connect() {
    let cfg = make_config("monitor@example.com", "monitor-pass");
    let tcp_factory = StubTcpFactory::new(); // never pushes udp_config

    let (udp_factory, connected_flag, _) = RecordingUdpFactory::new();
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(500),
        RelayRuntime::start_with_all_deps(
            &cfg,
            None,
            StubAuth,
            StubSupervisorFactory::new(fixture_session()),
            tcp_factory,
            udp_factory,
        ),
    )
    .await;

    assert!(
        !*connected_flag.lock().unwrap(),
        "STEP-12.13 D3: udp_factory.connect() must not be called before \
         the daemon receives a udp_config push from the TCP stream",
    );
    match result {
        Ok(Err(e)) => {
            // Acceptable: surface a typed error pointing at the
            // missing udp_config (e.g. RelayRuntimeError::NoUdpConfig
            // or the existing EstablishedTimeout repurposed).
            let msg = e.to_string();
            assert!(
                msg.contains("udp_config") || msg.contains("UDP"),
                "STEP-12.13 D3: error surfaced when no udp_config push \
                 arrives must mention udp_config or UDP; got {msg:?}",
            );
        }
        Ok(Ok(_)) => panic!(
            "STEP-12.13 D3: start_all_inner must not succeed when no \
             udp_config push arrives — silently falling back to \
             tcp_servers is the bug being fixed",
        ),
        Err(_) => panic!(
            "STEP-12.13 D3: start_all_inner must surface a typed error \
             within the 500 ms wait window rather than blocking forever",
        ),
    }
}
```

The exact error variant is implementation choice; the test pins
the contract ("an error mentioning UDP or udp_config", not
"silently falls back").

#### 3a.iii — `udp_pool_router_picks_target_for_watched_athlete`

If multiple `RelayAddress` entries are pushed (via
`udp_config_vod_1.relay_addresses_vod[*].relay_addresses[*]`), the
pool router must select the one matching the watched athlete's
`(realm, course)`. Lower priority than the first two — covers the
production-at-scale case where the pool has more than one entry.

```rust
#[tokio::test]
async fn udp_pool_router_picks_target_for_watched_athlete() {
    let mut cfg = make_config("monitor@example.com", "monitor-pass");
    cfg.watched_athlete_id = Some(5_213_306);

    // Push a UdpConfigVod with two pools: realm=0 generic (default)
    // and realm=1 course=42 (the test plants the watched athlete on
    // realm 1, course 42 via observe_watched_player_state below).
    let tcp_factory = ScriptedTcpFactory::pushing_udp_config_vod_pools(vec![
        ("10.0.0.1", 3023, 0, 0),  // generic
        ("10.0.0.2", 3023, 1, 42), // course-scoped
    ]);

    let (udp_factory, connected_to) = AddrCapturingUdpFactory::new();
    let runtime = RelayRuntime::start_with_all_deps(
        &cfg,
        None,
        StubAuth,
        StubSupervisorFactory::new(fixture_session()),
        tcp_factory,
        udp_factory,
    )
    .await
    .expect("start");
    runtime.observe_watched_player_state(1, 42, 0.0, 0.0);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    runtime.shutdown();
    let _ = runtime.join().await;

    let target = connected_to.lock().unwrap().expect("connect called");
    assert_eq!(
        target.ip().to_string(),
        "10.0.0.2",
        "STEP-12.13 D3: pool router must pick the (realm=1, course=42) \
         entry for the watched athlete; got {target}",
    );
}
```

This test depends on `observe_watched_player_state` running before
the UDP target selection, which means 3b must update the pool
selection any time the watched-athlete state changes (the existing
`recompute_udp_selection` machinery already exists for this; wire
it).

### 3b — Implementation (green state)

Five sub-edits:

1. **Surface `udp_config` decoding in `zwift-relay`.** Add a small
   public helper in `crates/zwift-relay/src/lib.rs` (or a new
   `udp_pool.rs`) that converts a `zwift_proto::ServerToClient`
   into `Option<Vec<RelayAddress>>`, preferring `udp_config_vod_1`
   (per-realm pools), falling back to `udp_config_vod_2`, and
   finally to the flat `udp_config.relay_addresses`. Returning
   `Vec<RelayAddress>` (not `Vec<TcpServer>`) preserves the
   `lb_realm` / `lb_course` / `port` fields the pool router needs.

   ```rust
   pub fn extract_udp_servers(stc: &zwift_proto::ServerToClient)
       -> Option<Vec<zwift_proto::RelayAddress>> { ... }
   ```

2. **Add a typed runtime error.** Extend
   `RelayRuntimeError` in `src/daemon/relay.rs`:

   ```rust
   #[error("no udp_config received from TCP stream within {0:?}")]
   NoUdpConfig(Duration),
   ```

   Hook it up in step 4 below.

3. **Add a "wait for udp_config" step in `start_all_inner`.**
   Between the existing TCP-Established branch (step 6) and the
   current UDP-connect (step 9):

   ```rust
   // 8.5. Wait for the first ServerToClient carrying a udp_config*.
   //      Sauce4zwift relies on this push to know where to send
   //      UDP — see docs/plans/STEP-12.13-still-screwing-up-after-
   //      all-these-years.md §D3.
   let udp_config_deadline = std::time::Duration::from_secs(5);
   let mut udp_servers: Vec<zwift_proto::RelayAddress> = Vec::new();
   let deadline = tokio::time::Instant::now() + udp_config_deadline;
   while udp_servers.is_empty() {
       let remaining = deadline.saturating_duration_since(
           tokio::time::Instant::now(),
       );
       if remaining.is_zero() {
           return Err(RelayRuntimeError::NoUdpConfig(udp_config_deadline));
       }
       match tokio::time::timeout(remaining, events_rx.recv()).await {
           Ok(Ok(zwift_relay::TcpChannelEvent::Inbound(stc))) => {
               if let Some(addrs) = zwift_relay::extract_udp_servers(&stc) {
                   tracing::info!(
                       target: "ranchero::relay",
                       count = addrs.len(),
                       "relay.udp.config_received",
                   );
                   udp_servers = addrs;
               }
           }
           Ok(Ok(_)) => continue, // ignore other events while waiting
           Ok(Err(_)) | Err(_) => {
               return Err(RelayRuntimeError::NoUdpConfig(udp_config_deadline));
           }
       }
   }
   ```

   Note: this consumes events from `events_rx` before the recv-loop
   spawns. The current code spawns the forwarder + recv-loop at
   step 12 (after UDP is up). That ordering still works — anything
   we consume here is "before the loop subscribed", so it cannot
   reach the loop. If the loop later needs to see those messages,
   re-broadcast them through `events_tx` after the wait completes.

4. **Apply the received pool to `RuntimeInner.pool_router` and
   pick the UDP target.** Build a `UdpServerPool` from the
   extracted `RelayAddress` list and call
   `pool_router.apply_pool_update(...)`. Use the watched athlete's
   `(realm, course)` (from `WatchedAthleteState`, defaulting to
   `(0, 0)` if no athlete is being watched) to pick the best entry.
   The existing `current_udp_server: Mutex<Option<SocketAddr>>`
   field is the natural place to cache the selection.

5. **Replace the `&session.tcp_servers[0]` read.** Use the
   `current_udp_server` selection from step 4 as the UDP target:

   ```rust
   let udp_addr = inner
       .current_udp_server
       .lock()
       .expect("current_udp_server mutex")
       .ok_or_else(|| RelayRuntimeError::NoUdpConfig(udp_config_deadline))?;
   tracing::info!(target: "ranchero::relay", addr = %udp_addr, "relay.udp.connecting");
   let udp_transport = udp_factory
       .connect(udp_addr)
       .await
       .map_err(RelayRuntimeError::UdpConnect)?;
   ```

   Delete the `udp_addr_str.parse()` block and the
   `BadTcpAddress` variant from the UDP path.

6. **Wire the live `udp_config` stream into the pool router.**
   The recv-loop already handles `TcpChannelEvent::Inbound` (Phase
   6b emits `relay.tcp.message.recv`). Extend that arm to call
   `inner.pool_router.lock().apply_pool_update(...)` whenever
   `extract_udp_servers(&stc)` returns `Some` and to call
   `recompute_udp_selection` so a `GameEvent::PoolSwap` fires when
   the chosen target changes. The pool-router infrastructure
   already exists (`apply_pool_update`,
   `observe_watched_player_state`, `recompute_udp_selection`); all
   that's missing is the bridge from live messages.

### Verification

Re-run the live trace:

```
ranchero start --debug --capture output.cap
```

Expected post-3b output (in order):

```
relay.tcp.established …
relay.tcp.hello.sent
relay.tcp.message.recv … has_udp_config=true
relay.udp.config_received count=N
relay.udp.connecting addr=<udp-target>
relay.udp.hello.started …
relay.udp.hello.sent hello_idx=1 …
relay.udp.hello.ack …
relay.udp.sync.converged …
relay.udp.established latency_ms=…
relay.heartbeat.started
relay.heartbeat.tick (per second)
```

If `relay.udp.hello.ack` still doesn't fire after 3b, the target is
right but something else (firewall, port, hello payload format) is
wrong — open a follow-up ticket; that's a different defect.

## Out-of-scope clarifications

- Capture-format changes. v2 is the current format; nothing in
  these defects requires another bump. The manifest already on disk
  remains valid.
- Documentation. STEP-12.12's `--capture` and `--debug` contract
  text remains correct; D1, D2, and D3 are implementation drift,
  not contract drift. Do not touch the STEP-12.12 acceptance
  section.
- Other tests / lints. STEP-12.13 is a defect log, not a refactor.
  New tests should be the minimum needed to pin the fix; do not
  rewrite tangential UDP tests just because they live in the same
  file.
- Pool-routing polish. D3 brings UDP up off the live `udp_config`
  push and wires the watched-athlete `recompute_udp_selection`
  loop. Any further pool-routing work — handling realm/course
  changes mid-session, multi-cluster failover, picking by
  `ra_f5` / `ra_f6` distance metrics — belongs in a future
  pool-routing step, not here.
