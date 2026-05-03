# Step 12.12 — Log every message-passing point so the next outage is visible

**Status:** investigation (2026-05-02).

The trace investigated in STEP-12.11

```
2026-05-02T09:27:05.297735Z  INFO ranchero::relay: relay.tcp.connecting addr=16.146.39.255:3025
2026-05-02T09:27:05.503111Z  INFO ranchero::relay: relay.tcp.established addr=16.146.39.255:3025
2026-05-02T09:27:15.703624Z  WARN ranchero::relay: relay.tcp.recv_error error=TCP peer closed
```

is exactly the kind of trace that should not require source-code
archaeology to interpret. Ten seconds of silence between
`relay.tcp.established` and `relay.tcp.recv_error` should never have
been possible: every packet sent, every packet received, every state
mutation, every internal channel event, and every HTTP exchange should
appear in the daemon log and (where it is on the wire) in the capture
file. They do not. The current `--debug` output emits roughly fifteen
event names; the actual code base has more than forty silent
message-passing points.

This document is both a research inventory of every silent point and
a phased implementation plan. The inventory and contracts come first;
the **Implementation phasing** section near the end of this file
breaks the work into eight phases, each with a tests-first / then-
implement sub-pair. The summary checklist immediately below tracks
those pairs.

## Summary checklist

External prerequisite (must already be in `done/` before phase 7
verification can run against live traffic):

- [x] STEP-12.11 — `start_with_writer` routed through `start_all_inner`,
      `DefaultUdpTransportFactory` no longer a stub.

Phased work (this step):

- [x] **0a** — Pre-requisites tests: capture format v2 round-trip,
      manifest record decode, `Http` transport variant, `--debug` filter
      enables `debug` for `zwift_relay::*` and `zwift_api::*`.
- [x] **0b** — Pre-requisites implementation: bump capture format to
      v2 (reject v1), add `TransportKind::Http`, add `RecordKind::Manifest`
      / `record_session_manifest` API, fix `--debug` filter scope.
- [x] **1a** — TCP module tests (`zwift-relay::tcp`): capture stores
      framed wire bytes (length + header + ciphertext + tag) for both
      send and recv; `relay.tcp.frame.sent` / `relay.tcp.frame.recv` /
      `relay.tcp.decrypt.ok` events with correct fields.
- [x] **1b** — TCP module implementation: relocate `record_outbound`
      to write `wire` (not `proto_bytes`); relocate `record_inbound` to
      write `payload_owned` before `process_inbound`; emit the three
      tracing events.
- [x] **2a** — UDP module tests (`zwift-relay::udp`): capture stores
      the encrypted datagram for both hello and steady-state, send and
      recv; `relay.udp.hello.started` / `hello.sent` / `hello.ack` /
      `sync.converged` / `playerstate.sent` / `message.recv` events
      with correct fields.
- [x] **2b** — UDP module implementation: relocate `record_outbound`
      / `record_inbound` calls to wire bytes at all four sites; emit the
      six tracing events; replace bare `relay.udp.inbound` with
      `relay.udp.message.recv` carrying decoded fields.
- [x] **3a** — Capture writer diagnostics tests
      (`zwift-relay::capture`): `relay.capture.record.dropped` (warn) on
      channel saturation; `relay.capture.writer.flushed` (debug) per
      flush; `relay.capture.writer.closed` (info) with totals.
- [x] **3b** — Capture writer diagnostics implementation: emit the
      three tracing events from the writer task and the producer-side
      drop path.
- [x] **4a** — Session and supervisor tests
      (`zwift-relay::session`): `relay.session.login.started` /
      `login.ok` / `tcp_servers` / `refresh.ok`; `relay.supervisor.
logged_in` / `refreshed` / `refresh_failed` / `relogin_attempt` /
      `relogin_ok` / `login_failed`.
- [x] **4b** — Session and supervisor implementation: emit the
      events from the single-shot helpers and the supervisor refresh
      loop.
- [x] **5a** — Auth / HTTP tests (`zwift-api`): every request and
      response body appears in the capture as a `TransportKind::Http`
      record; `relay.auth.token.requested` / `token.granted` /
      `profile.ok` / `profile.failed` / `http.request` / `http.response`
      / `http.retry` / `refresh.completed` events with correct fields.
- [ ] **5b** — Auth / HTTP implementation: inject a capture-sink
      dependency into `ZwiftAuth`, route request and response bodies
      through it, emit the eight tracing events.
- [ ] **6a** — Daemon integration tests (`ranchero::daemon::relay`):
      `start_all_inner` writes a session-manifest record after relay-
      session login; supervisor refresh / re-login writes a fresh
      manifest; `recv_loop` handles `TcpChannelEvent::Inbound` and emits
      `relay.tcp.message.recv`; `relay.state.change` info event on every
      `RuntimeState` transition; `relay.heartbeat.tick` debug events;
      `relay.heartbeat.send_failed` warn on tick failure.
- [ ] **6b** — Daemon integration implementation: call
      `record_session_manifest` from `start_all_inner` and the
      supervisor-event handler; add the `Inbound` arm to `recv_loop`;
      add state-change tracing alongside every `GameEvent::StateChange`
      emission; add heartbeat tick tracing.
- [ ] **7a** — Closing review verification: live `ranchero start
--debug --capture output.cap` matches the Acceptance event
      sequence; `CaptureReader` + manifest decrypt-and-decode round
      trip succeeds; audit confirms no `transport.send` / `write_all` /
      `recv` site lacks an adjacent `record_*` call; `cargo test` clean.
- [ ] **7b** — Closing review cleanup: remove obsolete TODOs and
      Defect 4 / Defect 7 comments tied to this work; refresh any plan-
      doc references that point at the now-changed line numbers.

Implementation will be tracked under STEP-12.13 (or whichever
follow-up step is opened to act on this).

## Two output channels — `--debug` vs `--capture`

These are independent surfaces with different responsibilities. The
inventory below assigns every silent point to exactly one of them; the
two channels are not interchangeable.

### `--debug` (the daemon log)

- **Purpose:** human-readable structured tracing of what the daemon is
  doing, why, and how the underlying interaction is progressing. Both
  network-related and non-network events belong here.
- **Format:** `tracing` events with named fields. `target` and event
  name follow the `ranchero::relay` / `relay.<area>.<verb>` convention
  (or other `target`s for non-relay code, e.g. `ranchero::daemon`).
- **Content:** intent, decisions, decoded message kinds, state
  transitions, errors, retry attempts, computed delays, and rolled-up
  counters. **Not** raw payload bytes.
- **Trigger:** the `--debug` flag must enable `debug`-level events for
  every relevant target — `ranchero::*`, `zwift_relay::*`,
  `zwift_api::*`. Today the flag appears to filter on the daemon
  target only; verify and correct as part of the implementation step.

### `--capture` (the capture file)

- **Purpose:** complete raw on-the-wire record of the conversation,
  suitable for offline replay and decode (`ranchero follow`). The file
  is the canonical artefact for "what was actually exchanged".
- **Format:** the existing `RNCWCAP\0` framed container defined in
  `crates/zwift-relay/src/capture.rs`, extended as required to carry
  the per-session decrypt manifest (see contract below). Each record
  carries a timestamp, a `Direction` (inbound vs outbound — which end
  of the connection the bytes came from), a `TransportKind` (UDP vs
  TCP vs HTTP), and the raw `payload` bytes.
- **Content:** the bytes that crossed the socket, exactly as they
  crossed it. No decoded fields, no human prose, no log levels, no
  event names. The direction tag and transport tag are the only
  annotations that may sit alongside the bytes.
- **Completeness rule:** every send and every receive on the wire
  must produce a capture record. If a send or receive happens without
  a corresponding `record_outbound` / `record_inbound` call, that is
  a defect.

### Capture contract — wire bytes, not plaintext

The capture must store **the wire bytes**: the encrypted ciphertext
exactly as transmitted on TCP and UDP, including the framed header
(for TCP) and the unencrypted prefix (for UDP). It must additionally
store, once per session, the material a future `ranchero follow`
needs to recover the plaintext: the AES key, the starting IV state,
and any conn-id / relay-id values needed to reconstruct the IV input
across frames.

This was the original intent in **STEP-11.5** (`--capture-raw` was
named there and deferred under the assumption that codec replay was
not a near-term need). The current implementation in
`crates/zwift-relay/src/{tcp,udp}.rs` instead records
**plaintext-after-decryption (inbound)** and
**plaintext-before-encryption (outbound)**:

- TCP outbound: `tcp.rs:234` records `proto_bytes` (encoded
  `ClientToServer` before envelope-prefix and AES-GCM).
- TCP inbound: `tcp.rs:400` records `plaintext` (post-decryption).
- UDP outbound: `udp.rs:245` and `udp.rs:403` record `proto_bytes`
  (pre-encryption).
- UDP inbound: `udp.rs:293` and `udp.rs:574` record `plaintext`
  (post-decryption).

This is a defect against the contract. The "plaintext" term here
means **post-AES-decryption protobuf bytes** (or the equivalent
pre-encryption bytes outbound) — not human-readable text. The actual
capture file is binary because protobuf is binary; this is also why a
quick `cat output.cap` shows mixed printable and unprintable
characters. The defect is that the captured bytes are the
**decrypted** payload, not the wire bytes that the socket actually
saw.

Plaintext capture loses every class of bug that lives between
`prost::encode` and the socket write — framing, header flag
construction, IV-counter divergence, AES-GCM tag handling, length-
prefix mismatches, sticky-server reconnect bookkeeping. None of
those are observable in the current file. A plaintext capture also
cannot be replayed against a real Zwift server because the keys and
IVs are gone.

#### What the wire-byte capture must contain

- **Per-session manifest record** (one per `start` invocation, written
  immediately after the file header and before any frame record):
  - the AES key derived for the session,
  - the device / channel discriminants used in the IV (`DeviceType`,
    `ChannelType` — see `RelayIv` in the codec),
  - the starting `iv_seqno` for both send and recv directions on each
    transport,
  - the assigned `relay_id` and `conn_id`,
  - the relay-session expiration timestamp so a reader can age out
    expired sessions.
- **Per-frame records:** the bytes as they appear on the wire — for
  TCP, the 2-byte length prefix plus header plus ciphertext plus tag;
  for UDP, the unencrypted header plus ciphertext plus tag.
- **HTTP records:** request and response bodies as transmitted (TLS
  termination has happened above this layer; what we store is what
  `reqwest` sent or received). HTTP is not encrypted at the
  application layer the way TCP/UDP are, so no manifest entry is
  needed for it.
- **Session-rotation manifest records:** any time the relay session
  is refreshed or re-logged-in and a new AES key or `relay_id` is
  issued, write a new manifest record so subsequent frames decrypt
  with the current material.

#### Trust boundary

The capture file inherits the trust boundary of the daemon process.
The captured AES key, refresh token, and credentials in HTTP request
bodies are sensitive. Document this in the writer module and in the
operator-facing `--capture` flag help text. Do **not** redact or
sanitise the file — a sanitised capture cannot reproduce a failure,
which defeats the purpose. The mitigation is operator awareness, not
content scrubbing.

#### Format-version implications

The current `RNCWCAP\0` format is version 1. The manifest record and
the new `Http` transport variant are both breaking format changes;
bump to version 2 and reject version 1 files in `CaptureReader` with
a clear error. There are no production captures to migrate.

### Other capture pinned decisions

- HTTP exchanges (auth, session login, profile fetch, refresh) are
  not captured today and must be under the completeness rule. Item
  5.0 below tracks the gap.
- All capture writes happen on a non-blocking mpsc to a background
  writer task. The completeness rule binds the producer side
  (`record_outbound` / `record_inbound` must be called for every wire
  event); the writer task is allowed to drop on a full channel and
  surface that drop as a `--debug` event (see section 7.B).

The remainder of this document tags every item with **Channel:
`--debug`** or **Channel: `--capture`** so the implementation step
knows where the work lands.

## Naming convention (`--debug` channel)

All daemon-emitted tracing events use `target = "ranchero::relay"` and
the event name pattern `relay.<area>.<verb>` (already established by
`relay.tcp.established`, `relay.capture.opened`, etc.). Areas in use
or proposed:

- `relay.auth.*` — HTTP exchanges with the OAuth and profile API.
- `relay.session.*` — relay-session login, refresh, and supervisor
  events (single-shot calls and supervisor-task lifecycle).
- `relay.supervisor.*` — high-level supervisor state transitions
  (`logged_in`, `refreshed`, `relogin_attempt`).
- `relay.tcp.*` — TCP transport: connect, established, frame send,
  frame recv, hello, shutdown.
- `relay.udp.*` — UDP transport: connect, established, hello,
  player-state send, message recv, shutdown.
- `relay.heartbeat.*` — the 1 Hz heartbeat scheduler.
- `relay.capture.*` — capture writer lifecycle and writer-state
  diagnostics. These are `--debug`-channel tracing events _about_ the
  writer; they are distinct from the records the writer puts into the
  `--capture` file. The capture file itself has no event names — only
  framed records.

Verbs already in use: `opened`, `closed`, `connecting`, `established`,
`timeout`, `recv_error`, `inbound`, `shutdown`, `started`. Verbs to
add: `sent`, `recv`, `requested`, `granted`, `failed`, `fire`,
`attempt`, `ok`, `dropped`, `flushed`.

## Logging level guidance

Three pragmatic tiers, since the volume of messages on a steady-state
connection is large enough that not everything can stay at `info`.

- **`info`** — every lifecycle event, every connection-state
  transition, every authentication exchange, every supervisor event,
  every error. This tier must be sufficient to reconstruct what the
  daemon was _trying_ to do and where it stopped. Default verbosity.
- **`debug`** — per-packet send and recv events, per-record capture
  writes, per-tick heartbeat output. Off by default; enabled by
  `--debug`.
- **`trace`** — per-frame IV sequence numbers, per-sample latency or
  offset accumulators, decode-buffer slice boundaries. Off by default;
  enabled only by an explicit `RUST_LOG=trace` override. This tier
  exists for the hardest correctness investigations and is allowed to
  be expensive.

The `--debug` flag must enable `debug`-level events for
`ranchero::relay` and the underlying crates (`zwift_relay::*`,
`zwift_api::*`). Today it appears to enable the daemon target only.
Verify and correct as part of the implementation step.

## Inventory of silent message-passing points

The eight sections below correspond to the architectural areas in
which traffic flows. Each entry names the source file, the line range,
the kind of message, the **channel** the missing output lands on
(`--debug`, `--capture`, or — most commonly — both), the proposed
tracing event name, the proposed log level, and the fields that
should be attached. File and line references are as of 2026-05-02 and
may shift slightly during implementation; treat them as a starting
point for the diff, not a contract.

Wire-traffic items (sections 1–4) all need work on **both** channels:
a `--capture` correction so the bytes survive replay (the current
plaintext records must be replaced with wire bytes per the contract
above), and a `--debug` event so the operator can see the message
kind and key fields without decoding the file. Items in sections 5–8
are mostly `--debug`-only — they describe internal state, not on-wire
bytes — with the exception of HTTP exchanges in section 5, which
today are not captured at all and should be (see 5.0).

### 1. TCP outbound (sent to server)

#### 1.1 TCP hello packet

- **Channel:** `--debug` (event) and `--capture` (bytes — corrected
  by 1.2 below).
- **Site:** `src/daemon/relay.rs:1036-1048` — the
  `tcp_sender.send_packet(...)` call inside `start_all_inner` step 8,
  with `hello = true`.
- **Today:** zero log output. `relay.tcp.hello.sent` was named in the
  STEP-12.6 plan but never landed in code. Capture: the hello's
  plaintext is recorded but the wire-byte hello (header + ciphertext
  - tag) is not — see 1.2.
- **Proposed:** `relay.tcp.hello.sent` at `info`, fields
  `{ athlete_id, server_realm, seqno }`. This is a one-shot event;
  `info` is appropriate.

#### 1.2 Generic TCP frame send

- **Channel:** `--debug` (event) and `--capture` (bytes — currently
  records the wrong content).
- **Site:** `crates/zwift-relay/src/tcp.rs:225-273` —
  `TcpChannel::send_packet(payload, hello)`. `record_outbound(...,
&proto_bytes)` runs at line 234 (writes plaintext proto bytes);
  `transport.write_all(&wire)` runs at line 271 (writes the framed
  ciphertext that actually crossed the socket).
- **Today:** zero `--debug` output. Capture records the wrong bytes
  (plaintext proto, not wire). `send_state.iv_seqno` is incremented
  at line 261 with no `--debug` record.
- **Proposed:**
  - **`--capture` correction:** move the `record_outbound` call to
    record `wire` (the framed ciphertext) instead of `proto_bytes`.
    The first send on the channel must be preceded by a manifest
    record (see capture contract above) carrying the AES key and the
    IV seed; subsequent frames carry only the wire bytes.
  - **`--debug` event:** `relay.tcp.frame.sent` at `debug`, fields
    `{ seqno, iv_seqno, hello, wire_size }`. Keep at `debug`, not
    `info`, because this is also the path the heartbeat-equivalent
    will use if any TCP traffic ever becomes periodic.

### 2. TCP inbound (received from server)

Capture coverage for this section is wrong today: `tcp.rs:400` records
the post-decryption plaintext, not the wire bytes that arrived.
Correction: move the `record_inbound` call to record `payload_owned`
(the framed wire bytes drained from the buffer at `tcp.rs:390`)
_before_ `process_inbound` runs, so the file holds what the server
actually sent. The `--debug` items below are otherwise additive.

#### 2.1 Frame extraction from buffer

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/frame.rs:94-105` —
  `next_tcp_frame()`. Called from `crates/zwift-relay/src/tcp.rs` recv
  loop around line 386.
- **Today:** silent. Successful frame extraction is observable only by
  the side effect that `process_inbound` is then called.
- **Proposed:** `relay.tcp.frame.recv` at `debug`, fields
  `{ size, seqno, relay_id_present, conn_id_present }`.

#### 2.2 Frame decryption and IV state advance

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/tcp.rs:307-342` —
  `process_inbound`. Header decode at 314, decrypt at 339, IV seqno
  post-increment at 340.
- **Today:** silent on success. Errors are returned and logged once,
  upstream, as `relay.tcp.recv_error`.
- **Proposed:** `relay.tcp.decrypt.ok` at `trace`, fields
  `{ seqno, relay_id, conn_id }`. This is the hardest information to
  reconstruct after the fact; trace-level keeps it cheap when off.

#### 2.3 Inbound `ServerToClient` decode

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/tcp.rs:401-405` — `ServerToClient::
decode(plaintext)` followed by `TcpChannelEvent::Inbound(stc)`
  broadcast.
- **Today:** the broadcast is silent; the recv loop in the daemon
  receives the event but does not log it (`recv_loop` only handles
  `Established`, `Timeout`, `RecvError`, `Shutdown`).
- **Proposed:** `relay.tcp.message.recv` at `debug`, fields
  `{ message_kind, seqno, has_state_change, has_world_info }` where
  `message_kind` is the discriminant of the relevant `oneof` field on
  `ServerToClient`. The recv loop in `src/daemon/relay.rs` is the
  natural place to emit this when handling `TcpChannelEvent::Inbound`,
  which today is unhandled.

### 3. UDP outbound (sent to server)

Capture coverage is wrong today: `udp.rs:245` (hello) and `udp.rs:403`
(steady state) record `proto_bytes` (pre-encryption plaintext), not
the encrypted datagram that goes on the wire. Correction: relocate
both `record_outbound` calls to record `wire` (the encrypted
datagram, ready for `transport.send`) instead of `proto_bytes`. As
with TCP, the first send must be preceded by a manifest record
carrying key and IV state. The `--debug` items below are otherwise
additive.

#### 3.1 UDP hello loop

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/udp.rs:239-271` — the hello loop
  inside `UdpChannel::establish`. Per iteration: `build_hello` at 244,
  `record_outbound` at 245 (capture present), `transport.send` at 268,
  IV and app seqno increments at 270-271.
- **Today:** silent on the `--debug` channel.
- **Proposed:** `relay.udp.hello.sent` at `debug`, fields
  `{ hello_idx, app_seqno, iv_seqno, payload_size }`. The first hello
  attempt is also worth a one-shot `info` event,
  `relay.udp.hello.started`, naming the target address.

#### 3.2 Steady-state player-state send

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/udp.rs:388-433` —
  `UdpChannel::send_player_state`. Encode at 402, `record_outbound` at
  403, `transport.send` at 431, IV/app seqno increments at 423-424.
- **Today:** silent on the `--debug` channel.
- **Proposed:** `relay.udp.playerstate.sent` at `debug`, fields
  `{ world_time, app_seqno, iv_seqno, payload_size }`. Optionally a
  `trace`-level companion `relay.udp.playerstate.fields` carrying the
  decoded position, cadence, speed, and action when one is debugging
  the heartbeat content directly.

#### 3.3 Heartbeat scheduler tick

- **Channel:** `--debug`. (The wire-side bytes from each tick are
  captured at 3.2.)
- **Site:** `src/daemon/relay.rs:182-196` (`UdpHeartbeatSink::send`)
  and `src/daemon/relay.rs:361-380` (`HeartbeatScheduler::run`). The
  1 Hz loop calls `send_one()` once per interval at line 378.
- **Today:** silent — only the one-shot `relay.heartbeat.started` is
  logged. There is no record of ticks firing or of send outcomes.
- **Proposed:** `relay.heartbeat.tick` at `debug`, fields
  `{ interval_ms, send_ok }`. On `send_ok = false`, also emit
  `relay.heartbeat.send_failed` at `warn` with the underlying error.

### 4. UDP inbound

Capture coverage is wrong today: `udp.rs:293` (hello path) and
`udp.rs:574` (steady state) record post-decryption plaintext.
Correction: move the `record_inbound` calls to record the encrypted
datagram returned from `transport.recv()` _before_
`process_inbound_packet` runs. The `--debug` items below are
otherwise additive.

#### 4.1 Hello-loop response handling

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/udp.rs:273-322` — inside
  `establish`. `transport.recv` at 284, `process_inbound_packet` at
  286-292, `record_inbound` at 293, decode at 294, ack matching and
  latency/offset sample collection at 296-309, `SyncOutcome` transition
  at 311.
- **Today:** silent. The latency reported by
  `relay.udp.established { latency_ms }` is the _only_ surviving
  artefact; the per-sample data and convergence path are invisible.
- **Proposed:**
  - `relay.udp.hello.ack` at `debug`, fields
    `{ app_seqno, latency_ms, offset_ms }` per ack received.
  - `relay.udp.sync.converged` at `info`, fields
    `{ mean_offset_ms, median_latency_ms, sample_count }` once.

#### 4.2 Steady-state inbound packet

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/udp.rs:544-603` — `recv_loop`.
  Decrypt at 566-572, capture at 574, decode at 575, broadcast at 577.
- **Today:** the recv loop in `src/daemon/relay.rs:1738` logs
  `relay.udp.inbound` at `debug` but with **no fields** — the message
  contents are lost.
- **Proposed:** replace the bare `relay.udp.inbound` with
  `relay.udp.message.recv` at `debug`, fields
  `{ message_kind, world_time, player_count, payload_size }`.

### 5. Auth / HTTP

All sites in `crates/zwift-api/src/lib.rs`. None of these emit any
tracing today; every HTTP exchange is silent on success and only the
constructed `Error` variant carries the failure detail.

#### 5.0 Capture-completeness gap

- **Channel:** `--capture` (defect — bytes are not captured today).
- **Today:** the `--capture` file contains TCP and UDP frames only.
  HTTP request and response bodies for token grant, refresh, profile,
  relay-session login, and relay-session refresh are not written. This
  violates the completeness rule — HTTP bytes are on-the-wire bytes.
- **Proposed:** extend `TransportKind` to include an `Http` variant
  (or an equivalently-named discriminant) and call
  `record_outbound` / `record_inbound` for each HTTP request body and
  response body in `crates/zwift-api/src/lib.rs`. Direction is
  unambiguous (outbound = request, inbound = response). The `hello`
  flag is unused for HTTP; pass `false`. Body bytes go in the payload
  field exactly as transmitted, including any `Content-Type` boundary
  bytes — header lines themselves do not need to be captured because
  the auth client builds them from a small fixed set already
  documented in `--debug` events 5.3.
- **Out-of-scope clarification:** OAuth credentials in the token
  request body are sensitive. Capture the body verbatim regardless —
  the capture file inherits the same trust boundary as the daemon
  process, and a sanitised capture would not be suitable for
  reproducing a failure. Document the sensitivity in the writer
  module instead.

#### 5.1 Token grant (`login`)

- **Channel:** `--debug` (event) and `--capture` (request and response
  bytes — see 5.0).
- **Site:** `crates/zwift-api/src/lib.rs:192-237`.
- **Proposed:**
  - `relay.auth.token.requested` at `info`, fields
    `{ username, grant_type }`.
  - `relay.auth.token.granted` at `info`, fields
    `{ expires_in_s, refresh_expires_in_s }`.
  - `relay.auth.profile.fetched` at `info`, fields `{ athlete_id }`
    (the existing `relay.login.ok` already carries this; the
    `relay.auth.*` event documents the underlying HTTP path).

#### 5.2 Profile fetch (`get_profile_me`)

- **Channel:** `--debug` and `--capture` (see 5.0).
- **Site:** `crates/zwift-api/src/lib.rs:243-272`.
- **Proposed:** `relay.auth.profile.ok` at `debug`, fields
  `{ athlete_id }`. Failures already raise typed errors (added in
  STEP-12.10); also emit `relay.auth.profile.failed` at `warn` with
  status and variant before the error propagates.

#### 5.3 Authenticated POST (`post`) and GET (`fetch`)

- **Channel:** `--debug` and `--capture` (see 5.0).
- **Sites:** `crates/zwift-api/src/lib.rs:322-360` (POST) and
  `crates/zwift-api/src/lib.rs:364-394` (GET). Both have a 401-retry
  path with an inline `refresh()` call that is also silent.
- **Proposed:**
  - `relay.auth.http.request` at `debug`, fields
    `{ method, urn, content_type, body_size }`.
  - `relay.auth.http.response` at `debug`, fields
    `{ method, urn, status, body_size, retried }`.
  - `relay.auth.http.retry` at `info` whenever the 401-retry path
    fires, fields `{ method, urn }`.

#### 5.4 Background refresh (`do_refresh`)

- **Channel:** `--debug` and `--capture` (see 5.0).
- **Site:** `crates/zwift-api/src/lib.rs:418-449`. Errors are logged
  at `warn` from the caller; success is silent.
- **Proposed:** `relay.auth.refresh.completed` at `info`, fields
  `{ expires_in_s, next_refresh_in_s }`.

### 6. Session login and supervisor

The HTTP exchanges in 6.1 and 6.2 go through `auth.post` and so are
covered by the section 5 capture work. All items in this section are
otherwise `--debug`-channel only.

#### 6.1 Single-shot `login`

- **Channel:** `--debug` (event); `--capture` is covered indirectly
  via 5.3.
- **Site:** `crates/zwift-relay/src/session.rs:137-190`. The HTTP
  exchange goes through `auth.post` (silent — see 5.3); the response
  decode and TCP-server filtering also produce no record.
- **Proposed:**
  - `relay.session.login.started` at `info`, fields `{ athlete_id }`.
  - `relay.session.login.ok` at `info`, fields
    `{ relay_id, tcp_server_count, server_time_ms, expiration_min }`.
  - `relay.session.tcp_servers` at `debug`, fields
    `{ servers }` where `servers` is a comma-joined list of IPs.

#### 6.2 Single-shot `refresh`

- **Channel:** `--debug` (event); `--capture` covered via 5.3.
- **Site:** `crates/zwift-relay/src/session.rs:194-221`.
- **Proposed:** `relay.session.refresh.ok` at `info`, fields
  `{ relay_id, new_expiration_min }`.

#### 6.3 Supervisor task

- **Channel:** `--debug`.
- **Site:** `crates/zwift-relay/src/session.rs:245-372` —
  `RelaySessionSupervisor::start` and the spawned `refresh_loop`. Emits
  `SessionEvent::LoggedIn`, `Refreshed`, `RefreshFailed`, `LoginFailed`
  on a broadcast channel; the daemon currently does not subscribe.
- **Proposed:** once the daemon subscribes to supervisor events
  (Defect 7 in STEP-12.6 / pending in STEP-12.11):
  - `relay.supervisor.logged_in` at `info`, fields
    `{ relay_id, expires_at }`.
  - `relay.supervisor.refresh.fire` at `info`, fields
    `{ scheduled_delay_ms, relay_id }`. Emitted when the refresh
    timer is computed, before the HTTP exchange.
  - `relay.supervisor.refreshed` at `info`, fields
    `{ relay_id, new_expires_at }`.
  - `relay.supervisor.refresh_failed` at `warn`, fields
    `{ relay_id, error }`.
  - `relay.supervisor.relogin_attempt` at `info`, fields
    `{ attempt, backoff_ms }`.
  - `relay.supervisor.relogin_ok` at `info`, fields
    `{ relay_id, attempt }`.
  - `relay.supervisor.login_failed` at `warn`, fields
    `{ attempt, error, backoff_next_ms }`.

### 7. Capture writer — completeness and writer-state diagnostics

This section is split in two by channel. 7.A audits the
`--capture`-side completeness rule (every wire byte produces a
record). 7.B covers `--debug`-side tracing about the writer itself
(drops, flushes, shutdown rollup). The two are not interchangeable:
the capture file holds payload bytes only, and writer diagnostics
must not bleed into it.

#### 7.A Capture completeness and correctness audit

- **Channel:** `--capture` (this is the byte-level pipe).
- **Today's `record_*` call sites** (verified by `Grep`): every TCP
  and UDP wire event has a `record_outbound` or `record_inbound`
  call, but each one records the **wrong content** under the
  wire-bytes contract above. The audit below is therefore both a
  completeness check (do all sends and recvs reach a record call?)
  and a correctness check (does each call record wire bytes, not
  plaintext?).
  - TCP send: `tcp.rs:234` records `proto_bytes`; must record the
    framed `wire` value built at `tcp.rs:270`.
  - TCP recv: `tcp.rs:400` records `plaintext`; must record
    `payload_owned` (the framed wire bytes drained from the read
    buffer at `tcp.rs:390`) before `process_inbound` runs.
  - UDP hello send: `udp.rs:245` records `proto_bytes`; must record
    the encrypted `wire` value built before `transport.send` at
    `udp.rs:268`.
  - UDP hello recv: `udp.rs:293` records `plaintext`; must record the
    raw datagram returned from `transport.recv()` at `udp.rs:284`
    before `process_inbound_packet` runs.
  - UDP steady-state send: `udp.rs:403` records `proto_bytes`; must
    record the encrypted `wire` value built before `transport.send`
    at `udp.rs:431`.
  - UDP steady-state recv: `udp.rs:574` records `plaintext`; must
    record the raw datagram returned from `transport.recv()` at
    `udp.rs:563` before `process_inbound_packet` runs.
- **Manifest gap:** there is no manifest record today. The session
  AES key, IV-seed values, `relay_id`, and `conn_id` are not
  persisted, so even after the wire-byte correction lands, a recorded
  file cannot be decrypted by `ranchero follow`. The capture writer
  needs a one-shot `record_session_manifest(...)` call from
  `start_all_inner` after the relay session is established (and
  again after every successful supervisor refresh / re-login that
  rotates key material).
- **HTTP gap:** auth, refresh, profile, relay-session login, and
  relay-session refresh exchanges are not captured at all. Tracked
  as item 5.0.
- **Proposed audit guard:** add a compile-time enforcement (a private
  helper or a `cargo clippy` deny rule on a custom lint) that
  prevents any future `transport.send` / `transport.write_all` /
  `transport.recv` call from compiling without an adjacent
  capture-record call. If enforcing this in lint form is too
  expensive, settle for a per-PR checklist item and revisit when the
  capture surface grows. The guard does not catch wire-vs-plaintext
  divergence; that is enforced by a round-trip test (write a known
  payload, read it back via `CaptureReader`, decrypt it using the
  manifest, and assert the result matches the original
  `ClientToServer` / `ServerToClient`).

#### 7.B Writer-state events on the `--debug` channel

- **Channel:** `--debug`. These events go to the daemon log, not into
  the capture file.
- **Site for drops:** `crates/zwift-relay/src/capture.rs:203-221` —
  `CaptureWriter::record`. Sends to a background mpsc; increments
  `dropped_count` at line 210 on full or closed channel.
- **Today:** silent on both success and drop. The dropped count is
  surfaced only at shutdown via the existing `relay.capture.closed`
  rollup.
- **Proposed:**
  - `relay.capture.record.dropped` at `warn`, fields
    `{ direction, transport, payload_size, total_dropped }`. Per-drop
    output is acceptable because drops should be rare; if they become
    frequent the warning rate itself is the diagnostic signal.
  - Per-record success logging at `trace` only (high volume), event
    `relay.capture.record.written` with the same fields. Default `info`
    and `debug` should not include per-record output.
- **Site for flush:** `crates/zwift-relay/src/capture.rs:254-277` —
  `writer_task` and `write_record`.
- **Proposed:** `relay.capture.writer.flushed` at `debug` on each
  flush boundary, fields `{ records_in_batch, bytes_written }`. On
  shutdown also emit `relay.capture.writer.closed` at `info` with
  `{ total_records, total_bytes }` so the final rollup is richer than
  the current `dropped_count` alone.

### 8. Internal channel events not yet handled

These are internal `tokio::sync::broadcast` events between async tasks
inside the daemon process. They never appear on the wire.

#### 8.1 `TcpChannelEvent::Inbound`

- **Channel:** `--debug`.
- The recv loop in `src/daemon/relay.rs` does not match `Inbound` at
  all (around lines 1700-1750 it handles `Established`, `Timeout`,
  `RecvError`, `Shutdown`). Inbound TCP frames are dropped without any
  observability.
- **Proposed:** add an `Inbound(stc)` arm that emits the event from
  2.3 above. This is also the natural place to dispatch decoded
  messages into the data model once that work begins.

#### 8.2 `GameEvent::StateChange`

- **Channel:** `--debug`.
- **Site:** `src/daemon/relay.rs:608-630`. Broadcast on
  `game_events_tx` for downstream consumers (HTTP/WebSocket server,
  data model). Today the daemon does not log these on emission.
- **Proposed:** `relay.state.change` at `info`, fields
  `{ from, to }` where `from` and `to` are `RuntimeState` discriminant
  names (e.g. `Authenticating`, `SessionLoggedIn`, `TcpEstablished`,
  `UdpEstablished`). This gives the operator a single chronological
  thread of "what state was the daemon in" without having to correlate
  six other event names.

## Silent state mutations worth instrumenting

These are not message-passing per se but they are state changes that
are observable from outside the function only via their absence. List
included here so they are not forgotten when the implementation step
is scoped.

| Site             | Mutation                            | Suggested trace event               |
| ---------------- | ----------------------------------- | ----------------------------------- |
| `tcp.rs:261`     | `send.iv_seqno` increment           | included in 1.2 fields              |
| `tcp.rs:340`     | `recv_iv_seqno` increment           | included in 2.2 fields              |
| `udp.rs:270-271` | hello-path seqno increments         | included in 3.1 fields              |
| `udp.rs:423-424` | playerstate seqno increments        | included in 3.2 fields              |
| `udp.rs:540`     | recv-path `recv_iv_seqno` increment | new `relay.udp.iv.seqno` at `trace` |
| `capture.rs:210` | `dropped_count` increment           | included in 7.1                     |
| `session.rs:316` | refresh-delay computation           | included in 6.3 `refresh.fire`      |
| `session.rs:356` | re-login `attempt` increment        | included in 6.3 `relogin_attempt`   |
| `udp.rs:300-307` | sync sample accumulation            | included in 4.1 `hello.ack`         |

## Cross-cutting work needed before the events are useful

### `--debug` channel

- **`--debug` must enable `debug` for the underlying crates.** Today
  it likely only filters on the daemon target. Inspect
  `src/daemon/runtime.rs` filter setup and adjust so that
  `zwift_relay=debug,zwift_api=debug` is included when `--debug` is on.
- **The recv loop must handle `TcpChannelEvent::Inbound`.** Without
  this arm, item 2.3 has no emission point in the daemon process.
- **The daemon must subscribe to `SessionEvent`.** Without this
  subscription, items 6.3 cannot be emitted from the daemon. (Tracked
  separately as Defect 7; the logging step depends on it but does not
  need to land it.)
- **`message_kind` field naming should match the proto-decoded
  discriminant** so a future `ranchero follow` can join `--debug`
  events to `--capture` records by message type. Confirm the
  discriminant naming before implementation.

### `--capture` channel

- **HTTP exchanges must reach the capture file (item 5.0).** The
  capture file today is incomplete by the rule stated at the top of
  this document. Adding HTTP coverage requires a new `TransportKind`
  variant and call sites in `crates/zwift-api/src/lib.rs` for every
  request and response body.
- **The completeness rule needs a guard.** As proposed in 7.A, either
  add a custom lint preventing transport send/recv calls without an
  adjacent `record_*` call, or maintain a per-PR checklist. Without a
  guard, the next addition of a wire-touching code path will silently
  break completeness again.
- **The `relay.capture.*` tracing namespace describes the writer, not
  the file content.** Reviewers should flag any proposal that adds
  human prose, decoded fields, or named events to the `--capture`
  byte stream — those belong on `--debug`.

## What this document is not

- It is not the implementation step. The implementation step (open as
  STEP-12.14 or similar) will pick a level of granularity, write
  failing tests that assert the event names appear under the right
  conditions, and add the emissions.
- It is not a commitment to add every event at every level today. It
  is a list of every place the code is silent today, with a default
  recommendation. The implementation step is free to defer the
  highest-volume `trace`-level entries if the cost of carrying them
  through the codebase outweighs the diagnostic value.

## Acceptance for the eventual implementation step

When this work is implemented, a successful

```
ranchero start --debug --capture output.cap
```

run must satisfy both of the following.

### `--debug` channel

The daemon log should include, in order, at minimum:

```
relay.capture.opened
relay.auth.token.requested
relay.auth.token.granted
relay.auth.profile.fetched
relay.login.ok
relay.session.login.started
relay.session.login.ok
relay.tcp.connecting
relay.tcp.established
relay.tcp.hello.sent
relay.udp.hello.started
relay.udp.hello.sent  (one or more, debug)
relay.udp.hello.ack   (one or more, debug)
relay.udp.sync.converged
relay.udp.established
relay.heartbeat.started
relay.heartbeat.tick  (one per second, debug)
relay.tcp.frame.recv  (debug, as frames arrive)
relay.tcp.message.recv (debug, per decoded ServerToClient)
relay.udp.message.recv (debug, per decoded ServerToClient)
```

A failure run should make the failing step obvious from the last
event emitted, without requiring the operator to read source code to
guess what was meant to happen next.

### `--capture` channel

The capture file (`output.cap`) is a wire-bytes container at format
version 2. It must contain:

- a session-manifest record written immediately after the file
  header, carrying the AES key, the device / channel discriminants
  used in the IV, the starting `iv_seqno` for both directions on
  each transport, the `relay_id`, the `conn_id`, and the relay-
  session expiration;
- a fresh manifest record after every successful supervisor refresh
  or re-login that rotates key material;
- every authenticated HTTP request and response body issued by
  `zwift-api` (token grant, refresh, profile, relay-session login,
  relay-session refresh) — direction inbound/outbound, transport
  `Http`. HTTP bodies are stored as transmitted; no manifest entry
  is needed because the application-layer encryption above this
  point is TLS, which terminates inside `reqwest`;
- every TCP frame as it appeared on the wire (length prefix +
  header + ciphertext + tag), direction inbound/outbound, transport
  `Tcp`;
- every UDP datagram as it appeared on the wire (unencrypted header
  - ciphertext + tag), direction inbound/outbound, transport `Udp`.

No record may contain decoded fields, event names, log levels, or
human-readable prose. The only annotation alongside payload bytes is
the framed-record header (timestamp, direction, transport kind, plus
the existing `hello` flag where it remains protocol-meaningful).

The round-trip property must hold: a capture from a successful run,
fed through `CaptureReader` plus the manifest, must decrypt and
decode back to the same `ClientToServer` and `ServerToClient`
sequences observed during the run. A failure-run capture must be
readable up to the moment of failure, with the framed-record count
from `relay.capture.writer.closed { total_records, total_bytes }`
matching the count produced by reading the file back through
`CaptureReader`.

## Implementation phasing

Eight phases. Each phase is a tests-first / then-implement pair
(`Na` writes failing tests; `Nb` makes them pass and adds nothing
beyond what the tests demand). The pairs are listed in the summary
checklist at the top of this file; the detail below names the
specific test cases, fixtures, and edits expected of each pair.

Run order: 0 must precede 1–6. Phases 1, 2, 3, and 4 are independent
and may be tackled in parallel by separate contributors if desired.
Phase 5 depends on phase 0 only (it needs the `Http` transport
variant and the manifest API). Phase 6 depends on phases 1–5 (it
writes manifests and consumes the events those layers emit).
Phase 7 depends on everything and includes the live-traffic
verification.

### Phase 0 — Pre-requisites

#### 0a — Pre-requisites tests (write first, expect red)

Tests live in `crates/zwift-relay/tests/capture.rs` and a new
`tests/debug_filter.rs` under the daemon crate.

- `capture_format_v2_round_trip_writes_and_reads_manifest_then_frames`
  — open a writer, call `record_session_manifest(...)` once with
  representative key + IV state + ids, then push two frame records
  (one TCP, one UDP); close; read back via `CaptureReader` and
  assert the iterator yields the manifest record first followed by
  the two frame records, all fields preserved.
- `capture_reader_rejects_v1_file_with_clear_error` — write a
  hand-crafted v1 header followed by zero records; assert
  `CaptureReader::new(...)` returns `CaptureError::UnsupportedVersion(1)`
  with the expected message text.
- `capture_record_supports_http_transport_kind` — round-trip a
  record with `TransportKind::Http`; assert it serialises and
  deserialises cleanly.
- `record_session_manifest_can_be_called_again_after_rotation` —
  push manifest, frame, manifest, frame; assert reader yields all
  four in order.
- `debug_flag_enables_debug_for_underlying_crates` — under a
  `tracing-test` (or equivalent) subscriber, build the `EnvFilter`
  the daemon uses when `--debug` is set; assert it admits events at
  `debug` for `ranchero::*`, `zwift_relay::*`, and `zwift_api::*`.

#### 0b — Pre-requisites implementation

- `crates/zwift-relay/src/capture.rs`:
  - bump `FORMAT_VERSION` constant to `2`.
  - add `RecordKind` discriminant byte (or expand the existing
    transport byte) to distinguish `Manifest` from `Frame`.
  - add `TransportKind::Http`.
  - add `pub struct SessionManifest { aes_key: [u8; 16],
device: DeviceType, channel: ChannelType, send_iv_seqno_tcp:
u32, recv_iv_seqno_tcp: u32, send_iv_seqno_udp: u32,
recv_iv_seqno_udp: u32, relay_id: u32, conn_id: u32,
expires_at_unix_ns: u64 }` (field set adjusted to match the
    real codec; verify against `RelayIv`).
  - add `pub fn record_session_manifest(&self, m: SessionManifest)`
    on `CaptureWriter`.
  - update `CaptureReader` to surface manifest records as a distinct
    iterator-item variant.
  - update `CaptureRecord` enum (or add a wrapper) so a reader can
    pattern-match `Manifest(SessionManifest)` vs `Frame(FrameRecord)`.
- `src/daemon/runtime.rs`: locate the `tracing_subscriber` /
  `EnvFilter` setup; under `--debug`, append directives so
  `zwift_relay=debug` and `zwift_api=debug` are admitted.

### Phase 1 — TCP module (zwift-relay)

#### 1a — TCP module tests

Add to `crates/zwift-relay/tests/tcp.rs` (or a new
`tests/tcp_capture.rs` if the existing file is unwieldy).

- `tcp_send_records_framed_wire_bytes_not_proto` — drive
  `TcpChannel::send_packet(payload, hello=true)` via the existing
  in-memory transport fixture with a capture writer attached; read
  the recorded record back; assert the bytes equal the framed wire
  produced by `frame_tcp(...)` (length prefix + header + ciphertext
  - tag), **not** `payload.encode_to_vec()`.
- `tcp_recv_records_pre_decrypt_buffer` — feed a known
  framed-and-encrypted payload into the channel's read side; assert
  the recorded record holds the raw framed wire bytes (matching
  what `next_tcp_frame` would have drained), not the post-decrypt
  plaintext.
- `tcp_send_emits_relay_tcp_frame_sent_with_required_fields` —
  using a `tracing-test` subscriber, drive a send and assert a
  `relay.tcp.frame.sent` event is emitted at `debug` with fields
  `seqno`, `iv_seqno`, `hello`, `wire_size` populated.
- `tcp_recv_emits_relay_tcp_frame_recv_with_required_fields` —
  drive an inbound frame; assert `relay.tcp.frame.recv` at `debug`
  with `size`, `seqno`, `relay_id_present`, `conn_id_present`.
- `tcp_recv_emits_relay_tcp_decrypt_ok_at_trace_level` — same
  fixture as above but with a `trace`-level subscriber; assert
  `relay.tcp.decrypt.ok` carries `seqno`, `relay_id`, `conn_id`.

#### 1b — TCP module implementation

- `crates/zwift-relay/src/tcp.rs:225-273`: move the
  `record_outbound` call from line 234 to immediately before the
  `transport.write_all(&wire)` call at line 271, passing `&wire`
  (not `&proto_bytes`).
- `crates/zwift-relay/src/tcp.rs:386-405`: move the
  `record_inbound` call to run on `payload_owned` immediately after
  the buffer drain at line 391 and before `process_inbound`. The
  decrypted-plaintext path no longer captures.
- Add the three tracing emissions in the same module. The send
  event sits inside the `send_packet` body after the wire is
  built; the recv events sit alongside the frame extraction and
  decrypt sites in the recv loop.

### Phase 2 — UDP module (zwift-relay)

#### 2a — UDP module tests

Add to `crates/zwift-relay/tests/udp.rs`. The existing UDP test
fixtures already simulate hello and steady-state.

- `udp_hello_send_records_encrypted_datagram` — run one hello
  iteration; assert the recorded outbound bytes equal the encrypted
  datagram (header + ciphertext + tag), not the protobuf payload.
- `udp_hello_recv_records_raw_datagram_pre_decrypt` — same for the
  inbound side.
- `udp_steady_state_send_records_encrypted_datagram` — drive
  `send_player_state`; assert as above.
- `udp_steady_state_recv_records_raw_datagram_pre_decrypt` — drive
  `recv_loop` with a synthetic datagram; assert as above.
- `udp_hello_emits_started_and_per_attempt_sent_events` — assert a
  one-shot `relay.udp.hello.started` at `info` with the target
  address, then one `relay.udp.hello.sent` per attempt at `debug`
  with `hello_idx`, `app_seqno`, `iv_seqno`, `payload_size`.
- `udp_hello_recv_emits_ack_per_response_and_one_converged_event`
  — assert `relay.udp.hello.ack` at `debug` per ack with
  `app_seqno`, `latency_ms`, `offset_ms`; then one
  `relay.udp.sync.converged` at `info` with `mean_offset_ms`,
  `median_latency_ms`, `sample_count`.
- `udp_steady_state_send_emits_relay_udp_playerstate_sent` —
  assert `relay.udp.playerstate.sent` at `debug` with `world_time`,
  `app_seqno`, `iv_seqno`, `payload_size`.
- `udp_steady_state_recv_emits_relay_udp_message_recv_with_fields`
  — assert the bare `relay.udp.inbound` is gone; the new event
  `relay.udp.message.recv` at `debug` carries `message_kind`,
  `world_time`, `player_count`, `payload_size`.

#### 2b — UDP module implementation

- `crates/zwift-relay/src/udp.rs:239-271`: relocate the
  `record_outbound` at line 245 to write `wire` (the encrypted
  datagram built before `transport.send` at line 268).
- `crates/zwift-relay/src/udp.rs:273-322`: relocate the
  `record_inbound` at line 293 to run on the raw datagram returned
  from `transport.recv()` at line 284, before
  `process_inbound_packet`.
- `crates/zwift-relay/src/udp.rs:388-433`: relocate the
  `record_outbound` at line 403 to write the encrypted `wire` value
  built before `transport.send` at line 431.
- `crates/zwift-relay/src/udp.rs:544-603`: relocate the
  `record_inbound` at line 574 to run on the raw datagram returned
  from `transport.recv()` at line 563, before
  `process_inbound_packet`.
- Add the six tracing emissions in the same module. The bare
  `relay.udp.inbound` event in `src/daemon/relay.rs` is removed by
  phase 6.

### Phase 3 — Capture writer diagnostics (zwift-relay)

#### 3a — Capture writer diagnostics tests

Extend `crates/zwift-relay/tests/capture.rs`.

- `capture_record_drop_emits_warn_event_with_total_dropped` — open
  a writer with a tiny channel capacity; flood records faster than
  the writer task drains; assert at least one
  `relay.capture.record.dropped` warn event with fields
  `direction`, `transport`, `payload_size`, `total_dropped`.
- `capture_writer_flush_emits_debug_event_with_record_and_byte_counts`
  — write N records, force a flush boundary; assert
  `relay.capture.writer.flushed` at `debug` with
  `records_in_batch`, `bytes_written`.
- `capture_writer_close_emits_info_event_with_totals` — write N
  records then close; assert `relay.capture.writer.closed` at
  `info` with `total_records`, `total_bytes` matching the produced
  count.

#### 3b — Capture writer diagnostics implementation

- `crates/zwift-relay/src/capture.rs:203-221`: emit the dropped
  event from the producer-side `record` path when the mpsc returns
  `Full` or `Closed`.
- `crates/zwift-relay/src/capture.rs:254-277`: emit the flushed
  event from the writer task at each flush boundary, and the
  closed event from the shutdown path.

### Phase 4 — Session and supervisor (zwift-relay)

#### 4a — Session and supervisor tests

Extend `crates/zwift-relay/tests/session.rs`.

- `session_login_emits_started_and_ok_events` — drive `login`
  against the wiremock fixture; assert `relay.session.login.started`
  at `info` with `athlete_id`, then `relay.session.login.ok` at
  `info` with `relay_id`, `tcp_server_count`, `server_time_ms`,
  `expiration_min`.
- `session_login_emits_tcp_servers_at_debug` — assert
  `relay.session.tcp_servers` at `debug` with the comma-joined
  IP list.
- `session_refresh_emits_ok_event` — drive `refresh`; assert
  `relay.session.refresh.ok` at `info` with `relay_id`,
  `new_expiration_min`.
- `supervisor_loggedin_event_emits_supervisor_logged_in_trace` —
  drive the supervisor `start`; assert
  `relay.supervisor.logged_in` at `info`.
- `supervisor_refresh_fire_emits_scheduled_delay_event` — assert
  `relay.supervisor.refresh.fire` at `info` with
  `scheduled_delay_ms`, `relay_id` before the HTTP call.
- `supervisor_refresh_failure_path_emits_refresh_failed_and_relogin_attempt`
  — induce a refresh failure; assert `relay.supervisor.refresh_failed`
  at `warn`, then `relay.supervisor.relogin_attempt` at `info` with
  `attempt`, `backoff_ms`.
- `supervisor_relogin_success_emits_relogin_ok` — induce one
  failed re-login then a successful one; assert
  `relay.supervisor.relogin_ok` at `info` with `attempt`.
- `supervisor_persistent_login_failure_emits_login_failed_warn` —
  induce N failures; assert `relay.supervisor.login_failed` at
  `warn` per attempt with `attempt`, `error`, `backoff_next_ms`.

#### 4b — Session and supervisor implementation

- `crates/zwift-relay/src/session.rs:137-190`: emit started, ok,
  and tcp_servers events around the existing flow.
- `crates/zwift-relay/src/session.rs:194-221`: emit refresh.ok.
- `crates/zwift-relay/src/session.rs:245-372`: emit the supervisor
  events on the relevant code paths inside `start` and
  `refresh_loop`. Compute `scheduled_delay_ms` once and emit
  `refresh.fire` before the HTTP call. Emit `relogin_attempt` on
  each iteration of the re-login loop and `relogin_ok` on the
  iteration that succeeds.

### Phase 5 — Auth / HTTP (zwift-api)

This phase introduces a new cross-crate dependency: `zwift-api`
needs to write into the capture file owned by `zwift-relay`.
Resolve by extracting a small `CaptureSink` trait into a shared
location both crates can depend on (`zwift-relay::capture` is
already a viable home — `zwift-api` can take a feature-gated dep
on `zwift-relay` purely for the trait, or both can depend on a new
`zwift-capture-types` micro-crate). Pick one in 5b; the tests in
5a do not constrain the choice as long as the trait is callable.

#### 5a — Auth / HTTP tests

Extend `crates/zwift-api/tests/auth.rs`.

- `login_request_body_appears_in_capture_as_http_outbound` —
  inject a `CaptureSink` recorder into `ZwiftAuth`; drive `login`;
  assert one outbound `Http` record holding the request body
  bytes verbatim.
- `login_response_body_appears_in_capture_as_http_inbound` —
  assert one inbound `Http` record holding the response body
  bytes verbatim.
- `profile_fetch_request_and_response_appear_in_capture` — same
  for `get_profile_me`.
- `authenticated_post_and_get_paths_record_request_and_response` —
  drive a manual `post` and `fetch`; assert each produces an
  outbound + inbound pair.
- `auth_emits_token_requested_and_granted_events` — assert
  `relay.auth.token.requested` at `info` with `username`,
  `grant_type`, then `relay.auth.token.granted` at `info` with
  `expires_in_s`, `refresh_expires_in_s`.
- `auth_emits_profile_ok_on_success_and_profile_failed_on_error` —
  assert `relay.auth.profile.ok` at `debug` (success) and
  `relay.auth.profile.failed` at `warn` with status + variant
  (each typed-error path).
- `auth_emits_http_request_and_response_at_debug` — assert
  `relay.auth.http.request` and `relay.auth.http.response` events
  at `debug` for both `post` and `fetch`.
- `auth_emits_http_retry_event_on_401_path` — drive the inline
  401-retry path; assert `relay.auth.http.retry` at `info` fires.
- `auth_emits_refresh_completed_event` — drive a background
  refresh; assert `relay.auth.refresh.completed` at `info` with
  `expires_in_s`, `next_refresh_in_s`.

#### 5b — Auth / HTTP implementation

- Choose the cross-crate sharing approach (trait location). Add
  the `CaptureSink` trait with a single `record(direction,
transport, &[u8])` method.
- `crates/zwift-api/src/lib.rs`: add an optional
  `Arc<dyn CaptureSink>` field to `Inner`; thread it into
  `with_client` / `Config` so the daemon can supply one. Default
  is `None` (no capture).
- Wrap each request and response body with two sink calls
  (outbound for the request body once it has been built, inbound
  for the response body once it has been read).
- Add the eight tracing emissions at the sites named in 5a.

### Phase 6 — Daemon integration (ranchero)

#### 6a — Daemon integration tests

Extend `tests/relay_runtime.rs` (or a new
`tests/daemon_logging.rs` if the existing file is heavy).

- `start_all_inner_writes_session_manifest_after_session_login` —
  drive `start_all_inner` with a recording `CaptureSink`; assert
  the first non-header record is a `Manifest` with the AES key,
  IV seeds, `relay_id`, `conn_id`, and expiration drawn from the
  fixture session.
- `supervisor_refresh_writes_fresh_manifest_when_key_rotates` —
  inject a supervisor `Refreshed` event with new key material;
  assert a second `Manifest` record is appended.
- `recv_loop_handles_tcp_inbound_and_emits_relay_tcp_message_recv`
  — inject a synthetic `TcpChannelEvent::Inbound(stc)`; assert
  `relay.tcp.message.recv` at `debug` with `message_kind`,
  `seqno`, `has_state_change`, `has_world_info`.
- `state_change_emissions_track_runtime_state_transitions` — drive
  a successful start; assert one `relay.state.change` info event
  per `RuntimeState` transition with `from`, `to` discriminant
  names.
- `heartbeat_tick_emits_debug_event_per_interval` — drive the
  heartbeat scheduler with a tiny tick interval; assert
  `relay.heartbeat.tick` at `debug` with `interval_ms`, `send_ok`
  per fire.
- `heartbeat_send_failure_emits_warn` — inject a sink that fails;
  assert `relay.heartbeat.send_failed` at `warn` carries the
  underlying error message.

#### 6b — Daemon integration implementation

- `src/daemon/relay.rs:start_all_inner` (around line 925): after
  the relay-session login completes (around step 4-6), call
  `capture_writer.record_session_manifest(...)` with the live
  session's key and IV state. Pull the values from the existing
  `RelaySession` struct (extend it if a needed field is not
  exposed).
- The supervisor-event handler (`supervisor_event_abort` task,
  around relay.rs:1100): on `Refreshed` and successful re-login
  events, call `record_session_manifest` again with the new key
  material.
- `src/daemon/relay.rs::recv_loop` (around lines 1700-1750): add
  the missing `TcpChannelEvent::Inbound(stc)` arm; emit
  `relay.tcp.message.recv` with fields drawn from the decoded
  `ServerToClient`.
- Wherever `GameEvent::StateChange(state)` is broadcast: emit a
  matching `relay.state.change` info event with `from` (previous
  state, tracked locally) and `to` (the new state).
- `src/daemon/relay.rs::HeartbeatScheduler::run` (around lines
  361-380): emit `relay.heartbeat.tick` per fire with
  `interval_ms` and `send_ok`. On failure, emit
  `relay.heartbeat.send_failed` at `warn` with the error.
- Remove the bare `tracing::debug!(target: "ranchero::relay",
"relay.udp.inbound");` at relay.rs:1738 (replaced by
  `relay.udp.message.recv` from phase 2).

### Phase 7 — Closing review

#### 7a — Closing review verification

Verification, not new test code:

- Run `cargo test --workspace`; assert clean.
- Run `ranchero start --debug --capture output.cap` against live
  Zwift; capture the trace; assert it contains, in order, the full
  event sequence in the **Acceptance — `--debug` channel** section
  above.
- With the same `output.cap`, write a small one-shot binary (or
  reuse `ranchero replay`) that loads the manifest record,
  decrypts the recorded TCP and UDP frames using the manifest key
  and IV state, and `prost::decode`s the result; assert the first
  frame from each transport decodes successfully.
- Audit: `Grep` for `transport.send`, `transport.write_all`, and
  `transport.recv` across `crates/zwift-relay/src` and
  `crates/zwift-api/src`; confirm every match has an adjacent
  `record_outbound` / `record_inbound` / `CaptureSink::record`
  call. Document any exceptions (and explain why they are not
  defects).
- Audit: open `output.cap` in a hex viewer; assert the first
  record after the file header is a manifest record (kind byte
  matching the new `RecordKind::Manifest` discriminant), then
  framed wire bytes follow (the first TCP frame should begin with
  the 2-byte length prefix and have a header that matches the
  documented flags).

#### 7b — Closing review cleanup

- Delete `Defect 4` and `Defect 7` doc comments in `relay.rs`
  that this work fully resolves.
- Delete any TODOs in the touched modules that this work closes.
- If any plan-doc reference inside the touched files (search for
  `STEP-1` patterns in code) points at stale line numbers, refresh
  them.
- Update STEP-12.13 (or the implementation step's actual filename)
  with the final per-phase status as you go; this STEP-12.12
  document does not need further edits — it is the contract, not
  the worklog.
