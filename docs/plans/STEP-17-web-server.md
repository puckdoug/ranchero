# Step 17 — HTTP + WebSocket server

## Goal

Build an actix-web HTTP and WebSocket server inside the
ranchero daemon. It replaces sauce4zwift's
`src/webserver.mjs` and serves the same JSON wire protocol
and REST endpoints, so the widget tree under `pages/`
works without changes.

The server shares the daemon's tokio runtime with the
relay subsystem. It reads from `AthleteRegistry` (added in
STEP 14 and 15) and from a `broadcast::Receiver<GameEvent>`
fed by STEP 12. This step also owns the proto-to-stats
translation: turning incoming `PlayerState` records from
the relay into the `AthleteData` updates the widgets read
back.

Per spec §6.3 and §7.9, the server must:

- Bind to `server.bind:server.port` from the resolved
  config (default `127.0.0.1:1080`).
- Serve the widget tree under `/pages/*` from a vendored
  copy at `pages/` in the repo root. No path may resolve
  through the `sauce4zwift` symlink (see CLAUDE.md).
- Expose one WebSocket endpoint for subscribe / unsubscribe
  / rpc.
- Auto-enable HTTPS when `https/key.pem` and
  `https/cert.pem` exist next to the working directory.
- Disconnect a client whose outbound buffer crosses 8 MB.
- Log lifecycle events (server start, server stop, client
  connect, client disconnect, subscription open,
  subscription close) by default — not only under `-v`.

### Three differences between the spec and what widgets actually do

Three places where spec §6.3 / §7.9 and the previous
version of this plan disagree with the running JavaScript.
Settling each one up front keeps the test suite aimed at
a single target.

1. **WebSocket endpoint path.** Spec §6.3 says
   `/api/socket`. `webserver.mjs:264` uses
   `/api/ws/events`, and every widget calls that path.
   Serve `/api/ws/events`. An `/api/socket` alias is one
   more path to keep working for no caller; skip it.

2. **Request frame shape.** Spec §6.3 shows a flat
   object `{type, method, uid, arg}`. The actual wire
   shape, from `webserver.mjs:35-56,121`, is
   `{type, uid, data: {method, arg}}`. The nested form
   is the one the encoder mirrors when it writes
   responses. The decoder accepts the flat form too,
   per point 3.

3. **Accept liberal, emit strict.** On input: accept any
   reasonable variant — either the nested shape from
   point 2 or the flat shape from the spec, any field
   order, ignoring unknown extra fields. When a frame
   is too broken to parse (missing `method` or `arg`,
   etc.), reply with `{type:"response", success:false,
   uid:<echoed if available, otherwise -1>,
   error:"..."}`. On output: always emit the same
   chosen form. Responses are `{type, success, uid,
   data|error}` in that field order. Events are
   `{success, type, uid, data}` in that field order
   (matching the byte layout `webserver.mjs:145-178`
   writes from three buffers). Use named
   `#[derive(Serialize)]` structs so field order is
   fixed at compile time. Note the chosen ordering at
   the encoder site; tests parse the JSON back rather
   than diff strings, so the order itself does not
   need test coverage.

## Summary checklist

`-T` is a failing test; `-I` is the code that turns it
green. Plain TDD throughout: write the test, watch it
fail, write the smallest code that makes it pass.

### Foundations

- [x] **17.1-T** `tests/web_server_bind.rs` — `WebServer`
      binds to a host:port pair and a separate client can
      open a TCP connection to it. Marked
      `#[ignore = "slow: binds a real socket"]`.
- [x] **17.1-I** Add `actix-web` (with the `rustls-0_23`
      feature), `actix-cors`, `actix-files`, `actix-ws`,
      `rustls`, `rustls-pemfile`, `mime_guess`, `base64`,
      `bytes`, and `futures-util` to the root
      `Cargo.toml`. Create `src/web/` with `mod.rs`,
      `server.rs`, `state.rs`, `http/`, `ws/`, `subs/`.
      Expose
      `web::start(cfg, state, shutdown) -> Result<WebServerHandle, WebError>`.

### Configuration surface

- [x] **17.2-T** `tests/web_config.rs` — `[server]
      pages_root = "..."` in the TOML surfaces as
      `server_pages_root: PathBuf` on `ResolvedConfig`;
      `RANCHERO_PAGES_ROOT` overrides the file value;
      default is the `pages/` directory that sits one
      level above the binary (i.e.
      `current_exe()?.parent()?.join("../pages")`),
      mirroring sauce4zwift's `WD/../pages`.
- [x] **17.2-I** Add `pages_root: Option<String>` to
      `ServerConfig` and plumb it through
      `ResolvedConfig` as `server_pages_root: PathBuf`.
      When no value is configured, derive the default
      from the binary location:
      `std::env::current_exe()?.parent()?.join("../pages")`.
      At daemon startup (before opening any sockets),
      check that the directory exists with a stat call.
      If it does not exist, write a plain message to
      stderr and exit. Also log the error through the
      normal logging path if the logger is already
      running.
- [x] **17.3-T** `tests/web_config.rs` (continued) —
      `[server] https_cert_dir = "/some/path"` surfaces
      as `server_https_cert_dir: PathBuf`; default is
      `"https"` next to the binary's working directory.
- [x] **17.3-I** Add `https_cert_dir: Option<String>`
      to `ServerConfig` and plumb it through
      `ResolvedConfig` as `server_https_cert_dir: PathBuf`.
      Default is `PathBuf::from("https")` relative to
      the working directory.

### HTTP routing

- [x] **17.4-T** `tests/http_get_api_root.rs` —
      `GET /api/` returns a JSON directory listing of
      registered endpoints, status 200, content-type
      `application/json`.
- [x] **17.4-I** Build the actix-web `App` inside the
      `HttpServer::new(...)` factory closure. Register
      the API directory handler that returns the list of
      mounted `/api/...` routes from a static manifest.
- [x] **17.5-T** `tests/http_cors_preflight.rs` —
      `OPTIONS /api/anything` returns 204 with
      `Access-Control-Allow-Origin: *` and
      `Access-Control-Allow-Headers: *`. The same applies
      to `OPTIONS /pages/...` and `OPTIONS /shared/...`.
- [x] **17.5-I** Wrap the `/api`, `/pages`, and `/shared`
      scopes with `actix_cors::Cors`. Allow any origin
      and any header; allow methods `GET`, `HEAD`,
      `OPTIONS`, and `POST`.
- [x] **17.6-T** `tests/http_athlete_v1.rs` —
      `GET /api/athlete/v1/:id` returns the athlete
      record for a present athlete; returns 404 for a
      missing one; accepts `self`, `watching`, or a
      numeric id; emits JSON whose top-level field names
      match `_formatAthleteData` in `stats.mjs:2050+`.
- [x] **17.6-I** Implement `athlete_v1_handler(path)`.
      Resolve `self` / `watching` through the daemon's
      `WatchedAthlete` state. The formatter is a port of
      `_formatAthleteData`; the test contract is
      field-for-field equality.
- [x] **17.7-T** `tests/http_nearby_v1.rs` —
      `GET /api/nearby/v1` returns a JSON array of
      nearby athletes; element fields match
      `_formatNearby` in `stats.mjs:2117+`.
- [x] **17.7-I** Implement `nearby_v1_handler()`. Reads
      from `AthleteRegistry` through the daemon's
      read-only snapshot path (see §17.20).
- [x] **17.8-T** `tests/http_groups_v1.rs` —
      `GET /api/groups/v1` returns a JSON object with
      the v1 group-classification shape
      (`stats.mjs:2150+`).
- [x] **17.8-I** Implement `groups_v1_handler()`. Uses
      `AthleteRegistry::groups`, which is already
      populated by `compute_groups` in
      `zwift-stats::groups`.
- [x] **17.9-T** `tests/http_404_fallthrough.rs` —
      `GET /api/anything-unknown` returns 200 with the
      API directory listing rather than a 404 body,
      matching `webserver.mjs:490-494`.
- [x] **17.9-I** Add a `default_service` on the `/api`
      scope so unmatched `/api/...` paths delegate to the
      API directory handler.

### v2 endpoints (resource filtering)

- [ ] **17.10-T** `tests/http_athlete_v2.rs` —
      `GET /api/athlete/v2/:id?resource=stats&resource=lap`
      returns only the requested resources; `?stats=true`
      includes the extended statistics block; omitting
      the query returns the v1 shape under a v2 wrapper.
- [ ] **17.10-I** Implement `athlete_v2_handler(path,
      query)`. Parse the multi-valued `resource` query
      parameter and the boolean `stats` parameter, reuse
      the v1 formatter, and apply resource filtering on
      the assembled value.
- [ ] **17.11-T** `tests/http_nearby_v2.rs` and
      `tests/http_groups_v2.rs` — same resource
      filtering applied to nearby and groups.
- [ ] **17.11-I** Implement `nearby_v2_handler` and
      `groups_v2_handler` using the shared filter
      helper.

### RPC

- [ ] **17.12-T** `tests/http_rpc_v1.rs` —
      `GET /api/rpc/v1/:name?arg=1&arg=true&arg=foo`
      dispatches to the registered handler with args
      `[1, true, "foo"]`. Types come from the lexical
      form (see `webserver.mjs:408-439`).
      `POST /api/rpc/v1/:name` with body
      `[1, true, "foo"]` dispatches with the same args.
      Unknown handler returns 404.
- [ ] **17.12-I** Implement `rpc_v1_handler` (GET) and
      `rpc_v1_post_handler` (POST). Share the handler
      registry between HTTP and WebSocket dispatch by
      passing it as `web::Data<RpcRegistry>`.
- [ ] **17.13-T** `tests/http_rpc_v2.rs` —
      `GET /api/rpc/v2/:name*` decodes a base64url-encoded
      JSON array tail as `[arg, arg, ...]` and
      dispatches.
- [ ] **17.13-I** Implement `rpc_v2_handler` (GET).
      Decode the tail with
      `base64::engine::general_purpose::URL_SAFE_NO_PAD`,
      parse as JSON, dispatch.
- [ ] **17.14-T** `tests/http_rpc_discovery.rs` —
      `GET /api/rpc/v1` and `GET /api/rpc/v2` both return
      a JSON array of registered handler names.
- [ ] **17.14-I** Implement both discovery routes
      against the shared handler registry.

### WebSocket protocol

- [ ] **17.15-T** `tests/ws_handshake.rs` — a WebSocket
      client completes the upgrade on `/api/ws/events`.
      Marked `#[ignore = "slow: real socket"]`.
- [ ] **17.15-I** Add the WebSocket route. The handler
      calls `actix_ws::handle(&req, body)` to perform
      the upgrade and returns `(HttpResponse, Session,
      MessageStream)`. The session and message stream
      move into a task spawned on the actix runtime
      (`actix_web::rt::spawn`); the task owns the read
      loop, the write loop, and the per-client `subs`
      map.
- [ ] **17.16-T** `tests/ws_request_response.rs` —
      three cases exercise the accept-liberal /
      emit-strict rule:
      (a) the nested wire form
      `{type:"request", uid:42, data:{method:"rpc",
      arg:{name:"getVersion", args:[]}}}` produces
      `{type:"response", success:true, uid:42, data:...}`
      with the matching uid;
      (b) the flat spec form
      `{type:"request", method:"rpc", uid:42,
      arg:{name:"getVersion", args:[]}}` produces an
      identical response;
      (c) a frame with `type`, `method`, `uid`, `arg`,
      and an unknown extra field dispatches normally,
      with the extra field ignored.
      An unknown RPC name produces `success:false` and
      `error:"unknown rpc handler: ..."`. A
      structurally malformed frame (no `method`)
      produces `success:false` with the echoed uid
      where available, otherwise `-1`.
- [ ] **17.16-I** Implement the WebSocket frame codec.
      The decoder is one `#[derive(Deserialize)]`
      struct with both `data: Option<MethodArg>` and the
      flat fields `method: Option<Method>` and
      `arg: Option<Value>`, all optional. A post-parse
      step folds the nested fields into the flat fields
      and rejects only when neither path is populated.
      Use `#[serde(default)]` and omit
      `#[serde(deny_unknown_fields)]` so extra fields
      are silently ignored. The encoder uses the named
      `Response` and `Event` structs defined in §17.15
      so the field order is fixed by the struct
      definition.
- [ ] **17.17-T** `tests/ws_subscribe_event.rs` — a
      `subscribe` request with `arg:{event:
      "athlete/watching", source:"stats", subId:7}`
      produces a `{type:"response", success:true,
      uid:N}` reply and then receives
      `{type:"event", uid:7, success:true, data:...}`
      frames as the registry changes. An `unsubscribe`
      request with `arg:{subId:7}` ends the stream.
- [ ] **17.17-I** Wire the subscription engine
      (§17.20-22) into the client task's
      `method: "subscribe"` and
      `method: "unsubscribe"` branches.
- [ ] **17.18-T** `tests/ws_uid_isolation.rs` — two
      clients subscribe to the same event with
      different `subId`s; each gets its own stream;
      unsubscribe on client A does not stop events on
      client B.
- [ ] **17.18-I** Per-client `subs: HashMap<SubId, Sub>`
      lives inside the client task. Subscription dedup
      across clients is a process-wide concern
      (§17.21); per-client tracking is local.
- [ ] **17.19-T** `tests/ws_disconnect_cleanup.rs` —
      closing the WebSocket while subscriptions are
      active drops the delegation reference counts;
      when a count reaches zero the upstream listener
      is removed.
- [ ] **17.19-I** On client task exit, walk `subs` and
      release each. The registry's release path drops
      the upstream listener once the last subscriber
      departs.

### Subscription engine

- [ ] **17.20-T** `tests/subs_source_lookup.rs` — a
      subscribe with `source:"stats"` finds the
      registered source; an unknown source produces
      `success:false` with a clear error.
- [ ] **17.20-I** Add a `SourceRegistry`
      (`HashMap<&'static str, Box<dyn EventSource>>`)
      to the shared web state. Built-in sources at this
      step: `stats`, `gameConnection`. Later steps
      register more (logs, mods, windows — see
      §"Out of scope").
- [ ] **17.21-T** `tests/subs_delegation_dedup.rs` —
      two clients subscribe to the same
      `(source, event, options)` triple; the upstream
      listener is added once; both clients receive each
      emission; one unsubscribes; the upstream listener
      stays attached; the second unsubscribes; the
      upstream listener is removed.
- [ ] **17.21-I** `Delegations: HashMap<(SourceId,
      EventId, OptionsHash), Arc<Delegation>>`. The
      reference count is `Arc::strong_count` on the
      `Delegation`; when the last client drops its
      `Arc`, the delegation is removed and the upstream
      listener is released.
- [ ] **17.22-T** `tests/subs_event_payload.rs` — an
      `athlete/:id` subscription with a known watched
      athlete emits frames whose `data` shape matches
      the v1 athlete formatter; the value updates as
      the underlying `AthleteData` ingests samples.
- [ ] **17.22-I** Implement the `stats` source. It
      exposes `subscribe(event_path, options, sink)`
      and internally subscribes to the daemon-internal
      stats broadcast, filtering by event path.

### Static files and CORS

- [ ] **17.23-T** `tests/http_static_pages.rs` —
      `GET /pages/index.html` returns the file with
      content-type `text/html` and
      `Access-Control-Allow-Origin: *`. `GET
      /pages/missing.html` returns 404.
- [ ] **17.23-I** Mount `actix_files::Files::new("/pages",
      &server_pages_root)` with `.index_file("index.html")`
      and `.use_last_modified(true)`. Mount the same at
      `/shared` rooted at
      `server_pages_root.parent().join("shared")`, which
      mirrors sauce4zwift's layout. Apply the cache
      headers from `webserver.mjs:236-251`:
      images → `private, max-age=3600`;
      `deps/flags` and `fonts` →
      `private, max-age=8640000`. Attach the headers as
      nested `Files` scopes wrapped with
      `actix_web::middleware::DefaultHeaders`.
- [ ] **17.24-T** `tests/http_root_index.rs` — `GET /`
      returns `pages/index.html`.
- [ ] **17.24-I** Add a `web::resource("/")` handler
      that reads and returns the index file with
      content-type `text/html`. `Files::new` mounted at
      `/pages` does not serve the bare-root case, so it
      needs an explicit route.
- [ ] **17.25-T** `tests/http_mime_types.rs` —
      `GET /pages/main.css` returns content-type
      `text/css`; `GET /pages/app.mjs` returns
      `text/javascript`. The MIME map must agree with
      sauce4zwift's `src/mime.mjs`.
- [ ] **17.25-I** `actix_files` uses `mime_guess`
      internally. Some `mime_guess` versions return
      `application/javascript` for `.mjs`; sauce4zwift
      returns `text/javascript`. Use
      `Files::mime_override(|mime, path| ...)` to force
      `text/javascript` when the path ends in `.mjs`.

### HTTPS

- [ ] **17.26-T** `tests/https_conditional.rs` — when
      `{cert_dir}/key.pem` and `{cert_dir}/cert.pem`
      exist, starting the server produces both an HTTP
      listener on `server_port` and an HTTPS listener
      on `server_port + 1`. When either certificate is
      missing, only the HTTP listener exists, and a
      warning reaches the tracing log. Marked
      `#[ignore = "slow: generates a self-signed cert"]`.
- [ ] **17.26-I** Read certs with `rustls-pemfile`.
      Bind both listeners on the same `HttpServer` by
      chaining `.bind(addr)?` and
      `.bind_rustls_0_23(https_addr, server_config)?`
      before `.run()`. This needs the `rustls-0_23`
      feature on `actix-web`.

### Backpressure

- [ ] **17.27-T** `tests/ws_backpressure.rs` — a client
      that stops reading is disconnected once buffered
      output crosses 8 MB. The disconnect reason
      appears in the tracing log; other clients are
      unaffected. Marked `#[ignore = "slow: pushes 8 MB
      through a real socket"]`.
- [ ] **17.27-I** Each per-client task owns an
      `mpsc::Sender<Bytes>` of bounded capacity. A
      separate `BufferedBytes(AtomicUsize)` per client
      tracks the sum of `frame.len()` for everything
      in flight (queued in the channel plus waiting to
      flush). The subscription dispatch reads this
      counter before pushing; if the value crosses
      8 MB the dispatch logs and the client task calls
      `session.close(Some(CloseReason::from(
      CloseCode::Policy)))`.

### proto-to-stats translation (the deferred STEP 14 work)

This section absorbs the items the previous stub flagged
as "deferred from STEP 14". They belong here because
STEP 17 is the first place that connects decoded
`PlayerState` records to the `AthleteData` stream the
widgets read.

- [ ] **17.28-T** `tests/proto_to_stats_routing.rs` —
      given a fixture `GameEvent::PlayerState`, the
      proto-to-stats router calls `registry.upsert(...)`
      and then the five
      `ingest_power / ingest_hr / ingest_speed /
      ingest_cadence / ingest_draft` calls on the
      resulting `AthleteData`, with the right unit
      conversions (proto sends `u_hz` for cadence,
      stats wants `rpm`; proto sends `mm_h` for speed,
      stats wants `m/s`).
- [ ] **17.28-I** Implement
      `route_player_state(event, registry, now)` in
      `src/web/proto_to_stats.rs`. The function is
      proto-aware so that `zwift-stats` stays
      proto-free.
- [ ] **17.29-T**
      `tests/registry_upsert_course_change.rs` — when
      the same `athlete_id` arrives with a different
      `course_id` than last seen, the test pins one of
      three behaviours: overwrite identity, reset the
      relevant collectors, or replace the record.
- [ ] **17.29-I** Pick one of the three behaviours.
      The recommendation, pending confirmation from a
      real capture, is to overwrite the identity
      fields on every upsert. Reasons: course changes
      mid-ride are rare; the collectors are anchored
      on `world_time`, not `course_id`, so resetting
      them on a course change discards useful data; the
      JavaScript behaviour of keeping the original
      `course_id` forever looks like an oversight in
      the source rather than an intended contract; and
      the other two options require new methods on
      `AthleteRegistry` that nothing else needs yet.
      If a captured trace later shows the JavaScript
      behaviour matters, switch to "reset the relevant
      collectors" and add the method then. Record the
      final choice in this step's as-built notes.
- [ ] **17.30-T**
      `tests/player_state_view_for_proto.rs` —
      implement `PlayerStateView` on
      `zwift_proto::PlayerState` so the STEP 15
      detectors can read proto values without an
      intermediate copy. The test runs one
      representative detector (segment-active or
      group-gap) against a synthetic proto value and
      asserts that its output matches the
      `MostRecentState` path byte-for-byte.
- [ ] **17.30-I** Add `impl PlayerStateView for
      zwift_proto::PlayerState` in
      `src/web/proto_view.rs`. Keep the existing
      `MostRecentState` impl in place; the proto impl
      is additive. Record the choice (use the proto
      impl in production, keep `MostRecentState` for
      tests and as the in-memory snapshot) in this
      step's as-built notes.
- [ ] **17.31-T**
      `tests/event_behavior_from_config.rs` — adding
      `[stats] auto_reset_events = true` and
      `auto_lap_events = false` to the TOML produces
      `EventBehavior { auto_reset: true, auto_lap:
      false }` on `ResolvedConfig`. The defaults match
      the JS `_autoResetEvents` and `_autoLapEvents`
      values from `stats.mjs:884-887` (both default
      to `true`).
- [ ] **17.31-I** Add `StatsConfig {
      auto_reset_events, auto_lap_events }` to the
      file schema. Surface it on `ResolvedConfig` as
      `event_behavior: EventBehavior` and thread it
      through every `apply_event_state` call.
- [ ] **17.32-T** `tests/gc_tick_runs_on_interval.rs`
      — driving the GC tick at the chosen interval
      calls `registry.gc(now)` and produces a tracing
      event with the `GcReport` counts. The test uses
      `tokio::time::pause()` and
      `tokio::time::advance()` so it does not actually
      wait.
- [ ] **17.32-I** Spawn a `tokio::time::interval`
      driver that calls `registry.gc(now)`. The
      interval is a single constant in
      `src/web/state.rs`; see §17.33 for the value.
- [ ] **17.33-T** `tests/gc_interval_documented.rs` —
      the chosen interval matches
      `GC_TICK_INTERVAL_SECS` from
      `zwift-stats::periods` (currently `62.768`) and
      the constant has a doc-comment that cites
      `stats.mjs:3553`. The 10 s figure from the
      previous stub is recorded as rejected.
- [ ] **17.33-I** Confirm `GC_TICK_INTERVAL_SECS =
      62.768` against a fresh read of
      `stats.mjs:3553`. If it matches, keep the
      constant and add the doc-comment. If not,
      update the constant and update STEP 14's
      as-built notes.

### Wiring

- [ ] **17.34-T** `tests/daemon_starts_web_server.rs`
      — daemon boot starts the web server on the
      configured port and `GET /api/` returns 200.
      Uses `RANCHERO_SERVER_PORT=0` so each test gets
      a free port; the bound port is read back from
      the actix `ServerHandle` after listen. Marked
      `#[ignore = "slow: full daemon boot"]`.
- [ ] **17.34-I** In `src/daemon/runtime.rs` (`start`),
      after `Stores::open()` succeeds and before the
      relay comes up, spawn the web server on the
      same tokio runtime through
      `web::start(cfg, state, shutdown)`. The
      shutdown signal is a `tokio::sync::Notify` the
      daemon fires when the control socket receives
      `shutdown`; the web server's `start()` watches
      the notify and calls `ServerHandle::stop(true)`
      on receipt.
- [ ] **17.35-T** `tests/cli_status_includes_web.rs`
      — `ranchero status` reports the web-server
      bind, port, HTTPS state, and active connection
      count under a `Web server` section, even when
      the daemon is not running. In the not-running
      case, bind and port come from `ResolvedConfig`
      and the connection count reads
      `daemon not running`.
- [ ] **17.35-I** Extend the status printer (the
      same one STEP 16 added the `Persistence:` block
      to) with a `Web server:` block. The live
      connection count comes from the control
      protocol; add a `web` field to `StatusResponse`.
      The not-running branch reads the config-only
      fields.

## Tests-first plan (detail)

### 17.1 Server binding

The simplest possible server: bind, accept one TCP
connection, return 404 to everything. The test does not
exercise routing — it only checks that `start()`
succeeds and the port becomes reachable.

The slow marker reflects a real loopback socket plus a
500 ms watchdog for the bind to complete.

### 17.4 — 17.9 HTTP REST

These tests grow the route table one endpoint at a
time. Each one runs in-process via
`actix_web::test::init_service` and
`actix_web::test::call_service` against a
`TestRequest::get().uri("/api/...")`. No real socket,
no slow marker. The formatter is the only real work;
the routing is plumbing.

The `_formatAthleteData` port from `stats.mjs:2050+` is
where most of the field-parity effort goes. Approach:
read the JS function once, list every field
assignment, and write a Rust function with the same
names and the same unit choices. The test is a single
fixture `AthleteData` value with hand-derived field
values; it asserts the resulting JSON object equals a
hand-written golden value via `serde_json::Value`
equality, so field ordering does not matter.

### 17.10 — 17.11 v2 endpoints

`webserver.mjs:336-362` defines the v2 query
semantics: `resource` is repeatable
(`?resource=stats&resource=lap`); `stats` is a
boolean. The v2 endpoint applies the filter to the
v1-shaped value: `let v1 = formatter(); v1.filter(
resources, stats)`.

`actix_web::web::Query<T>` does not natively bind a
repeated key to `Vec<T>`. The options are pulling in
`serde_qs::actix::QsQuery<T>` or hand-rolling a struct
that implements `FromRequest` against
`req.uri().query()`. Hand-roll it; the dependency is
not worth one query parameter.

The filter is a whitelist of top-level keys:
`stats | state | athlete | lap | lastLap | laps |
segments | events | timeInPowerZones`. A single helper
walks a `serde_json::Value` and returns a new value
containing only the requested keys.

### 17.12 — 17.14 RPC

Three transport surfaces share one handler registry:

- WebSocket (`method: "rpc"`).
- `GET /api/rpc/v1/:name?arg=...&arg=...` — types
  inferred from the lexical form (number / bool /
  string).
- `POST /api/rpc/v1/:name` with a JSON array body.
- `GET /api/rpc/v2/:name*` with a base64url-encoded
  JSON tail.

The registry is `HashMap<&'static str, Arc<dyn
RpcHandler>>`, where `RpcHandler` is
`async fn(args: Vec<Value>) -> Result<Value, RpcError>`.
Share it with handlers as `web::Data<RpcRegistry>` (an
`Arc<T>` underneath); the WebSocket per-client task
gets a clone of the `Data` handle.

The only handler STEP 17 itself registers is
`getVersion`, which returns `env!("CARGO_PKG_VERSION")`.
That is enough to exercise the dispatch path
end-to-end in tests without depending on any
unimplemented surface. Later steps register the real
handlers (`getWebWindowManifests`, `setAthleteFTP`,
and so on).

### 17.15 — 17.19 WebSocket protocol

The codec is the most error-prone piece because the
wire is asymmetric — accept either the nested or the
flat request shape, but always emit the canonical
flat form. The codec is three free functions on
`web::ws::frame`:

```rust
pub fn decode_request(bytes: &[u8]) -> Result<RequestFrame, FrameError>;
pub fn encode_response(uid: u64, body: ResponseBody) -> String;
pub fn encode_event(sub_id: u64, data: &serde_json::Value) -> String;
```

`decode_request` accepts either form, any field
order, and ignores unknown extra fields.
`encode_response` and `encode_event` serialise small
named structs so the field order is fixed at compile
time and no extra fields can leak out.

`RequestFrame`:
```rust
pub struct RequestFrame {
    pub uid:    u64,
    pub method: Method,           // Subscribe | Unsubscribe | Rpc
    pub arg:    serde_json::Value,
}
```

`ResponseBody`:
```rust
pub enum ResponseBody {
    Success(serde_json::Value),
    Failure(String),              // error message
}
```

Test the codec in isolation before wiring up the
per-client task; once those tests pass, the
integration tests exercise the codec through a real
socket.

The per-client task uses `actix_ws::handle(&req,
body)` to take ownership of the WebSocket. It returns
`(HttpResponse, Session, MessageStream)`. The handler
returns the `HttpResponse` right away (which
completes the upgrade) and
`actix_web::rt::spawn(client_task(session, msg_stream,
shared))` runs the read/write loop on the actix
runtime.

### 17.20 — 17.22 Subscription engine

Three layers:

1. **Per-client subscriptions.** Owned by the client
   task as `HashMap<SubId, ClientSub>`, so the
   disconnect path can walk and release each one.
   `ClientSub` carries the source name, event path,
   options hash, and an `Arc` on the delegation.

2. **Process-wide delegations.**
   `Mutex<HashMap<(SourceId, EventId, OptionsHash),
   Arc<Delegation>>>`. A `Delegation` owns the
   upstream listener handle and a
   `tokio::sync::broadcast::Sender<Bytes>` of
   pre-encoded event frames. The reference count is
   `Arc::strong_count` on the `Delegation`; when the
   last client drops its `Arc`, the delegation is
   removed and the upstream listener released.

3. **Sources.** Each implements `EventSource`:
   ```rust
   pub trait EventSource: Send + Sync {
       fn subscribe(
           &self,
           event:   &str,
           options: &SubscriptionOptions,
           sink:    mpsc::Sender<Bytes>,
       ) -> Result<UpstreamListener, SubError>;
   }
   ```
   The `stats` source is the only one STEP 17
   implements. It subscribes to the daemon-internal
   stats broadcast and filters by event path.

`OptionsHash` is the SHA-256 of the canonical JSON
encoding of `options` (with sorted keys). Two
subscriptions with the same options share a
delegation; two with different options do not.

### 17.23 — 17.25 Static files

`actix_files::Files` does the heavy lifting. The
work is in the per-subdirectory cache-control
headers and the MIME-type override for `.mjs`.

Cache-control headers go on through
`actix_web::middleware::DefaultHeaders` wrapped
around nested `Files` scopes:

- `/pages/images` — `private, max-age=3600`
- `/pages/deps/flags` — `private, max-age=8640000`
- `/pages/fonts` — `private, max-age=8640000`

Register the nested mounts before the broader
`/pages` mount so actix's longest-prefix routing
hits the specific scope first.

The `.mjs` override goes through
`Files::mime_override`. Some `mime_guess` versions
return `application/javascript` for `.mjs`;
sauce4zwift returns `text/javascript`. Forcing the
latter keeps widget content-type sniffing
identical.

### 17.26 HTTPS

The HTTPS path is conditional:

1. Try to read `{cert_dir}/cert.pem` and
   `{cert_dir}/key.pem`.
2. If both exist and parse as a valid certificate
   and key, chain a second
   `HttpServer::bind_rustls_0_23(addr,
   server_config)` call onto the same `HttpServer`.
   The HTTPS address is `server_port + 1`.
3. If either is missing, log a warning at lifecycle
   level (so it shows up in the daemon log by
   default) and skip the HTTPS bind.
4. If both exist but fail to parse, fail the daemon
   start. An operator who configured HTTPS with
   broken certs is better served by a clear failure
   than by silently serving HTTP only.

The test uses `rcgen` to generate a self-signed
cert into a `tempfile::TempDir` and points the
daemon at that directory.

### 17.27 Backpressure

`webserver.mjs:170-172` checks `ws.bufferedAmount >
8 MB` on every emission. `actix-ws::Session` does
not expose an equivalent counter, so the Rust
analogue is a little more work:

- Each client task owns an `mpsc::Sender<Bytes>` of
  bounded capacity (default 256 frames). The write
  loop drains the receiver and writes to the
  socket via `session.text(...)` or
  `session.binary(...)`. A slow socket fills the
  channel.
- A separate `BufferedBytes(AtomicUsize)` per
  client tracks the sum of `frame.len()` for
  everything in flight (queued in the channel plus
  waiting to flush). The subscription dispatch
  reads this counter before pushing; if the value
  crosses 8 MB, it logs and the client task calls
  `session.close(Some(CloseReason::from(
  CloseCode::Policy))).await` to shut the socket
  down cleanly.

The threshold is configurable via
`[server] max_buffered_bytes = N` in the TOML;
default `8 388 608` (8 MB).

Dropping frames silently would be simpler but
would also leave widgets out of sync with the
daemon's state, which is harder to debug than a
closed connection.

### 17.28 — 17.33 proto-to-stats translation

The notes on each item are above; this is a quick
recap:

- **17.29 course-change handling.** Overwrite
  identity on every upsert (option a in §17.29-I)
  unless a captured trace argues otherwise.

- **17.30 `PlayerStateView` for proto.** Add the
  impl. Use it in production; keep `MostRecentState`
  for tests and the in-memory snapshot.

- **17.31 EventBehavior config.** Defaults match
  `stats.mjs:884-887` (both `true`); the file
  schema exposes the two booleans under
  `[stats]`.

- **17.32 / 17.33 GC tick.** Read `stats.mjs:3553`
  once, confirm the constant, add a doc-comment
  that cites the source line. The 10 s figure in
  the previous stub looks like a confused
  reference to `_zwiftMetaRefresh` at
  `stats.mjs:3565`; record the rejection
  alongside the constant.

### 17.34 — 17.35 Wiring

The web server is the third long-lived subsystem
the daemon starts, after `Stores` from STEP 16 and
the relay runtime from STEP 12. Three subsystems
makes it worth introducing a small `Subsystems`
struct on the daemon stack. STEP 16 (§AB-4)
deferred this until "a second long-lived
subsystem", which STEP 17 provides. The struct
keeps the `run_daemon` signature manageable and
makes the shutdown order explicit.

`HttpServer::run()` returns a `Server` future whose
`handle()` method yields a clonable
`actix_web::dev::ServerHandle`. The daemon's
`web::start()` keeps the handle for graceful
shutdown (`handle.stop(true).await`) and spawns
the `Server` future on the daemon's tokio runtime
via `tokio::spawn`. No separate actix-rt system is
created; actix-web 4 uses `actix-rt` internally,
which is a thin wrapper over tokio, and
`HttpServer::run()` is spawnable from any tokio
context.

The status printer gains a `Web server:` block in
the same shape as STEP 16's `Persistence:` block:

```
Web server:
  bind         127.0.0.1
  port         1080
  https        enabled (port 1081)
  connections  3
```

The not-running branch reads `bind`, `port`, and
`https` from `ResolvedConfig` and prints
`connections` as `daemon not running`.

## Decisions

- **actix-web 4.** Has the WebSocket support
  (via `actix-ws`), middleware system (via
  `.wrap(...)`), and built-in HTTPS binding the
  step needs. It runs on `actix-rt`, a thin
  wrapper over tokio, so `HttpServer::run()` is
  spawnable on the daemon's existing tokio
  runtime — no separate executor is introduced.
  axum 0.7 is the other realistic option;
  capability-wise they are close, but the actix
  ecosystem ships `actix-files`, `actix-cors`,
  and `actix-ws` as named crates that line up
  with the Express + ws pieces sauce4zwift uses,
  which makes cross-referencing easier.

- **`actix-ws`** for the WebSocket path rather
  than the actor-based `actix-web-actors::ws`.
  `actix-ws` exposes a small
  `handle(&req, body) -> (HttpResponse, Session,
  MessageStream)` API that maps directly onto the
  per-client task model. The actor variant
  requires a separate `Handler` impl per message
  type and a different way of scheduling work
  (`ctx::run_interval`), which is more setup than
  this protocol needs.

- **`actix-files`** for static files, not a
  hand-rolled file server. The per-subdirectory
  cache headers (§17.23-I) go on as middleware
  via `actix_web::middleware::DefaultHeaders`
  wrapped around nested `Files` scopes.

- **`actix-cors`** for CORS, layered via
  `.wrap(...)` on the `/api`, `/pages`, and
  `/shared` scopes.

- **`HttpServer::bind_rustls_0_23`** for the
  HTTPS listener instead of a separate
  TLS-wrapping crate. Needs the `rustls-0_23`
  feature on `actix-web`. The HTTP and HTTPS
  listeners share the same `App` factory through
  chained `.bind(addr)?.bind_rustls_0_23(addr,
  cfg)?` calls before `.run()`.

- **`rustls`** (via `rustls-pemfile` and the
  `actix-web` `rustls-0_23` feature) rather than
  `native-tls` or `openssl`. Pure-Rust TLS keeps
  the dependency tree small and avoids the
  OpenSSL build burden on macOS.

- **`base64::engine::general_purpose::URL_SAFE_NO_PAD`**
  for the v2 RPC tail decoder, matching
  sauce4zwift's `Buffer.from(s, 'base64url')`.

- **WebSocket compression deferred.**
  `webserver.mjs:175-178` uses per-message
  deflate; this step does not. Compression
  interacts awkwardly with the three-buffer
  write pattern (the deflate state is
  per-message, not per-buffer), and the widget
  tree works fine without it on localhost.
  Revisit if a deployment puts the daemon
  behind a real network link; see §"Out of
  scope".

- **One `App` factory shared between HTTP and
  HTTPS.** Cleaner than running two
  `HttpServer` instances with duplicated route
  tables — chained `.bind` calls produce a
  single `Server` that listens on multiple
  sockets and dispatches to the same `App`.

- **Per-client task model.** Each WebSocket
  connection spawns one tokio task (via
  `actix_web::rt::spawn`) that owns the read
  loop, write loop, and per-client `subs` map.
  The alternatives — a single shared task with
  per-client mailboxes, or the actor model with
  a `Handler<Message>` per message type — both
  make backpressure harder to reason about.

- **`web::start()` returns a handle, not a
  future.** The daemon owns the handle and
  calls `shutdown().await` on it during
  teardown. The handle holds the spawned
  server's `JoinHandle` and the
  `actix_web::dev::ServerHandle` used to issue
  the graceful stop.

- **Web server lives in the root binary, not a
  sibling crate.** It reads daemon-internal
  state (`AthleteRegistry`,
  `broadcast::Receiver<GameEvent>`,
  `ResolvedConfig`) and has no use outside
  ranchero. STEP 16 went the other way for
  `zwift-store` because persistence is a
  stand-alone library; the web server is the
  inverse — daemon context is the whole point.

## Module layout

```
src/web/
    mod.rs                  — re-exports, WebError, WebServerHandle
    server.rs               — start(), shutdown(), HttpServer factory
    state.rs                — SharedState (registry handle, sources,
                              delegations, source registry)
    proto_view.rs           — impl PlayerStateView for zwift_proto::PlayerState
    proto_to_stats.rs       — GameEvent::PlayerState → registry routing
    http/
        mod.rs              — App configuration entry point
        api_directory.rs    — /api/ and /api/* fallthrough
        athlete.rs          — /api/athlete/{v1,v2}/:id
        nearby.rs           — /api/nearby/{v1,v2}
        groups.rs           — /api/groups/{v1,v2}
        rpc.rs              — /api/rpc/{v1,v2}/:name and discovery
        static_files.rs     — /pages/*, /shared/*, root index, MIME override
        cors.rs             — actix_cors::Cors configuration
        format/
            athlete.rs      — _formatAthleteData{,V2} port
            nearby.rs       — _formatNearby port
            groups.rs       — _formatGroups port
            filter.rs       — resource filter helper
    ws/
        mod.rs              — /api/ws/events upgrade handler
        frame.rs            — decode_request, encode_response, encode_event
        client.rs           — client_task (read loop, write loop, subs)
        backpressure.rs     — BufferedBytes counter + threshold check
    subs/
        mod.rs              — re-exports
        registry.rs         — Delegations map, dedup, ref-count cleanup
        source.rs           — EventSource trait, SourceRegistry
        stats_source.rs     — built-in `stats` source
        game_connection_source.rs — built-in `gameConnection` source
    handlers/
        get_version.rs      — the one RPC handler STEP 17 registers
tests/
    web_server_bind.rs                   (ignored: slow)
    web_config.rs
    http_get_api_root.rs
    http_cors_preflight.rs
    http_athlete_v1.rs
    http_athlete_v2.rs
    http_nearby_v1.rs
    http_nearby_v2.rs
    http_groups_v1.rs
    http_groups_v2.rs
    http_rpc_v1.rs
    http_rpc_v2.rs
    http_rpc_discovery.rs
    http_404_fallthrough.rs
    http_static_pages.rs
    http_root_index.rs
    http_mime_types.rs
    ws_handshake.rs                      (ignored: slow)
    ws_request_response.rs
    ws_subscribe_event.rs
    ws_uid_isolation.rs
    ws_disconnect_cleanup.rs
    ws_backpressure.rs                   (ignored: slow)
    subs_source_lookup.rs
    subs_delegation_dedup.rs
    subs_event_payload.rs
    https_conditional.rs                 (ignored: slow)
    proto_to_stats_routing.rs
    registry_upsert_course_change.rs
    player_state_view_for_proto.rs
    event_behavior_from_config.rs
    gc_tick_runs_on_interval.rs
    gc_interval_documented.rs
    daemon_starts_web_server.rs          (ignored: slow)
    cli_status_includes_web.rs           (ignored: slow)
```

## Public API surface

```rust
// src/web/mod.rs
pub use server::{start, WebServerHandle};
pub use state::SharedState;

#[derive(Debug)]
pub enum WebError {
    Io(std::io::Error),
    Bind(std::net::AddrParseError),
    Tls(rustls::Error),
    MissingCert(PathBuf),
    Config(String),
}
```

```rust
// src/web/server.rs
pub fn start(
    cfg:      &ResolvedConfig,
    state:    SharedState,
    shutdown: Arc<tokio::sync::Notify>,
) -> Result<WebServerHandle, WebError>;

pub struct WebServerHandle {
    pub http_addr:   SocketAddr,
    pub https_addr:  Option<SocketAddr>,
    pub connections: Arc<AtomicUsize>,
    server:          actix_web::dev::ServerHandle,
    join:            tokio::task::JoinHandle<std::io::Result<()>>,
}

impl WebServerHandle {
    pub async fn shutdown(self) -> Result<(), WebError>;
}
```

```rust
// src/web/state.rs
pub struct SharedState {
    pub registry:     Arc<RwLock<AthleteRegistry>>,
    pub watched:      Arc<AtomicI64>,
    pub game_events:  broadcast::Sender<GameEvent>,
    pub stats_events: broadcast::Sender<StatsEvent>,
    pub sources:      SourceRegistry,
    pub rpc_handlers: RpcRegistry,
    pub config:       Arc<ResolvedConfig>,
}
```

```rust
// src/web/subs/source.rs
pub trait EventSource: Send + Sync {
    fn name(&self) -> &'static str;
    fn subscribe(
        &self,
        event:   &str,
        options: &SubscriptionOptions,
        sink:    mpsc::Sender<Bytes>,
    ) -> Result<UpstreamListener, SubError>;
}

pub struct SubscriptionOptions {
    pub resources: Option<Vec<String>>,
    pub stats:     bool,
    pub raw:       serde_json::Value,
}
```

```rust
// src/web/ws/frame.rs
pub fn decode_request(bytes: &[u8]) -> Result<RequestFrame, FrameError>;
pub fn encode_response(uid: u64, body: ResponseBody) -> String;
pub fn encode_event(sub_id: u64, data: &serde_json::Value) -> String;

pub struct RequestFrame {
    pub uid:    u64,
    pub method: Method,
    pub arg:    serde_json::Value,
}

pub enum Method { Subscribe, Unsubscribe, Rpc }

pub enum ResponseBody {
    Success(serde_json::Value),
    Failure(String),
}
```

## Wire format reference

### Inbound request

What every widget sends:

```json
{
  "type": "request",
  "uid":  42,
  "data": {
    "method": "subscribe" | "unsubscribe" | "rpc",
    "arg":    { ... }
  }
}
```

The decoder also accepts the spec-shaped flat form
`{"type":"request", "method":..., "uid":..., "arg":...}`,
any field order, and silently ignores unknown extra
fields. Examples below use the nested form.

`subscribe` arg:
```json
{
  "event":   "athlete/watching",
  "source":  "stats",
  "subId":   7,
  "options": { "resources": ["stats", "lap"], "stats": true }
}
```

`unsubscribe` arg:
```json
{ "subId": 7 }
```

`rpc` arg:
```json
{ "name": "getVersion", "args": [] }
```

### Outbound response

Success:
```json
{ "type": "response", "success": true, "uid": 42, "data": ... }
```

Failure:
```json
{ "type": "response", "success": false, "uid": 42, "error": "..." }
```

### Outbound event

```json
{ "success": true, "type": "event", "uid": 7, "data": ... }
```

The encoder emits fields in the order
`{success, type, uid, data}` so byte-for-byte capture
comparisons line up. Tests parse the JSON back rather
than diff strings.

## Wiring into the workspace

- Root `Cargo.toml`:
  add `actix-web = { version = "4", features = ["rustls-0_23"] }`,
  `actix-cors = "0.7"`, `actix-files = "0.6"`,
  `actix-ws = "0.3"`, `rustls = "0.23"`,
  `rustls-pemfile = "2"`, `mime_guess = "2"`,
  `base64 = "0.22"`, `bytes = "1"`,
  `futures-util = "0.3"`, and `rcgen` (dev-only,
  for the HTTPS test).
- `src/config/mod.rs`:
  add `pages_root: Option<PathBuf>`,
  `https_cert_dir: Option<PathBuf>`, and
  `max_buffered_bytes: Option<u64>` to
  `ServerConfig`. Add a new `StatsConfig` section
  with `auto_reset_events` and `auto_lap_events`.
  Plumb every new field through `ResolvedConfig`
  with the `RANCHERO_*` env-var override
  convention.
- `src/daemon/runtime.rs`:
  introduce a `Subsystems` struct that owns
  `Stores`, `RelayRuntime`, and `WebServerHandle`.
  The shutdown order is web → relay → stores
  (reverse of the boot order). `run_daemon` takes
  a `Subsystems` by value.
- `src/daemon/mod.rs`:
  add a `Web(WebError)` variant to `DaemonError`.
- `src/daemon/control.rs`:
  add a `web: WebStatus` field to `StatusResponse`
  carrying bind, port, HTTPS state, and current
  connection count.
- `pages/`:
  vendor sauce4zwift's `pages/` tree into the
  workspace root. The vendoring is a one-time
  copy with no modifications; later maintenance
  happens in-tree. Record the source commit hash
  in `pages/VENDOR.md`.

## Acceptance criteria

- `cargo test` (fast set) is green;
  `cargo test -- --ignored` (slow set) is also
  green.
- `ranchero start` on a clean machine binds the
  configured port, serves `pages/index.html` at
  `GET /`, and accepts a WebSocket connection at
  `/api/ws/events`.
- The vendored widget tree works unchanged:
  subscribe / unsubscribe / rpc flows succeed and
  the widgets render live data when the relay
  delivers `GameEvent::PlayerState` frames.
- `ranchero status` prints a `Web server:` block
  listing bind, port, HTTPS state, and active
  connection count.
- A WebSocket client that stops reading is
  dropped once its outbound buffer crosses 8 MB;
  other clients are unaffected; a tracing event
  records the disconnect at lifecycle level.
- A POST `application/json` body to
  `/api/rpc/v1/getVersion` returns
  `{"success":true,"data":"<crate version>"}`.
- HTTPS auto-enables when the cert pair exists
  at `{server.https_cert_dir}/{cert,key}.pem`;
  the daemon logs the absence at lifecycle level
  when the certs are missing.
- No code outside `src/web/` opens an HTTP
  listener.
- No path the web server resolves goes through
  the `sauce4zwift` symlink (verified by grep on
  the loaded static-file paths during the
  integration test).

## Out of scope for STEP 17

Items below are not done in this step. Each is
recorded either here or in
[STEP-20-additional-considerations.md](STEP-20-additional-considerations.md)
with a rule for when to come back to it.

- **Per-message WebSocket compression.** Defer to
  a step that introduces a non-localhost
  deployment scenario. The three-buffer write
  pattern sauce4zwift uses to avoid re-encoding is
  also not done; the Rust encoder is fast enough
  to encode the full frame on every emission at
  the expected traffic volume.
- **Mod web roots and the mod-management
  surface.** Ranchero has no mod loader yet; the
  mod-management RPCs and the
  `/mods/<mod-id>/` static mounts wait for a
  step that introduces mods.
- **Native window manifests
  (`window-manifests.json`,
  `getWebWindowManifests`).** Ranchero has no
  native window manager equivalent to Electron's
  `BrowserWindow`; the RPC stays unregistered.
- **Browser-source assets and the patron / EULA
  pages.** They will be vendored into `pages/`
  because the tree is copied wholesale, but no
  route or RPC supports them functionally.
  Update if a future step introduces a
  browser-source workflow.
- **HTTPS certificate provisioning.** Operators
  bring their own certs. ACME or Let's Encrypt
  integration is a later step.
- **Resource-filter parity for every v2 endpoint
  field.** The filter in this step is a
  whitelist of top-level keys. Deeper filtering
  (for example, `resource=lap.distance`) belongs
  in STEP 18 alongside the v2 formatter port.
- **WebSocket authentication.** Sauce serves the
  WebSocket with no auth (loopback only by
  default). Ranchero matches; binding to
  `0.0.0.0` is the operator's responsibility.
- **The `/api/athlete/streams/v1/:id` route.**
  Stream data lives on `AthleteData::streams`
  (added in STEP 14), but the wire format
  requires the STEP 18 formatter; the HTTP route
  waits for that step.
- **The `/api/athlete/laps`, `/segments`, and
  `/events` routes.** Same reason as `/streams`
  — the data is there, the formatter belongs to
  STEP 18.

## Things to confirm before starting implementation

Quick reads to sanity-check before writing code.
If any of these is wrong, the matching checklist
item needs an amendment.

- **`stats.mjs:3553` defines the GC tick
  interval as `62.768` (or its JS equivalent).**
  Confirm before pinning the constant in
  §17.33-I.
- **Defaults for `_autoResetEvents` and
  `_autoLapEvents` at `stats.mjs:884-887`.**
  STEP 15 recorded both as `true`; reconfirm
  against the current upstream source before
  pinning §17.31-I defaults.
- **`webserver.mjs:264` endpoint path.**
  Pinned in §"Three differences" as
  `/api/ws/events`; one last read confirms the
  source has not changed.
- **`_formatAthleteData` location.** The plan
  cites `stats.mjs:2050+` from the survey;
  reconfirm the line range against the upstream
  tree at implementation time, since unrelated
  edits may have moved it.
- **`webserver.mjs:236-251` cache-control
  directives.** Reconfirm the three directories
  (`images`, `deps/flags`, `fonts`) and their
  `max-age` values before §17.23-I.
- **`actix-web` 4 `rustls-0_23` feature.**
  Confirm the feature flag name and the
  `HttpServer::bind_rustls_0_23` signature
  against the current `actix-web` release; the
  method has been renamed across minor versions
  (`bind_rustls` → `bind_rustls_021` →
  `bind_rustls_0_22` → `bind_rustls_0_23`) as
  rustls itself moves.
- **`actix-ws` 0.3 API stability.** Confirm
  `actix_ws::handle` is still the entry point
  and still returns `(HttpResponse, Session,
  MessageStream)`. The crate is post-1.0
  candidate but pre-1.0 in version, so minor
  breaking changes are possible.
- **`actix-web::rt::spawn` from inside a tokio
  runtime.** Confirm that spawning from inside
  an `actix_web` request handler, when the
  server itself is hosted on a vanilla tokio
  runtime, works without a double-runtime
  panic. The plan assumes it does because
  `actix-rt` is a thin wrapper over tokio;
  verify with one integration test before
  building the per-client task on top.
- **`mime_guess` default for `.mjs`.** If the
  upstream default already returns
  `text/javascript`, drop the override in
  §17.25-I.
- **`AthleteRegistry::iter()` is the right read
  path for the HTTP nearby and groups
  handlers.** STEP 14 exposes it
  (`crates/zwift-stats/src/athlete.rs`).
  Confirm there is no concurrency issue with
  reading while the GC tick is mutating — the
  registry sits behind `Arc<RwLock<_>>` in
  `SharedState`, so it should be fine, but
  check that the STEP 15 detector tests do not
  assume otherwise.
- **The captured `GameEvent::PlayerState`
  variant is the only thing the proto-to-stats
  router needs.** Other variants (`Latency`,
  `StateChange`, `PoolSwap`) feed different
  sources (`gameConnection`, diagnostics);
  confirm none of them carries data the
  `AthleteRegistry` needs.
