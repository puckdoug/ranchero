# Step 09 — Relay session (login + refresh supervisor)

**Status:** planned (2026-04-27).

## Goal

Implement the HTTPS handshake described in spec §4.1 / §7.6:

1. Generate a 16-byte AES key with `OsRng` (the *session* key, used by
   STEP 08's codec for every subsequent TCP/UDP packet).
2. POST `LoginRequest { key }` to
   `https://us-or-rly101.zwift.com/api/users/login` with
   `Content-Type: application/x-protobuf-lite` and a bearer token from
   `zwift_api::ZwiftAuth`.
3. Decode `LoginResponse`; persist `relay_session_id`, `aes_key`, the
   filtered TCP server list, `expiration`, `logged_in_at`,
   `server_time_ms` (for STEP 12 clock sync).
4. Background **supervisor task** refreshes at
   `logged_in_at + 0.9 × expiration` via `/relay/session/refresh`;
   on refresh failure, attempts a full re-login; on re-login failure,
   surfaces a typed event and backs off.

This is the last piece before STEP 10 / 11 (UDP / TCP channels) can be
written: those channels need `aes_key`, `relay_id`, and a server IP,
all of which come from here.

## Scope

**In scope**:

- One-shot async functions `login(...)` and `refresh(...)`.
- A `RelaySession` POD that holds everything channels need.
- `RelaySessionSupervisor`: long-running task that owns the periodic
  refresh, exposes the current session via a snapshot accessor, emits
  lifecycle events on a `tokio::sync::broadcast` channel.
- TCP server list filtering (`lb_realm == 0 && lb_course == 0`).
- The server-time field plumbed through (used by STEP 12 for clock
  sync; STEP 09 just exposes it).
- Extension to `zwift_api::ZwiftAuth` to support
  `POST` with a body (the existing `fetch()` is GET-only).

**Out of scope**:

| Concern | Where it lives |
|---|---|
| Opening TCP/UDP sockets to the chosen server | STEP 10 (UDP), STEP 11 (TCP) |
| World-time clock / SNTP-style sync (`worldTimer.adjustOffset`) | STEP 10 (UDP handshake), STEP 12 |
| Per-course UDP server pool selection (`UdpConfigVod`) | STEP 12 (`GameMonitor`) |
| `_lastTCPServer` "stick to the same server" logic | STEP 11 (channel reconnect policy) |
| Watching/leaving worlds, `/relay/worlds/.../leave`, `/api/users/logout` | Later (probably STEP 12) |
| The `await sleep(1000)` after login (sauce comment: "No joke this is required") | This step — see "Open verification points" §4 |

## Crate layout

`zwift-relay` already exists from STEP 08 with codec modules. We add
the session module next to them — no new crate, no feature gate (the
codec is still `no_std`-clean in source even though the crate as a
whole now depends on tokio + reqwest).

```
crates/zwift-relay/
├── Cargo.toml          — extended deps below
├── src/
│   ├── lib.rs          — re-exports (codec + session)
│   ├── consts.rs       (existing)
│   ├── iv.rs           (existing)
│   ├── header.rs       (existing)
│   ├── crypto.rs       (existing)
│   ├── frame.rs        (existing)
│   └── session.rs      ← NEW
└── tests/
    ├── iv.rs           (existing)
    ├── header.rs       (existing)
    ├── crypto.rs       (existing)
    ├── frame.rs        (existing)
    └── session.rs      ← NEW (wiremock-driven, like zwift-api/tests/auth.rs)
```

Tooling: STEP 09's tests follow the same wiremock pattern STEP 07
established for `zwift-api`. Both `auth_base` and `api_base` (and
ranchero's relay host) get pointed at the same `MockServer::uri()`
because routing happens by URN.

## Dependencies

`crates/zwift-relay/Cargo.toml` gains:

```toml
[dependencies]
# (existing: aes, bitflags, ghash, subtle, thiserror)
prost   = "0.13"                            # encode LoginRequest, decode LoginResponse
rand    = { version = "0.8", default-features = false, features = ["std", "std_rng", "getrandom"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
tokio   = { version = "1", features = ["sync", "time", "rt", "macros"] }
tracing = "0.1"
zwift-api   = { path = "../zwift-api" }
zwift-proto = { path = "../zwift-proto" }   # was dev-dep; promote to runtime

[dev-dependencies]
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
tokio      = { version = "1", features = ["sync", "time", "macros", "rt", "rt-multi-thread", "test-util"] }
wiremock   = "0.6"
```

`hex-literal` stays a dev-dep (used by codec tests). The `zwift-proto`
dep moves from `dev-dependencies` → `[dependencies]`.

## Public API surface (proposed)

### Constants (extend `consts.rs`)

```rust
pub const LOGIN_PATH:           &str = "/api/users/login";
pub const SESSION_REFRESH_PATH: &str = "/relay/session/refresh";

/// Refresh fires at this fraction of the session's announced lifetime.
/// Matches `zwift.mjs:1926` (`refreshDelay = (expires - now) * 0.90`).
pub const SESSION_REFRESH_FRACTION: f64 = 0.90;

/// Lower bound on refresh attempt cadence (back-off floor on
/// repeated failures). Spec §7.4.
pub const MIN_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(3);
```

### `RelaySession` (the POD that channels consume)

```rust
pub struct TcpServer {
    pub ip:   String,
    pub port: u16,
}

pub struct RelaySession {
    /// Client-chosen 16-byte AES key. Used by `zwift_relay::encrypt`
    /// / `decrypt` for every TCP/UDP packet.
    pub aes_key: [u8; 16],
    /// `LoginResponse.relay_session_id`, used as the IV's `relayId`
    /// component and as the `relaySessionId` field in subsequent
    /// `RelaySessionRefreshRequest` bodies.
    pub relay_id: u32,
    /// Filtered to `lb_realm == 0 && lb_course == 0`.
    pub tcp_servers: Vec<TcpServer>,
    /// `Instant` when the supervisor must (at the latest) have
    /// refreshed by. Computed as `logged_in_at + expiration`.
    pub expires_at: tokio::time::Instant,
    /// `LoginResponse.info.time` in milliseconds — the server's wall
    /// clock at login time. STEP 12's `WorldTimer` uses this for
    /// initial clock alignment; STEP 09 just plumbs it through.
    pub server_time_ms: Option<u64>,
}
```

### Single-shot async functions

```rust
pub struct RelaySessionConfig {
    /// Base URL (with scheme). Production default
    /// `https://us-or-rly101.zwift.com`. Mock servers inject their
    /// own URI here (pattern shared with `zwift_api::Config`).
    pub api_base: String,
    pub source:        String,  // header value; default "Game Client"
    pub user_agent:    String,  // header value; default "CNL/4.2.0"
    pub min_refresh_interval: std::time::Duration,
}

impl Default for RelaySessionConfig { /* … */ }

pub async fn login(
    auth: &zwift_api::ZwiftAuth,
    config: &RelaySessionConfig,
) -> Result<RelaySession, Error>;

/// Returns the new `expiration` (minutes) on success.
pub async fn refresh(
    auth: &zwift_api::ZwiftAuth,
    config: &RelaySessionConfig,
    relay_id: u32,
) -> Result<u32, Error>;
```

### Supervisor (long-running)

```rust
pub struct RelaySessionSupervisor { /* private */ }

impl RelaySessionSupervisor {
    /// Performs the initial login synchronously, then spawns a
    /// background task that drives subsequent refreshes.
    pub async fn start(
        auth: zwift_api::ZwiftAuth,
        config: RelaySessionConfig,
    ) -> Result<Self, Error>;

    /// Cheap snapshot of the currently-active session.
    pub async fn current(&self) -> RelaySession;

    /// Subscribe to lifecycle events.
    pub fn events(&self) -> tokio::sync::broadcast::Receiver<SessionEvent>;

    /// Cancels the background refresh task. The current session
    /// snapshot remains readable through `current()` until the
    /// supervisor is dropped.
    pub fn shutdown(&self);
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Initial login succeeded.
    LoggedIn(RelaySession),
    /// Periodic refresh extended the existing session.
    Refreshed { relay_id: u32, new_expires_at: tokio::time::Instant },
    /// `/relay/session/refresh` failed; the supervisor will fall
    /// back to a full re-login.
    RefreshFailed(String),
    /// Re-login after a refresh failure also failed; the supervisor
    /// is backing off and will retry with `MIN_REFRESH_INTERVAL`
    /// growing per attempt.
    LoginFailed { attempt: u32, error: String },
}
```

### Errors

```rust
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("auth error: {0}")]
    Auth(#[from] zwift_api::Error),

    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },

    #[error("LoginResponse missing required field: {0}")]
    MissingField(&'static str),

    #[error("protobuf decode error: {0}")]
    Decode(#[from] prost::DecodeError),
}
```

## Required upstream change: extend `ZwiftAuth` for POST-with-body

`zwift_api::ZwiftAuth::fetch()` is GET-only (per STEP 07). The relay
endpoints both need POST with a protobuf body. STEP 09 adds:

```rust
impl ZwiftAuth {
    /// POST `body` to `{api_base}{urn}` with the supplied
    /// `Content-Type`. Includes `Authorization`, `Source`,
    /// `User-Agent` headers exactly like `fetch()`. On 401, refreshes
    /// the auth token inline and retries once (same shape as
    /// `fetch()`'s 401-retry).
    pub async fn post(
        &self, urn: &str, content_type: &str, body: Vec<u8>,
    ) -> Result<reqwest::Response, Error>;
}
```

Why on `ZwiftAuth` and not in this crate's session module:

- The auth + Source + User-Agent + 401-retry pipeline is already there
  for `fetch()`. Re-implementing it in `zwift-relay` would duplicate
  five lines of logic and silently drift if the channel-pool refresh
  rules change.
- Keeping `ZwiftAuth` ignorant of prost (it stays at the `Vec<u8>`
  layer) preserves the boundary the STEP 07 plan deliberately drew.

This adds an `authed_post_includes_bearer_source_and_user_agent_headers`
test to `crates/zwift-api/tests/auth.rs` (mirrors the existing GET
counterpart). Update STEP 07's as-built doc when this lands.

## Tests-first plan

All tests are wiremock-driven, in `crates/zwift-relay/tests/session.rs`.
Both the auth host and the relay host point at the same
`MockServer::uri()`. The auth path (`/auth/realms/zwift/.../token`)
returns a fixed access token; subsequent relay paths assert the
bearer header.

| Test | Asserts |
|---|---|
| `login_posts_protobuf_login_request_with_correct_body` | The request to `/api/users/login` carries `Content-Type: application/x-protobuf-lite` and a `LoginRequest` whose `key` field is exactly the 16 bytes the client generated. The mock captures the body, decodes it as a `LoginRequest`, and the test asserts on the captured value. |
| `login_response_decodes_into_relay_session` | A canned `LoginResponse` (built in-test) round-trips through encode → wiremock → decode → `RelaySession`. Assert `relay_id`, `tcp_servers`, `expires_at` derived from `expiration`. |
| `login_filters_tcp_servers_to_realm_zero_course_zero` | A `LoginResponse` with three `TcpAddress` entries (`(0,0)`, `(0,5)`, `(1,0)`). Only the first is in `RelaySession.tcp_servers`. |
| `login_includes_bearer_from_zwift_auth` | The `/api/users/login` request carries `Authorization: Bearer <access_token>` from a pre-logged-in `ZwiftAuth` instance. |
| `login_surfaces_http_error_status` | A 500 from the login endpoint produces `Error::Status { status: 500, … }`. |
| `login_surfaces_protobuf_decode_error` | A 200 response with body `b"not-a-protobuf"` produces `Error::Decode`. |
| `login_surfaces_missing_required_field` | A `LoginResponse` missing `relay_session_id` produces `Error::MissingField("relay_session_id")`. |
| `refresh_posts_relay_session_refresh_request_body` | The request to `/relay/session/refresh` carries a hand-encoded body containing only `tag=1, varint relaySessionId` (see "Open verification points" §1). |
| `refresh_returns_new_expiration_minutes` | A canned `RelaySessionRefreshResponse { expiration: 30 }` produces `Ok(30)`. |
| `supervisor_initial_login_emits_logged_in_event` | Subscribing to `events()` then calling `start()` produces a `SessionEvent::LoggedIn` event whose snapshot equals `current().await`. |
| `supervisor_refresh_fires_at_90pct_of_expiration` | Initial login with `expiration: 1` minute (60 s). Refresh endpoint expects exactly 1 hit at `t = 54 s` (real-time wait, ~55 s budget — see "Design decisions" §3). The post-refresh `current()` reflects the new `expires_at`. |
| `supervisor_refresh_failure_triggers_relogin` | First refresh returns 500. Supervisor falls back to a full `/api/users/login` POST. `events()` shows `RefreshFailed` then `LoggedIn`. |
| `supervisor_relogin_failure_emits_login_failed_with_attempt_count` | Both `/api/users/login` and `/relay/session/refresh` return 500. Supervisor emits `LoginFailed { attempt: 1 }` then `LoginFailed { attempt: 2 }` after backoff. |
| `supervisor_shutdown_cancels_pending_refresh` | After `shutdown()`, no refresh request is observed at the next deadline. |

`zwift-proto` is the test oracle for both directions: tests build
`LoginRequest` / `LoginResponse` / `RelaySessionRefreshResponse` via
the generated types, encode them in test setup, and decode them in
assertion code, so the wire shape is exercised without hand-rolled
byte arrays.

## Open verification points

These are claims the implementor should resolve and record in the
as-built doc.

1. **`RelaySessionRefreshRequest` is missing from the vendored
   upstream proto tree.** Sauce's fork has it
   (`sauce4zwift/src/zwift.proto:902-904`); upstream
   `zoffline/zwift-offline` does not. The body shape is trivial —
   one varint field — so two reasonable approaches:

   a. **Hand-encode the body** in this step. The on-the-wire shape
   for `RelaySessionRefreshRequest { relay_session_id: u32 }` is just
   `[tag=0x08, varint relay_session_id_le]`, 2-6 bytes total. The
   plan above takes this approach (no proto vendor change).

   b. **Add a `relay-session-refresh.proto` to the vendored tree** and
   re-run the build. Cleaner, but requires a proto vendor change for
   one trivially-encoded message.

   Decision pending — record which path was taken in the as-built doc.
   If (b), update STEP-06's as-built note about the vendor file list.

2. **Field-name divergence between spec wording and vendored proto.**
   The spec text (§4.1 / §7.6) and sauce's JS use sauce-fork field
   names; the vendored upstream uses different names. Mapping:

   | Spec / sauce | Vendored upstream (`zwift-proto`) |
   |---|---|
   | `LoginRequest.aesKey` | `LoginRequest.key` |
   | `LoginResponse.session` | `LoginResponse.info` |
   | `LoginResponse.session.tcpConfig.servers[]` | `LoginResponse.info.nodes.nodes[]` |
   | `TcpAddress.realm` / `courseId` | `TcpAddress.lb_realm` / `lb_course` |
   | `LoginResponse.session.time` | `LoginResponse.info.time` (Option<u64>) |

   No design choice here — the implementation just uses the vendored
   names. The plan flags this so reviewers reading the spec aren't
   surprised by what they see in code.

3. **Should `LoginResponse.relay_session_id` being `None` be an error?**
   The proto field is `optional`. In practice every successful Zwift
   login carries it. The plan treats `None` as
   `Error::MissingField("relay_session_id")` because downstream
   consumers (channels) would otherwise panic when constructing the
   IV. Confirm this matches observed server behavior or relax to
   "default to 0" if real captures show otherwise.

4. **The `await sleep(1000)` after login.** Sauce's comment at
   `zwift.mjs:1651`: *"No joke this is required (100ms works about
   50% of the time)."* Zwift's relay servers apparently need ~1 s to
   stabilize after login before they'll accept relay traffic. Plan:
   include the sleep in `login()` itself with the sauce comment
   reproduced verbatim above the line, so a future reader who
   "optimizes it away" sees the warning. Verify against captured
   wire data whether 1 s is still required (Zwift's API may have
   improved; might also have gotten worse).

## Design decisions worth pre-committing

- **Single supervisor pattern.** One `RelaySessionSupervisor` owns
  the periodic refresh and the lifecycle events. Channels (STEP
  10/11) are passive consumers — they read `current()` for the
  session snapshot and subscribe to `events()` if they want
  notification. This avoids the JS pattern where `GameMonitor` mixes
  session, channels, and SNTP all in one class
  (`zwift.mjs:1633-1933`).
- **Snapshot via `tokio::sync::RwLock<Arc<RelaySession>>`.** Read
  side is cheap (`current().await` clones an `Arc`). Refresh swaps
  the inner `Arc` atomically. No partial-update races where a reader
  sees `aes_key` from session N and `relay_id` from session N+1.
- **Refresh failure → re-login → backoff.** Failure handling uses an
  exponential backoff floor of `MIN_REFRESH_INTERVAL` × `2^attempt`,
  capped at the original `expiration`. After 5 consecutive failures
  the supervisor still keeps trying but emits a
  `LoginFailed { attempt: ≥ 5 }` event so callers can take
  intervention (alert, surface in TUI, etc.).
- **Real-time waits in tests, not `tokio::time::pause()`.** Same
  rationale as STEP 07's preemptive-refresh test (logged in
  STEP-20 §20.1): paused virtual time + `current_thread` runtime +
  reqwest IO deadlocks because the reactor never gets a turn. The
  one supervisor-schedule test pays a real ~55 s wall-clock cost
  with `expiration: 1` minute. If this turns out to dominate the
  suite, revisit STEP-20 §20.1 and migrate.
- **No CLI surface in this step.** STEP 09 is library code only;
  the daemon will wire it in at STEP 12. The existing `auth-check`
  diagnostic does *not* extend to a "relay-login dry-run" because a
  dry-run would have to actually POST to Zwift's relay host, which
  defeats the no-network guarantee.

## Wiring into the workspace

- `crates/zwift-relay/` already exists; STEP 09 only edits its
  `Cargo.toml` (deps) and adds `src/session.rs` + `tests/session.rs`.
- `crates/zwift-api/` gains a `post()` method on `ZwiftAuth` (see
  "Required upstream change" above) plus a parser/headers test.
- The root `ranchero` crate does **not** need to depend on
  `zwift-relay` yet — that comes at STEP 12 when the daemon's
  `start` command actually orchestrates a session + channels.
- License header `// SPDX-License-Identifier: AGPL-3.0-only` at the
  top of every new `.rs` file.
