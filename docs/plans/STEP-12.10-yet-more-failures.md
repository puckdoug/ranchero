# Step 12.10 — Two failures observed end-to-end after STEP-12.9

**Status:** investigation (2026-05-02).

After STEP-12.9 closed the capture-path-before-fork gaps, the operator
workflow

```
ranchero start --capture session.cap
sleep 10
ranchero follow session.cap
```

was exercised with real monitor credentials. The capture lifecycle now
runs cleanly, but two distinct failures appeared in the daemon log:

```
2026-05-02T07:57:48.623168Z  INFO ranchero::daemon::runtime: ranchero started pid=47600
ranchero started (pid 47600)
2026-05-02T07:57:48.626112Z  INFO ranchero::relay: relay.capture.opened
2026-05-02T07:57:48.920751Z  INFO ranchero::relay: relay.login.ok email="doug+sauce@mhost.com" athlete_id=0
2026-05-02T07:57:50.319435Z  INFO ranchero::relay: relay.tcp.connecting addr=16.146.39.255:3023
2026-05-02T07:57:50.528127Z  INFO ranchero::relay: relay.capture.closed dropped_count=0
2026-05-02T07:57:50.528233Z ERROR ranchero::relay: relay.start.failed error=TCP connect: Connection refused (os error 61)
2026-05-02T07:57:50.528293Z  INFO ranchero::daemon::runtime: ranchero stopped
ranchero stopped
error: I/O error: TCP connect: Connection refused (os error 61)
```

This document captures what was found while investigating both lines and
sets up the work that needs to follow.

## Implementation plan

Two independent items must land for the workflow to complete end-to-end.
Item 1 is the smaller change and unblocks runtime testing of Item 2; do
it first. Each item follows the project's red-then-green TDD discipline:
write the failing test surface, watch it fail, implement, watch it pass.

Reference: the diagnoses behind these decisions live in **Issue A**,
**Issue B**, and **Error type for the profile-fetch path** below — read
those before starting any item if the rationale is unclear.

### Order

1. **Item 1** — Hard-code TCP relay port to 3025; remove the misleading
   `port` field on `TcpServer`.
2. **Item 2** — Fetch `/api/profiles/me` during `ZwiftAuth::login`;
   remove the `AuthLogin::athlete_id` trait default; split
   `zwift_api::Error::AuthFailed` into typed variants.
3. **Out of scope, deferred to a later step** — Sticky TCP server
   selection across reconnects (`_lastTCPServer` in sauce). Track in a
   follow-up plan, not here.

### Item 1 — TCP relay port

#### Pinned decisions

- The TCP listener port is `3025`, matching sauce
  (`sauce4zwift/src/zwift.mjs:1212`). Introduce
  `pub const TCP_PORT_SECURE: u16 = 3025;` next to `UDP_PORT_SECURE`
  in `crates/zwift-relay/src/consts.rs`, re-export from
  `crates/zwift-relay/src/lib.rs`.
- Drop the `port` field from `TcpServer` entirely. Reading the proto
  field at all is what regressed the implementation away from the
  documented constant. Removing the field eliminates the foot-gun and
  surfaces any future regression at compile time.
- `lb_realm == 0 && lb_course == 0` filtering stays — that part already
  matches sauce semantics.

#### Files to touch

- `crates/zwift-relay/src/consts.rs` — add `TCP_PORT_SECURE`
- `crates/zwift-relay/src/lib.rs` — re-export `TCP_PORT_SECURE`
- `crates/zwift-relay/src/session.rs` — drop `port` from `TcpServer`,
  drop the `n.port?` decode, update doc comment on `TcpServer`
- `src/daemon/relay.rs` lines 986-992 and 1240-1245 — replace
  `format!("{}:{}", server.ip, server.port)` with
  `format!("{}:{}", server.ip, zwift_relay::TCP_PORT_SECURE)`
- `src/daemon/relay.rs` test fixtures that construct `TcpServer { ip,
  port }` — remove `port`
- `crates/zwift-relay/src/session.rs` tests — same

#### Red-state tests (write first, watch fail)

- [x] **T1-A** `tcp_connect_uses_constant_port_not_proto_field`. In
  `tests/relay_runtime.rs` (or a new test in
  `crates/zwift-relay/tests/`), build a `LoginResponse` whose first
  node has `port = 3023` and assert the connect address ends in
  `:3025`. Fails today (the connect address ends in `:3023`).

#### Green-state implementation

- [x] **G1-1** Add `pub const TCP_PORT_SECURE: u16 = 3025;` to
  `crates/zwift-relay/src/consts.rs` next to `UDP_PORT_SECURE`. Mirror
  the comment style.
- [x] **G1-2** Re-export `TCP_PORT_SECURE` from
  `crates/zwift-relay/src/lib.rs`.
- [x] **G1-3** Enumerate every `TcpServer { ip, port` literal in the
  workspace (`grep -rn "TcpServer {"`). Confirm the list matches the
  files named under "Files to touch" before proceeding.
- [x] **G1-4** Remove `pub port: u16,` from
  `crates/zwift-relay/src/session.rs::TcpServer`. Remove `n.port?` from
  the `filter_map` decode (it now becomes
  `filter_map(|n| Some(TcpServer { ip: n.ip? }))`). Fix every broken
  literal surfaced in G1-3.
- [x] **G1-5** Update the call sites in `src/daemon/relay.rs:988` and
  `:1242` to use the constant.
- [x] **G1-6** `cargo test` clean.

#### Done when

- T1-A passes (connect goes to `:3025`).
- T1-B no longer applies (no `port` literals remain).
- Re-running the live workflow advances past `connect()` — the failure
  point shifts from `Connection refused` to whatever happens at the
  application layer (likely a malformed-hello rejection driven by
  Item 2's `athlete_id = 0`, until Item 2 lands).

### Item 2 — Athlete identity from `/api/profiles/me`

#### Pinned decisions

- Eager fetch: `ZwiftAuth::login` calls `get_profile_me` as the last
  step before returning `Ok`, matching sauce's
  `authenticate()` (`zwift.mjs:362`). On success the profile is cached
  on `ZwiftAuth`. Any caller that sees `Ok(())` from `login` can rely
  on `athlete_id()` returning a real value without further I/O.
- **Remove `AuthLogin::athlete_id` trait default**. Every implementor
  must override explicitly. This is the whole reason for choosing
  Rust here: catch the placeholder bug at compile time rather than via
  a tracing log on a real account.
- Split `zwift_api::Error::AuthFailed(String)` into four typed
  variants: `AuthFailedUnauthorized` (401), `AuthFailedForbidden`
  (403), `AuthFailedBadSchema` (200 with malformed body), and
  `AuthFailedUnknown` (everything else). `Status { status, body }`
  stays for non-auth HTTP failures.
- The same monitor account flow applies: `monitorAPI` in sauce is just
  a second `ZwiftAPI` instance with the monitor credentials, running
  the identical `authenticate()` body. Confirmed in
  `sauce4zwift/src/main.mjs:111, 660-670`. So one code path covers
  both accounts.

#### Files to touch

- `crates/zwift-api/src/lib.rs` — `Error` variants split, `Profile`
  struct, `get_profile_me`, `athlete_id`, eager profile fetch in
  `login`
- `crates/zwift-api/tests/auth.rs` — wiremock tests for each new
  variant and for the eager profile fetch
- `src/daemon/relay.rs` — remove the `AuthLogin::athlete_id` default,
  add `DefaultAuthLogin::athlete_id` delegating to `ZwiftAuth`
- `src/daemon/relay.rs` test stubs (`StubAuth` at line 1867, plus any
  other `impl AuthLogin`) — explicit `athlete_id` override returning a
  deterministic non-zero value
- Any call site that constructed `Error::AuthFailed(_)` — re-route to
  the right variant
- `RelayRuntimeError::Auth` consumers — confirm error messages on the
  daemon stderr name the failure precisely

#### Red-state tests (write first, watch fail)

- [ ] **T2-A** `login_eager_fetches_profile_and_caches_id`. Wiremock
  serves `200 + token` on the Keycloak endpoint and `200 + {"id":
  12345, ...}` on `GET /api/profiles/me`. After
  `ZwiftAuth::login(...).await?`, `auth.athlete_id().await?` returns
  `12345` without further I/O (use a `.expect(1)` on the wiremock
  profile mock to assert the call count).
- [ ] **T2-B** `get_profile_me_401_returns_unauthorized`. Wiremock
  returns 401; assert the error matches
  `Error::AuthFailedUnauthorized(_)`. Fails to compile until the
  variant exists.
- [ ] **T2-C** `get_profile_me_403_returns_forbidden`. Same as T2-B
  with 403 → `AuthFailedForbidden`.
- [ ] **T2-D** `get_profile_me_200_with_malformed_body_returns_bad_schema`.
  Body is `{}` (no `id` field) → `AuthFailedBadSchema`.
- [ ] **T2-E** `get_profile_me_5xx_returns_unknown`. 503 →
  `AuthFailedUnknown`.
- [ ] **T2-F** `relay_login_log_carries_real_athlete_id`. End-to-end
  assertion through the relay-runtime test surface that the
  `relay.login.ok` log line carries the wired athlete id (e.g. read it
  off the captured tracing events). Fails today (`athlete_id=0`).
- [ ] **T2-G** Structural / build-time: removing the
  `AuthLogin::athlete_id` default produces compile errors for every
  stub that does not override it. List the failing stubs before
  fixing — this is your guide to the test-stub update step.

#### Green-state implementation

- [ ] **G2-1** Split `Error::AuthFailed(String)` into the four typed
  variants. Update every existing constructor.
- [ ] **G2-2** Add `pub struct Profile { pub id: i64, ... }` to
  `crates/zwift-api/src/lib.rs`. Decode at minimum the `id` field;
  optional fields can land later if any consumer needs them.
- [ ] **G2-3** Add `ZwiftAuth::get_profile_me() -> Result<Profile,
  Error>` issuing `GET /api/profiles/me` with the bearer token.
  Mapping table: 200 + valid body → `Ok`; 200 + bad body →
  `AuthFailedBadSchema`; 401 → `AuthFailedUnauthorized`; 403 →
  `AuthFailedForbidden`; everything else → `AuthFailedUnknown`.
- [ ] **G2-4** Cache the profile on `ZwiftAuth` (e.g. `profile:
  RwLock<Option<Profile>>`). Add `ZwiftAuth::athlete_id() -> Result<i64,
  Error>` that reads from the cache and returns
  `Error::NotAuthenticated` if `login` has not been called.
- [ ] **G2-5** Extend `ZwiftAuth::login` so that on success it calls
  `get_profile_me` and stores the result in the cache. If
  `get_profile_me` fails, the whole `login` returns the error (do not
  half-succeed).
- [ ] **G2-6** Remove the default impl on `AuthLogin::athlete_id` in
  `src/daemon/relay.rs:97-101`. The trait method becomes:
  ```rust
  fn athlete_id(&self)
      -> impl std::future::Future<Output = Result<i64, zwift_api::Error>> + Send;
  ```
- [ ] **G2-7** `DefaultAuthLogin::athlete_id` delegates to
  `self.auth.athlete_id().await`.
- [ ] **G2-8** Update every test stub flagged by T2-G with an explicit
  `athlete_id` override returning a deterministic non-zero id (e.g.
  `12345`).
- [ ] **G2-9** `cargo test` clean across all crates.

#### Done when

- All T2-* tests pass.
- The live workflow logs `relay.login.ok email=... athlete_id=<real
  id>` instead of `athlete_id=0`.
- The first hello packet captured into `session.cap` (after Item 1
  also lands) decodes to a `ClientToServer` with
  `player_id = <real id>`.
- A future `impl AuthLogin` for any new type fails to build unless it
  provides an `athlete_id` override.

## Issue A — `athlete_id=0` after a successful login

### What the log records

`relay.login.ok email="doug+sauce@mhost.com" athlete_id=0`. The login
itself succeeded — bearer token was obtained — but the athlete identity
attached to that session is reported as zero.

### Root cause in this repository

`AuthLogin` in `src/daemon/relay.rs:86–102` declares:

```rust
pub trait AuthLogin: Send + Sync + 'static {
    fn login(&self, email: &str, password: &str)
        -> impl std::future::Future<Output = Result<(), zwift_api::Error>> + Send;

    fn athlete_id(&self)
        -> impl std::future::Future<Output = Result<i64, zwift_api::Error>> + Send {
        async { Ok(0i64) }   // ← default implementation
    }
}
```

`DefaultAuthLogin` in `src/daemon/relay.rs:1513–1527` only overrides
`login`; the `athlete_id` default is inherited and always returns
`Ok(0)`. The fetched value flows from `start_inner` (line 957 / 1225)
into:

| Use site | Field |
|---|---|
| `src/daemon/relay.rs:1041` | `ClientToServer { player_id: athlete_id, … }` (TCP hello packet) |
| `src/daemon/relay.rs:1063` | `UdpChannelConfig { athlete_id, … }` |
| `src/daemon/relay.rs:1091` | `HeartbeatScheduler::new(sink, …, athlete_id)` |
| `src/daemon/relay.rs:958, 1226` | `relay.login.ok` log field |

So the tracing log is not the only consumer of the bogus value: every
TCP/UDP packet the daemon would send is keyed off it. Even when the TCP
connection is established, the relay server sees `player_id=0` in the
hello and almost certainly rejects or ignores the session.

### Where `zwift-api` stops

`crates/zwift-api/src/lib.rs` exposes `ZwiftAuth::login`, the bearer
token state, and `fetch`/`post` helpers, but has no method that returns
the authenticated athlete's identity. `grep -n "athlete_id\|profile"`
in that crate returns nothing. There is no call site that would
populate a real value for `AuthLogin::athlete_id` to delegate to.

### How sauce4zwift gets it

In `sauce4zwift/src/zwift.mjs`:

```javascript
// line 333 — single ZwiftAPI.authenticate() used by both main and monitor
async authenticate(username, password, options={}) {
    // ... POST password-grant to Keycloak ...
    this._authToken = resp;
    this._authTokenTime = this.getTime();
    this._schedRefresh(this._authToken.expires_in * 1000 / 2);
    this.profile = await this.getProfile('me');   // ← line 362
}

// line 541
async getProfile(id, options) {
    if (this.exclusions.has(getIDHash(id))) return;
    try {
        return await this.fetchJSON(`/api/profiles/${id}`, options);
    } catch(e) {
        if (e.status === 404) return;             // ← swallow 404 only
        throw e;                                  // ← propagate everything else
    }
}
```

The contract sauce relies on:

1. After the OAuth password-grant succeeds, fetch `/api/profiles/me`
   inside the same `authenticate()` call.
2. Cache the resulting JSON object on `this.profile`.
3. Use `this.profile.id` as the `athleteId`/`player_id` for every
   relay packet built thereafter (`zwift.mjs:977, 1318, 1481, 2395`).

`getProfile('me')` is treated as fatal-on-failure except for 404, which
sauce maps to "profile not found, return undefined". 404 on `me` is
effectively impossible (the bearer token belongs to a real account), so
in the eager-on-login path treating any non-200 as a hard error is
fine.

### Sauce monitor-account flow (open-question follow-up)

Confirmed by reading `sauce4zwift/src/main.mjs`:

```javascript
// main.mjs:111
export const zwiftMonitorAPI = new Zwift.ZwiftAPI({getTime});

// main.mjs:660-670
const mainUser = await zwiftAuthenticate({api: zwiftAPI, ident: 'zwift-login'});
const monUser = await zwiftAuthenticate({
    api: zwiftMonitorAPI,
    ident: 'zwift-monitor-login',
    monitor: true,
});
```

`zwiftMonitorAPI` is a *separate instance of the same `ZwiftAPI` class*
fed the monitor account's credentials. The `monitor: true` flag only
selects which keychain entry holds the credentials (`'zwift-monitor-login'`
vs. `'zwift-login'`); it does not branch the API flow. So
`zwiftMonitorAPI.authenticate()` runs the exact same body as the main
`zwiftAPI.authenticate()`, including the `getProfile('me')` call at
line 362. The monitor account therefore caches its own profile, and
`zwift.mjs:2489` (`athleteId: this.monitorAPI.profile.id`) reads the
monitor account's own athlete ID into outgoing packets.

Implication for ranchero: there is no separate "monitor profile lookup"
endpoint to discover. The same `GET /api/profiles/me` against the
monitor's bearer token is what sauce uses. The relay packets sent on
the monitor-only path must carry the monitor account's own ID, not the
main account's.

### Required fix sketch

1. Add a `get_profile_me()` (or similar) call to `crates/zwift-api`
   that issues `GET /api/profiles/me` against `cfg.api_base` with the
   bearer token. Decode at minimum the `id` field (i64).
2. Have `ZwiftAuth::login` either eagerly fetch the profile or expose
   a lazy `athlete_id()` that fetches on first use and caches. Match
   sauce's contract (line 362, `authenticate()` ends with
   `this.profile = await this.getProfile('me')`): eager fetch as part
   of the login call, so any caller that sees `Ok` from `login` knows
   the profile is already cached.
3. Change `DefaultAuthLogin::athlete_id` to delegate to the new
   `ZwiftAuth` accessor instead of inheriting the trait default.
4. **Decision (2026-05-02): remove the `AuthLogin::athlete_id` trait
   default.** Every `AuthLogin` implementation must explicitly return
   a value. Rationale: the whole point of choosing Rust here is to
   surface this kind of correctness gap at compile time rather than
   discover it via a tracing log on a real account. Existing test
   stubs (`StubAuth` in `src/daemon/relay.rs:1867` and similar) get
   an explicit `async fn athlete_id(&self) -> Result<i64, _> { Ok(N) }`
   override. Test fixtures that previously inherited the placeholder
   are updated to return a deterministic non-zero ID so they remain
   distinct from the production zero-bug.
5. Confirm the authenticated user is the *monitor* account on the
   monitor-only path. Sauce's
   `sauce4zwift/src/zwift.mjs:2489` reads `this.monitorAPI.profile.id`,
   and `monitorAPI` is a separate `ZwiftAPI` instance fed the monitor
   credentials at startup (`main.mjs:666–670`). Both `ZwiftAPI`
   instances run the same `authenticate()` body, so the monitor
   instance's `profile.id` is its own athlete ID — confirmed by
   reading the sauce source.

## Issue B — `TCP connect: Connection refused (os error 61)`

### What `os error 61` means

`os error 61` is the literal `errno` returned by the macOS kernel,
surfaced by `std::io::Error::raw_os_error()` and rendered by
`Display`. On Darwin (macOS) `errno 61` is `ECONNREFUSED`
("Connection refused"). It is not a string from Zwift — it is a local
kernel error.

The kernel produces `ECONNREFUSED` for a `connect(2)` call when the
TCP three-way handshake is rejected. The two routes there are:

1. The peer answered the SYN with a TCP RST. The remote host is
   reachable and the IP/port pair is correct, but nothing is listening
   on that port (or a host firewall rejects rather than drops).
2. A middlebox on the path returned an ICMP "destination unreachable
   (port unreachable)" message that the local kernel turned into
   `ECONNREFUSED`.

A pure firewall-drop (no RST, no ICMP) would surface as `ETIMEDOUT`
after the kernel's `connect` timeout, not as `ECONNREFUSED`. So the
remote side is producing some kind of explicit refusal.

### Where the address came from

`src/daemon/relay.rs:986–992` and `:1240–1245`:

```rust
let server = &session.tcp_servers[0];
let addr_str = format!("{}:{}", server.ip, server.port);
let addr: std::net::SocketAddr = addr_str.parse()
    .map_err(|_| RelayRuntimeError::BadTcpAddress(addr_str.clone()))?;
tracing::info!(target: "ranchero::relay", addr = %addr, "relay.tcp.connecting");
```

`session.tcp_servers` is populated in
`crates/zwift-relay/src/session.rs:166–179` from the `LoginResponse`
and is filtered to `lb_realm == 0 && lb_course == 0`. The proto
comments (`crates/zwift-proto/proto/per-session-info.proto:5–6`,
`udp-node-msgs.proto:211–212`) describe these as the "generic" load
balancing cluster, which matches sauce's `realm === 0 && courseId === 0`
filter at `sauce4zwift/src/zwift.mjs:1814`. The filter semantics are
equivalent.

### Where sauce diverges — the port is hard-coded

Reading `sauce4zwift/src/zwift.mjs:1209–1218`:

```javascript
async establish() {
    this.conn = Net.createConnection({
        host: this.ip,
        port: 3025,            // ← hard-coded, ignores anything from the response
        timeout: 31000,
        onread: { ... },
    });
    // ...
}
```

Sauce **uses the IP from the response but ignores the `port` field
entirely**, hard-coding the TCP port to **3025**. Confirmed against
the proto schema (`crates/zwift-proto/proto/per-session-info.proto:2–4`):

```
message TcpAddress {
    optional string ip = 1;
    optional int32 port = 2;
    ...
}
```

The field exists, our code reads it (`crates/zwift-relay/src/session.rs:166–179`)
and stores it on `TcpServer`, but in production the value Zwift fills
in is not the port to dial. Whatever it represents (a hint to the
load balancer? a legacy field?), the listener is on 3025.

What we did wrong: `src/daemon/relay.rs:986–992` and `:1240–1245` both
build the connect address as `format!("{}:{}", server.ip, server.port)`,
honoring the proto value. With the failing log's `port = 3023`, the
SYN went to a port that has nothing listening (or a host firewall
sending RST), so the kernel returned `ECONNREFUSED`.

The crate-level doc comment on `crates/zwift-relay/src/tcp.rs:4`
already says "the chosen relay server's port 3025" — the port number
*was* correctly identified at design time, then the implementation in
`relay.rs` regressed by reading the proto field. The UDP port is
already handled correctly in `relay.rs:1054`
(`format!("{}:{}", udp_server.ip, zwift_relay::UDP_PORT_SECURE)`),
which uses a constant. The TCP path needs the same treatment.

### Sticky-server selection (secondary)

Beyond the port, sauce keeps a *sticky* server selection across
reconnects (`zwift.mjs:1813–1827`):

```javascript
async establishTCPChannel(session) {
    const servers = session.tcpServers.filter(x => x.realm === 0 && x.courseId === 0);
    let ip;
    if (this._lastTCPServer) {
        const lastServer = servers.find(x => x.ip === this._lastTCPServer);
        if (lastServer) ip = lastServer.ip;
    }
    if (!ip) ip = servers[0].ip;
    this._lastTCPServer = ip;
    // ...
}
```

The comment in the source ("I really need to stick to the same TCP
server no matter what") indicates Zwift has session affinity to a
specific server. For a *fresh* session sauce still picks `servers[0]`,
which is what we already do — so sticky selection is not the cause of
the first-attempt failure, but is a known requirement for retry/refresh
flows that we will need before STEP-12 is fully closed.

### Relationship with Issue A

Now that the port issue is identified, the two bugs are independent:

- The `ECONNREFUSED` is fully explained by dialing the wrong port; it
  would have happened even with a correct `athlete_id` in the hello
  packet, because the hello is never sent (the connect itself fails).
- The `athlete_id=0` would also have failed the session at the
  application layer once the TCP connect succeeded, because the hello
  carries `player_id: 0`.

Both must be fixed for the workflow to complete end-to-end, but
neither is upstream of the other and they can be tackled in either
order.

### Diagnostic checks (only if green tests still fail after the port fix)

- Capture the raw `LoginResponse` bytes from
  `crates/zwift-relay/src/session.rs` and inspect every entry in
  `info.nodes.nodes`, not only the post-filter list. Compare against
  what sauce sees on the same account (sauce logs the chosen IP at
  `zwift.mjs:1828`).
- Confirm reachability at the corrected port with
  `nc -vz <ip> 3025` while the daemon is not running. A refusal on
  3025 too would point at firewall/network rather than ranchero.

## Error type for the profile-fetch path

`crates/zwift-api/src/lib.rs:52–71` currently exposes:

```rust
pub enum Error {
    Http(#[from] reqwest::Error),
    InvalidTokenResponse(String),
    AuthFailed(String),
    NotAuthenticated,
    RefreshFailed(String),
    Status { status: u16, body: String },
}
```

`AuthFailed(String)` carries everything in one bucket. For the new
`/api/profiles/me` call (and to make the existing token errors equally
typed) the failure modes are pulled out so callers can branch
exhaustively in `match` instead of string-sniffing. **Decision
(2026-05-02):**

```rust
pub enum Error {
    Http(#[from] reqwest::Error),
    InvalidTokenResponse(String),
    NotAuthenticated,
    RefreshFailed(String),
    Status { status: u16, body: String },

    // Replaces the single AuthFailed(String) variant.
    AuthFailedUnauthorized(String),  // 401 — bad credentials, expired token
    AuthFailedForbidden(String),     // 403 — token valid, account/permission denies
    AuthFailedBadSchema(String),     // 200 with malformed/unexpected body
    AuthFailedUnknown(String),       // anything else (e.g. 5xx, decode error)
}
```

Mapping for `get_profile_me`:

| HTTP outcome | Variant |
|---|---|
| 200, body parses, `id` present | `Ok(Profile { id, .. })` |
| 200, body missing `id` or wrong shape | `AuthFailedBadSchema` |
| 401 | `AuthFailedUnauthorized` |
| 403 | `AuthFailedForbidden` |
| 5xx, network error not already covered by `Http`, etc. | `AuthFailedUnknown` |

The same split applies to the existing `login`/`refresh` paths so the
trait surface stays consistent: any "auth-shaped" failure picks one of
the four variants. `Status { status, body }` remains for *non-auth*
HTTP failures (e.g. a misbehaving general endpoint).

Test surface: every test that constructed an `Error::AuthFailed(_)`
must be updated to construct the right specific variant, and
`zwift_api::Error` consumers (notably `RelayRuntimeError::Auth`)
re-export or re-wrap accordingly so the daemon stderr message names
the failure precisely (`"unauthorized"`, `"forbidden"`,
`"unexpected response shape"`, etc.).

## Open questions

- What does the proto `port` field on `TcpAddress` actually mean, if
  it is not the listener port? Worth grep-ing zwift-offline source
  and the original sauce comments for any clue, since the field is
  declared `optional int32 port = 2` but is unused at the call site
  that matters. Not blocking — we can ignore it the way sauce does —
  but documenting it would close the loop.
- Sticky TCP server selection across reconnects (`_lastTCPServer` in
  sauce) is not implemented in ranchero. Required before reconnect /
  session-refresh flows are robust, but not blocking the first-time
  workflow.

(Resolved: companion-app setup is not required — sauce works on a
fresh monitor account with no pairing/event-join. So the broadcast-IP
hypothesis was wrong — the IP `16.146.39.255` is in fact a real
listening Zwift relay; we just dialed the wrong port.)

## Suggested ordering

The two fixes are independent and can land in either order, but the
TCP port fix is the smaller change and unblocks any further runtime
testing (without it the connect always fails, regardless of athlete
ID):

1. **Hard-code the TCP relay port to 3025** (matching sauce). Either:
   - Introduce `pub const TCP_PORT_SECURE: u16 = 3025;` in
     `crates/zwift-relay/src/consts.rs` (or wherever
     `UDP_PORT_SECURE` lives) and use it in
     `src/daemon/relay.rs:986–992` and `:1240–1245`; or
   - Drop the `port` field from `TcpServer` entirely so callers
     cannot accidentally read it. Cleaner — the field is misleading
     dead state once we know it is not the listener port.
   Adjust the test fixtures in `crates/zwift-relay/src/session.rs`
   tests and `src/daemon/relay.rs` tests that build `TcpServer` to
   match. Re-run the workflow and confirm the connect succeeds (or at
   least proceeds past `connect()` and reaches the hello packet).
2. **Implement `get_profile_me`** in `zwift-api`, wire it into
   `ZwiftAuth::login` (eager fetch), expose
   `ZwiftAuth::athlete_id() -> Result<i64, Error>`, **remove the
   `AuthLogin::athlete_id` trait default**, and have
   `DefaultAuthLogin::athlete_id` delegate to the new accessor. Update
   every test stub to override `athlete_id` with a deterministic
   non-zero value. Split `Error::AuthFailed` into the four typed
   variants. Re-run and confirm `relay.login.ok` shows the real
   athlete ID, then check whether the application-layer session
   establishes.
3. (Stretch, only if step 2's outcome reveals more breakage)
   Implement sticky TCP server selection on reconnect / refresh.

## Cross-references

- `docs/plans/done/STEP-12.9-confirm-path-before-background.md` —
  Item 1 here closed the capture-path-before-fork gap; the workflow
  above runs cleanly through the capture lifecycle as a result.
- `sauce4zwift/src/zwift.mjs:362` — `getProfile('me')` after login.
- `sauce4zwift/src/zwift.mjs:1813–1827` — TCP server selection.
