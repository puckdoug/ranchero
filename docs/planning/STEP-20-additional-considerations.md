# Step 20 — Additional considerations (parking lot)

## Purpose

A running list of items to consider later. These items surface during
earlier-step work but do not justify pausing the current step to
resolve. Each entry should be self-contained: where it came from, what
the trade-off looks like, and when to revisit.

Triage when starting any new step: any item here that the new step
naturally touches gets pulled into that step's elaboration. Items left
behind here are either accepted or revisited at the end of the
porting effort.

---

## Open items

### 20.1 — Virtual-time vs. real-time in async HTTP tests (from STEP 07)

**Where it came from.** The
`preemptive_refresh_fires_at_half_expires_in` test in
`crates/zwift-api/tests/auth.rs` originally used
`#[tokio::test(flavor = "current_thread", start_paused = true)]` plus
`tokio::time::advance(...)` so the half-life elapsed in virtual time
without a real-world wait. It deadlocked: after the scheduled
`tokio::time::sleep(expires_in / 2)` woke, the spawned refresh task
issued a `reqwest` round-trip to wiremock, which needs the IO driver
to make progress; however, on a `current_thread` runtime the reactor
only turns when the runtime parks, and the test task was busy
yielding, so the runtime never parked.

**Current resolution.** The test uses a 2 s `expires_in` (1 s
half-life) and a real `tokio::time::sleep(Duration::from_millis(2000))`.
This adds approximately 2 s of wall time to the suite and uses no
virtual-time machinery. A comment in the test explains the choice.

**Why this might come back.** Subsequent steps add more
time-driven background tasks against mock HTTP servers:

- STEP 09 — relay session refresh at ~90% of session lifetime.
- STEP 10 / 11 — UDP/TCP channel watchdogs (>30 s silent → reconnect),
  exponential backoff on reconnect.
- STEP 12 — `GameMonitor` supervision and reconnect cadence.

If several real-time waits accumulate to a noticeable suite slowdown
(for example, more than 5 s aggregate), revisiting is warranted.
Options:

1. **`flavor = "multi_thread"` + manual `tokio::time::pause()`** after
   the mock server is up. The IO driver runs on a worker thread, so
   reqwest can make progress while the test task yields. Cost: a
   `std::time::Instant` deadline loop in the test (since `tokio::time`
   is paused), which is awkward.
2. **Inject the clock and sleeper.** A `trait Clock` / `trait Sleeper`
   abstraction in `zwift-api` (and any other crate that schedules
   work) would let tests substitute a deterministic in-memory
   implementation, with no real sleeps and no interaction between
   virtual time and IO. Cost: an extra abstraction layer in
   production code, paid for by every consumer of the crate.
3. **Status quo.** Accept short real-time sleeps as the cost of
   testing time-driven behavior end-to-end through real `reqwest`
   and wiremock. Cost: the suite is a few seconds slower per such
   test.

**Decision rule.** Revisit when (a) total real-time test wait crosses
approximately 5 s, or (b) a flaky failure appears tied to scheduling
jitter on CI. Until then, the status quo is retained.

### 20.2 — Shared inbound-decode helper between UDP and TCP channels (from STEP 11)

**Where it came from.** STEP 11's plan recommended extracting
`process_inbound` (header decode → relay_id validation → IV state
mutation → AES-128-GCM-4 decrypt → `ServerToClient::decode`) into
a private module shared by `udp.rs` and `tcp.rs`. The two copies of
the function differ only in one constant: `ChannelType::UdpServer`
versus `ChannelType::TcpServer` in the IV construction.

**Current resolution.** The function was not extracted. Two
near-identical copies of `process_inbound` reside in
`crates/zwift-relay/src/udp.rs` and `crates/zwift-relay/src/tcp.rs`.
A shared helper parameterized on channel type would add one
indirection (passing the channel type as a parameter, or as a
generic) for one line of difference; this provides little value at
this step.

**Why this might come back.**

- A third channel type appears (the companion-app reverse channel is
  spec §6 out-of-scope today, but is listed there).
- The two copies begin to diverge; for example, one channel adds
  inbound envelope handling, error retry, metrics counters, or trace
  spans that the other does not need. At that point, either the
  divergence is real and the helper would have hidden it, or the
  divergence is a defect introduced by editing one copy and
  forgetting the other.
- A reviewer identifies the duplication as a code smell.

**Decision rule.** Extract when (a) the two copies have diverged
beyond the `ChannelType` constant in a way that would have been
caught by a shared helper, or (b) a third channel type is
implemented. Until then, the duplication is the lower-cost choice.

### 20.3 — HTTP-client and policy-string injection beyond URL override (from STEP-12.5 §F)

**Where it came from.** STEP-12.5 §F.3 closed the testability gap
on `RelayRuntime::start` by adding URL-only injection for the
Zwift auth and game-API endpoints: a `[zwift]` section in the
config file, `RANCHERO_ZWIFT_AUTH_BASE` and
`RANCHERO_ZWIFT_API_BASE` environment variables, and a
`zwift_endpoints` field on `ResolvedConfig`. Two larger
redesigns were considered alongside that work and deliberately
excluded from §F so they could be evaluated on their own merits
rather than introduced as a side effect of the testability fix.
STEP-12.5 §F.5 records the exclusion; this parking-lot entry is
the place to revisit it.

**Current resolution.** Both items are deferred. Neither is
required for the operator-facing capability or the test
infrastructure produced by §F.

1. **Injecting a higher-level HTTP-client trait into
   `ZwiftAuth`.** `zwift_api::ZwiftAuth` constructs a
   `reqwest::Client` internally.
   `ZwiftAuth::with_client(http, config)` already exists so
   callers can share a `reqwest::Client` for connection pooling
   across multiple instances (for example, the main and monitor
   accounts in a future multi-account configuration). A
   trait-based HTTP client would let tests substitute an
   in-memory transport and bypass `reqwest` and wiremock
   entirely, but URL-only injection — already exercised by
   every test in `crates/zwift-api/tests/auth.rs` and
   `crates/zwift-relay/tests/session.rs` — is sufficient to
   keep ranchero's tests away from production Zwift endpoints
   and matches the pattern the rest of the workspace uses.
2. **Surfacing `source` and `user_agent` to operator
   configuration.** These two `zwift_api::Config` fields
   default to `"Game Client"` and `"CNL/4.2.0"`. They are
   policy values that mimic Zwift's own client and have no
   operator-relevant effect on testability or
   staging-environment redirection. §F.3 leaves them at the
   library defaults rather than expanding the schema for
   fields no current deployment needs.

**Why this might come back.**

- A future spec or behaviour change requires Zwift identifying
  the client differently — for example, a self-hosted relay
  that refuses connections without a custom user agent, or a
  per-deployment differentiation scheme. At that point
  `source` and `user_agent` need an operator-facing knob and
  the schema work in §F.3.1 / §F.3.2 is the natural place to
  add them.
- Test infrastructure outgrows wiremock. Examples that would
  push toward a trait-based HTTP client: asserting request
  headers in a way wiremock does not support, injecting
  HTTP-level latency to exercise retry and backoff paths, or
  exercising connection-pool behaviour without real sockets.
- A consumer of `zwift-api` outside of ranchero needs to mock
  HTTP at a level wiremock cannot reach.

**Decision rule.** Revisit when (a) operator configuration of
`source` or `user_agent` is required by a real deployment
scenario, or (b) test infrastructure needs to substitute the
HTTP client itself, not just its target URL. Until then, the
URL-only injection plus `ZwiftAuth::with_client` is the
lower-cost choice.

### 20.4 — Configuration extensibility (from STEP-02, STEP-02.1)

**Where it came from.** Three items declared deferred in earlier
configuration work and never picked up:

- **Schema-version migrations.** STEP-02 deferred the migration
  story until a v2 schema actually exists. The current
  `serde`-derived parser reads a v1 schema only.
- **Configuration categories beyond v1.** STEP-02 listed mods,
  route overrides, and other sauce-only categories as
  out-of-scope until a real consumer needs them.
- **`--editing-mode` command-line flag.** STEP-02.1 added the
  `editing_mode = "default" | "vim"` field to the configuration
  file but deferred the corresponding CLI flag. Today the
  configuration file is the only way to choose.

**Current resolution.** The configuration parser in `src/config/`
accepts the v1 schema. There is no migrator and no v2 schema.
The TUI honours `editing_mode` from the file; no CLI override
exists.

**Why this might come back.** A deployment that needs per-mod
or per-route configuration, a schema change that is not
backwards-compatible with v1, or an operator who wants to switch
between vi and default editing modes without rewriting the
configuration file.

**Decision rule.** Revisit each sub-item when a concrete
deployment requirement appears. The migration framework only
becomes worth building once a v2 field is actually being added;
until then, a one-line `version = 1` check is enough.

### 20.5 — TUI vi-mode completeness and mouse support (from STEP-02.1, STEP-02.2)

**Where it came from.** STEP-02.2 ported a subset of vi
navigation; several motions and editing operations were
deferred, alongside two TUI-input items from STEP-02.1.
Specifically:

- `gg` and `G` (jump to first / last screen).
- `0`, `$`, `^` line motions in outer Normal mode.
- Numeric prefix counts (`3j`, `5l`, `2dd`).
- `c{motion}` and `s` (change and substitute).
- Cross-field paste from the edtui clipboard into the outer
  paste buffer (a `dw` inside an edtui field today does not
  populate `paste_buffer` in `src/tui/model.rs`).
- Custom `:` commands beyond the documented set
  (`:w`, `:wq`, `:x`, `:q`, `:q!`, `:u`, `:undo`).
- Redo (`Ctrl-R` / `:redo`).
- Mouse support, resize handling beyond ratatui's defaults,
  mouse cursor positioning within fields, and click-to-focus.
- Visual-mode selector widget for the log-level enumeration
  (currently a free-text field in the configuration TUI).

**Current resolution.** The TUI driver implements the subset
that covers everyday configuration editing. None of the items
above are present in `src/tui/`. The TUI runs in keyboard-only
mode; mouse events are ignored.

**Why this might come back.** A user who relies on full vi
muscle memory finds the gaps disruptive, or a deployment
context (for example, a remote SSH session over a
mouse-capable terminal) makes the keyboard-only choice
awkward. A reviewer who consistently expects a working
`Ctrl-R` would also push toward this.

**Decision rule.** Add motions on demand: when an item is
requested by a real user with a concrete workflow that the
omission blocks, port the equivalent edtui or
`tui-input`-level binding. The full mouse track-and-click set
is a larger piece of work; defer until the keyboard-only
choice is contested by a real user, not by a hypothetical
preference.

### 20.6 — Syntax highlighting in the Review-screen TOML preview (from STEP-02.1)

**Where it came from.** STEP-02.1 deferred syntax highlighting
in the Review screen's read-only TOML preview, on the basis
that a plain monospace render is sufficient for sanity-checking
a configuration before save.

**Current resolution.** The preview pane renders the serialised
TOML in the default style. No `tree-sitter`, `syntect`, or
grammar reference exists in `src/tui/`.

**Why this might come back.** A configuration grows large
enough that the human eye benefits from coloured key/value
distinction, or a future schema introduces nested tables and
arrays where mismatched delimiters become hard to spot in a
plain render.

**Decision rule.** Defer until the configuration schema crosses
roughly 50 lines in typical use, or a Review-screen rendering
defect is traced to mis-formatted TOML being hard to spot.

### 20.7 — Daemon log rotation, structured output, and shipping (from STEP-03, STEP-04)

**Where it came from.** STEP-03 and STEP-04 deferred three
related logging features:

- **Log rotation.** `src/logging/mod.rs` opens the daemon log
  file with `OpenOptions::new().create(true).append(true)`. No
  `tracing_appender::rolling` consumer is in place. A
  long-running daemon grows the file without bound.
- **JSON / structured log output.** Only `fmt::layer()` is
  configured. No JSON layer, no operator-selectable output
  format.
- **Log shipping to external collectors.** No OTLP, syslog, or
  vector-style sink. Operators who want centralised logging
  have to tail the file from outside the daemon.

**Current resolution.** Operators rotate the log externally
(`logrotate` or equivalent). Structured output is not
available; the daemon writes a human-readable line format only.

**Why this might come back.** The first long-running production
deployment will exhaust disk space without rotation. A
deployment under a centralised log policy will need either JSON
output or a shipping sink. These are operational must-haves
once ranchero is run anywhere other than a developer laptop.

**Decision rule.** Pick this up before the first deployment
intended to run for more than a week without supervision.
`tracing_appender::rolling` plus a `--log-format=json` CLI flag
is the minimum viable response; the shipping piece can wait
until a specific collector is required.

### 20.8 — Cross-platform daemon: Windows service and Linux capability drop (from STEP-03)

**Where it came from.** STEP-03 deferred two
operating-system-specific items:

- **Windows service integration.** The current daemon assumes a
  POSIX `fork`. Windows has no equivalent; a service-control
  shim using `windows-service` would be required.
- **Privileged-capabilities drop on Linux.** When the daemon is
  started by a process with elevated capabilities (for example
  to bind a low port), it should drop everything not strictly
  required after binding. The current process inherits whatever
  the parent had.

**Current resolution.** Ranchero runs on Linux and macOS only.
The daemon does not drop capabilities on Linux. Neither item is
required for the current deployment target (a developer or a
single user running the daemon on their own machine).

**Why this might come back.** A Windows port is requested, or
ranchero is deployed under `systemd` with `AmbientCapabilities`
and a security audit asks for a defence-in-depth capability
drop after `setsockopt`.

**Decision rule.** Windows: defer until a Windows port is on
the roadmap. Capability drop: defer until ranchero is packaged
for `systemd` or another supervisor that grants ambient
capabilities; at that point the drop is two `caps`-crate calls
and a test that verifies effective and permitted sets are empty
after binding.

### 20.9 — `ranchero follow` enhancements (from STEP-12.2)

**Where it came from.** STEP-12.2 implemented a polling
follower for the capture file. Five enhancements were deferred:

- **File-system event notification.** The follower polls. An
  `inotify` (Linux) / `kqueue` (BSD/macOS) watch via the
  `notify` crate would reduce wake-ups and improve latency on
  small writes.
- **Capture-file rotation support.** If the capture writer ever
  rotates (see 20.7), the follower must reopen the new file.
  No reopen logic exists today.
- **JSON output mode for `--decode`.** Today the decoded form
  is human-readable text only.
- **Filter flags.** Direction (inbound/outbound), transport
  (UDP/TCP), and message-type filters are not exposed.
- **"From offset" or "from timestamp" follower mode.** The
  follower starts at end-of-file. Replaying a window from the
  middle of a long capture is not supported.

**Current resolution.** The follower works for the documented
`ranchero start --capture out.cap; sleep 5; ranchero follow
out.cap` flow. Anything beyond that requires external tools.

**Why this might come back.** A debugging session that needs to
inspect a specific traffic class (for example, only TCP
inbound), an automation scenario that pipes JSON into another
tool, or a capture-file rotation choice that breaks the
follower.

**Decision rule.** Pick up filter flags and JSON output the
first time a debugging session would have benefited from them.
File-system event notification is a latency optimisation;
defer until polling becomes a bottleneck. Rotation support
becomes mandatory the same day capture rotation is enabled.

### 20.10 — `RelayRuntime::start_*` consolidation (from STEP-12.11)

**Where it came from.** STEP-12.11 deferred retiring the
`start_inner` and `start_with_deps*` family of entry points.
Each was introduced for a specific test-injection need; the
overlap between them has grown.

**Current resolution.** `src/daemon/relay.rs` exposes
`start_with_all_deps` (used by tests), `start_with_deps`
(legacy), and `start_inner` (legacy). Production code calls
`start`. The duplication is real but stable.

**Why this might come back.** A new injection point is needed
that does not fit any existing entry, forcing yet another
overload, or a refactor of `start_all_inner` exposes the
duplication as a maintenance hazard.

**Decision rule.** Consolidate when (a) a fourth entry point
would otherwise be added, or (b) a behaviour change requires
editing all of `start_inner`, `start_with_deps`, and
`start_with_all_deps` in lock-step. Until then, the
duplication is the lower-cost choice.

### 20.11 — Relay-protocol cosmetic and niche items (from STEP-12.14, STEP-12.15)

**Where it came from.** Three relay-protocol items were
flagged in earlier reviews and explicitly deferred because the
server tolerates the current behaviour or the items did not
unblock the trace they were investigating:

- **Portal-pool handling (STEP-12.14 §k3).** Sauce honours UDP
  pools keyed by a portal `(realm, course)` pair when the
  watched athlete is on a portal road. Ranchero's
  `find_best_udp_server` falls back to the generic `(0, 0)`
  pool. A stub test exists at
  `tests/relay_runtime.rs:portal_pool_handled_via_portal_key`;
  no production code reads the portal key.
- **TCP non-hello flag=0 cleanup and hello SEQNO=0 omission
  (STEP-12.14 §M3 / §k1).** A header-encoding cosmetic
  difference from sauce. The Zwift relay tolerates both.
- **Proto-fork items N1, N12, C11 (STEP-12.15).** Marked in
  STEP-12.15 as "fix only if C5 + C6/7/8 don't unblock the
  trace, and they did".

**Current resolution.** None of the three has any operational
effect on the smoke test or daily use. They remain visible in
the source plans for future reference.

**Why this might come back.** Portal-pool handling becomes
relevant the moment a watched athlete enters a portal road and
a UDP pool actually exists for that portal; the fall-back
behaviour will then send packets to a sub-optimal server. The
cosmetic header items become relevant only if a future relay
server tightens validation.

**Decision rule.** Portal-pool handling: implement once a
captured trace shows portal pools being received but ignored.
Cosmetic items: implement only if a server-side change makes
the current behaviour an error.

### 20.12 — Auth and session resilience: broader retry, error counting (from STEP-12.16)

**Where it came from.** STEP-12.16's "deferred follow-ups"
section called out three resilience gaps that did not block
the smoke test on a healthy first run:

- **Auth and session-login retry across all error categories.**
  `start_with_retry` currently retries only `TcpConnect`,
  `NoUdpConfig`, and `EstablishedTimeout`. A transient 503 from
  the auth endpoint exits the daemon. Sauce retries every
  error category through `_schedConnectRetry`.
- **UDP error-count threshold reconnect.** Sauce's
  `incErrorCount()` (`zwift.mjs:1934-1939`) calls
  `_schedConnectRetry` after every 10 UDP errors. Ranchero has
  no equivalent counter and no equivalent reconnect trigger.
- **Reconnecting at the auth or session layer beyond F3 / F4.**
  F3 handles session refresh; F4 handles TCP-channel shutdown.
  A failure in the auth round-trip itself, or a session
  re-establishment that succeeds at the supervisor level but
  fails downstream, has no broader retry envelope.

**Current resolution.** A mistyped password is a fatal exit (a
defensible choice for a real deployment). A transient auth-side
503 is also fatal (a less defensible choice). UDP errors are
counted only at the channel level; no error budget feeds back
into reconnect.

**Why this might come back.** The first multi-day production
run will exhibit transient auth-side and UDP-side errors that
the current implementation cannot ride through.

**Decision rule.** Pick this up once a multi-day smoke run
exposes the failure mode. Implementation: extend the
`start_with_retry` retryable-error set; introduce a
per-channel UDP error counter that signals
`reconnect_needed.notify_one()` after a threshold; classify
auth errors as retryable / non-retryable based on the response
code (5xx and connection errors retryable; 4xx fatal).

### 20.13 — Mid-ride course transitions and resume reuse (from STEP-12.16)

**Where it came from.** STEP-12.16 §7 declared mid-ride course
transitions out of scope for the smoke-test-resilience plan.
The resume code path was implemented for "athlete enters a
game while the daemon is suspended" only.

**Current resolution.** When the watched athlete enters a game
the first time, the daemon transitions out of suspended state,
brings UDP up, and emits `relay.runtime.resumed`. When the
watched athlete moves between courses mid-ride (for example,
crossing a portal into a different world), no equivalent
transition fires. UDP packets continue on the channel
established for the original course; if that pool is no longer
optimal, throughput suffers but the session does not break.

**Why this might come back.** A user who frequently uses portals
or world-hop events sees stale UDP-server choice; the multi-UDP
swap (item 20.14) is the natural place to attach this logic
once that feature exists.

**Decision rule.** Implement alongside item 20.14. The reuse
case is "feed the new course id through the same
`resume_udp_tx` channel and have it close the old UDP channel
after a 60-second grace window".

### 20.14 — Completion of partial implementations (from STEP-12.11, STEP-12.14, STEP-12.16)

**Where it came from.** Two placeholders are wired into the
runtime but the underlying behaviour is not actually executed:

- **Sticky TCP server selection across reconnects** (STEP-12.11,
  restated in STEP-12.14 §L4 and STEP-12.16). The pinned IP is
  tracked at `src/daemon/relay.rs:1693` and the supervisor-event
  handler emits `relay.runtime.tcp_server_pinned`, but the
  reconnect path does not actually re-establish on the pinned
  address — it picks `tcp_servers[0]` from whatever set the
  supervisor most recently emitted.
- **Multi-UDP-channel with grace-shutdown swap** (STEP-12.14
  §L6). `recompute_udp_selection` at `src/daemon/relay.rs:519`
  emits `relay.udp.channel.grace_shutdown` and broadcasts
  `GameEvent::PoolSwap`, but the body of the spawned 60-second
  grace task contains a literal `// Placeholder: actual channel
  transfer is implemented in L6` comment. The new channel is
  not actually opened; the old channel is not actually closed.

**Current resolution.** The trace events fire and the symbols
exist, so the contract surface looks complete from outside. The
behavioural contract is not honoured. STEP-12.20 lists these as
"implemented (partial)" rather than missing, on the grounds
that the wiring is real even though the body is not.

**Why this might come back.** A reconnect during a mid-session
TCP shutdown picks an arbitrary server rather than the pinned
one; a multi-UDP swap on a portal entry never actually happens.
Both turn into observable behavioural defects the moment the
respective scenario is exercised.

**Decision rule.** Complete L4 (TCP pinning) the first time a
real reconnect picks a different server than the one pinned;
the failure is silent today but visible in
`relay.tcp.connect.attempt` traces. Complete L6 (UDP swap)
either alongside item 20.13 or when the first portal-entry
trace shows the swap is needed. Both are well-scoped pieces of
work — neither warrants its own STEP, but both should be
elevated out of "parking lot" once a concrete scenario is in
hand.

### 20.15 — State-refresher cadence (acknowledged out of scope; from STEP-12.16)

**Where it came from.** STEP-12.16 §7 explicitly excluded
operator-tunable state-refresher cadence. The 3-second minimum
/ 30-second expanding / 5-minute cap behaviour from STEP-12.14
§L1 is the locked-in choice.

**Current resolution.** The cadence values are constants in
`src/daemon/relay.rs:584-587`. There is no operator override.
This is recorded here for completeness, not because the values
are expected to change.

**Why this might come back.** A deployment scenario where the
3-second minimum poll rate causes detectable load on the Zwift
auth endpoint (in aggregate across many ranchero instances), or
a debugging scenario where the operator wants to force a
slower or faster cadence for reproduction.

**Decision rule.** Do not add the knob until a concrete
scenario justifies it. If the scenario appears, the change is
mechanical: thread two `Duration` fields through
`ResolvedConfig` into the state-refresher.

### 20.16 — Auth-failure response-body diagnostics (from STEP-12.17)

**Where it came from.** STEP-12.17 fixed the missing
`Accept: application/json` header on `get_profile_me`, which had
caused a real-account smoke run to fail with
`relay.auth.profile.failed status=200 variant="BadSchema"`. The
incident exposed two diagnostic shortcomings that turned a single
header omission into a hard-to-investigate failure:

- The `BadSchema` trace records only `status` and
  `variant="BadSchema"`; the response Content-Type — the most
  diagnostic field for "200 but wrong body type" — is not
  captured.
- The `Error::AuthFailedBadSchema` message
  (`crates/zwift-api/src/lib.rs:73-74`) renders as
  `"authentication failed: unexpected response shape: expected
  value at line 1 column 1"` once the serde error is appended.
  The serde "line 1 column 1" string buries the actionable
  signal (which is "the body is not JSON at all").

**Current resolution.** Both diagnostic improvements were noted
during STEP-12.17 but kept out of the in-plan fix on the principle
that the immediate fix (adding the missing header) is the smallest
change that makes the smoke pass. The diagnostics make the *next*
failure of the same class self-diagnosing; the smoke does not need
them today.

**Why this might come back.** Any future failure where a Zwift
endpoint returns a 200 with an unexpected body type — server
rolling out a new content type, an account-flagged response, an
intermediate proxy reformatting bodies — produces the same opaque
`BadSchema` error today. The first time that recurs, this entry
becomes the cheapest path to a self-diagnosing trace and a
self-explanatory error message.

**Decision rule.** Implement when (a) a second `BadSchema`
incident happens on a different endpoint or under different
conditions, and reading `relay.auth.profile.failed` traces is no
longer enough to identify the cause; or (b) the operator-facing
error message at the daemon-exit boundary is rewritten for any
other reason, at which point folding the body prefix in costs
nothing. The change is local: one extra `tracing` field on the
BadSchema branch in `get_profile_me`, and one extra argument to
the `AuthFailedBadSchema` error variant carrying a body-prefix
slice.

### 20.17 — SQLite persistence deferrals (from STEP-16)

**Where it came from.** STEP-16 shipped `zwift-store` with three
SQLite databases (`store.sqlite`, `athletes.sqlite`,
`segments.sqlite`) but explicitly excluded six items from its
"Out of scope" section. None of the six is tracked elsewhere; this
entry is the parking lot for all of them so a future reader can find
them without re-reading STEP-16.

The six items split into three deferred-work items (expected to land
in a later step), one spec-level deferral (FIT export), and two
deliberate non-features (no encryption at rest, no operational
hygiene tooling). They are grouped here because they share a single
subsystem and decision-making context.

**Deferred work, expected in a later step:**

1. **Live `AthleteData` → `athletes.sqlite` persistence.** STEP-16
   built `AthletesDb::upsert`/`touch`/`get` but no caller writes
   ingest data into them. The store is exercised only by its own
   tests. The natural home is the step that joins `zwift-stats`
   ingest to persistence — STEP-16 calls this out as "probably
   STEP 18+" without committing.
2. **Background eviction for the segments cache.**
   `SegmentsDb::evict_expired(now) -> Result<usize>` exists and is
   tested, but no scheduled task calls it. The natural home is
   whichever step first writes leaderboards into the cache
   (segment-leaderboard fetcher, not yet planned). Until that step
   lands, the cache is unused and unbounded growth is not a risk.
3. **Schema introspection in `ranchero status`.** The persistence
   block today is bytes-only. A future enhancement could report
   `user_version`, row counts per table, or the oldest/newest
   `last_seen` in the athletes cache. No step has committed to
   this; it is a low-priority operator-ergonomic item.

**Spec-level deferral:**

4. **FIT export of finished sessions.** Deferred past v1 per the
   spec stub (`stats.mjs:2057` in sauce4zwift's `exportFIT`) and
   CLAUDE.md. Not a STEP-NN item — a v2 concern. Listed here only
   so a reader of STEP-16 can find the trail.

**Deliberate non-features (the "current resolution" is "never,
unless the threat model changes"):**

5. **Encryption at rest for the SQLite files.** Credentials live in
   the OS keyring (STEP-05); the SQLite files contain no secrets
   (athlete profiles, KV settings, cached leaderboards). Adding
   SQLCipher or equivalent would cost a vendored fork of
   `rusqlite`, an operator-managed key, and migration tooling for
   existing on-disk DBs, in exchange for protecting data that is
   not sensitive. Revisit only if the schema grows to hold
   personally-identifying or financially-sensitive data, or a
   deployment context (multi-tenant, regulated industry) requires
   encryption-at-rest as a checkbox.
6. **Backups, vacuum scheduling, integrity checks.** SQLite's
   defaults plus WAL are sufficient for a single-user daemon.
   `VACUUM` reclaims space after large deletes (which do not happen
   in v1: athletes accumulate, segments expire-and-overwrite but
   the row count stays bounded). `PRAGMA integrity_check` is a
   recovery tool, not a steady-state task. Operator backups
   (`cp` while the daemon is stopped, or `.backup` over the SQLite
   CLI while it is running) sit outside ranchero by design — the
   same place log rotation sits today (see 20.7).

**Why this might come back.**

- Items 1 and 2 come back the moment the upstream subsystem
  (`AthleteData` ingest, segment-leaderboard fetcher) is wired in
  and would otherwise duplicate the in-memory state across
  restarts.
- Item 3 comes back when an operator reports that the persistence
  block is too thin to diagnose a real symptom (for example,
  "athletes cache is huge — how many rows is that").
- Item 4 comes back if v2 takes FIT export off the deferred list.
- Items 5 and 6 come back only on a threat-model or
  deployment-context change. Neither is expected.

**Decision rule.**

- Items 1 and 2: pull into the step that introduces the upstream
  subsystem. Do not implement speculatively.
- Item 3: implement on first operator request that the current
  bytes-only line is insufficient. Each new field is a one-line
  addition to `format_persistence_status` plus a corresponding
  `KvStore` / `AthletesDb` / `SegmentsDb` accessor.
- Item 4: tracked by the spec, not by this plan; no action here.
- Items 5 and 6: do not implement. Re-evaluate only on an explicit
  deployment-context or schema change that invalidates the
  reasoning above.

### 20.18 — Web-server feature deferrals (from STEP 17)

**Where it came from.** STEP 17's "Out of scope for STEP 17" section listed
six sauce4zwift web-server features that ranchero deliberately does not build
in the initial web-server step. None pointed to a concrete later step; this
entry is the parking lot for all six so a future reader finds them without
re-reading STEP 17. (The formatter-dependent routes and the v2 deep
resource filter from that same section are *not* here — they have a concrete
home in STEP 18 and are recorded there.)

1. **Per-message WebSocket compression.** Ranchero encodes the full frame on
   every emission; sauce's three-buffer no-re-encode write pattern is also
   skipped. The Rust encoder is fast enough at the expected localhost traffic
   volume. Revisit when a non-localhost deployment scenario makes wire size or
   re-encode cost matter.
2. **Mod web roots and the mod-management surface.** Ranchero has no mod
   loader; the mod-management RPCs and `/mods/<mod-id>/` static mounts wait
   for a step that introduces mods.
3. **Native window manifests (`window-manifests.json`,
   `getWebWindowManifests`).** Ranchero has no native window manager
   equivalent to Electron's `BrowserWindow`; the RPC stays unregistered until
   a native-window concept exists.
4. **Browser-source assets and the patron / EULA pages.** Vendored into
   `pages/` because the tree is copied wholesale, but no route or RPC supports
   them functionally. Revisit if a future step introduces a browser-source
   workflow.
5. **HTTPS certificate provisioning (ACME / Let's Encrypt).** Operators bring
   their own certs today. Automated provisioning is a later step, driven by a
   deployment that needs it.
6. **WebSocket authentication.** Sauce serves the WebSocket with no auth
   (loopback only by default); ranchero matches. This is a deliberate
   match-sauce decision, not pending work — binding to `0.0.0.0` is the
   operator's responsibility. Recorded here for completeness.

**Why this might come back.** Items 1–5 each return with the deployment
scenario named in their text (non-localhost traffic, a mod loader, a native
window manager, a browser-source workflow, automated cert management). Item 6
returns only if ranchero's threat model changes to require authenticated
WebSocket access.

**Decision rule.** Items 1–5: pull into the step that introduces the named
capability; do not implement speculatively. Item 6: do not implement unless
the deployment model stops being loopback-by-default.

### 20.19 — Relay-to-web data-path follow-ups (from STEP 17)

**Where it came from.** The relay-to-web bridge (STEP 17 items 17.36–17.38,
detailed in `STEP-17-relay-web-bridge-design.md`) is functional and tested,
but four gaps were deliberately left open. Three were listed in the design
note's "What is intentionally NOT in scope"; one is the event-subgroup cache
population deferred inside the proto-to-stats section ("out of scope for
17.31"). Grouped here because they share the bridge / proto-to-stats
subsystem.

1. **Reduce `GameEvent::PlayerState` to `{ athlete_id }`.** The variant
   carries eleven scalar fields but only `athlete_id` is read downstream (the
   stats fanout looks the athlete up in the registry; the full proto travels
   on the dedicated `player_states` stream). The surplus scalars are
   vestigial and harmless. Reducing the variant is a mechanical cleanup that
   would re-touch six test files and two relay tests for no functional gain.
2. **World-meta altitude adjustment and lat/lng projection.**
   `route_player_state` and `ProtoView` currently store raw `proto.z / 100`
   as altitude (no `(z - seaLevel + eleOffset) / 100 * physicsSlopeScale`
   adjustment) and return `0.0` for `lat`/`lng`. Both need the world-meta
   tables — a STEP-14-era data file not yet vendored. TODOs mark the spots in
   `src/web/proto_to_stats.rs` and `src/web/proto_view.rs`.

   **STEP 18 dependency (gap G3).** The `_formatState` formatter
   (`src/web/format.rs::format_state`) inherits this gap. STEP 18 leaves the
   following state fields absent because they all need the world-meta
   projection: `state.latlng` (sauce4zwift's `[lat, lng]` pair),
   `state.x`/`state.y` (Web-Mercator projection), `state.roadCompletion`,
   and `state.progress`. There is also a named **deviation**: where
   sauce4zwift emits a single `latlng: [lat, lng]` array, ranchero emits
   separate `lat`/`lng` scalar fields. When the world-meta tables are
   vendored, decide whether to repack `lat`/`lng` into a `latlng` array in
   `format_state` (full parity) or keep the scalars as a documented API
   extension. See `docs/planning/STEP-18-parity-ledger.md` (`_formatState`
   table) and STEP 19's widget-compatibility note.
3. **`self_athlete_id` sourcing in `WebState`.** `run_daemon` cannot yet
   determine the logged-in athlete's own id at boot, so `self_athlete_id` is
   `None` (inline `TODO 17.36-I`). The `self` aliases in the athlete
   endpoints and the `apply_event_state` self-comparison fall back to `0`
   until it is sourced from the monitor/self identity.
4. **Event-subgroup cache population.** `WebState.event_subgroups` exists and
   `apply_event_state` reads it, but no background fetch fills it (out of
   scope for 17.31). Every lookup misses, so `apply_event_state` returns
   `Idle` — matching sauce4zwift's behaviour while its own background fetch is
   pending. A real population task (fetch event subgroups from the Zwift API
   and refresh the cache) is the deferred work.

   **STEP 18 dependency (gap G4, part).** The `_getEventOrRouteInfo` spread
   in both `format_athlete_data_v1` and `format_athlete_v2`
   (`src/web/format.rs`) depends on this cache. Until it is populated, the
   spread fields `eventLeader`, `eventSweeper`, `remaining`,
   `remainingMetric`, `remainingType`, and `remainingEnd` are absent —
   parity-correct, because sauce4zwift omits them too when its own cache
   misses. They become available when this population task lands. See
   `docs/planning/STEP-18-parity-ledger.md`.

**Why this might come back.** Item 1 is pure cleanup — pick it up if the
vestigial fields ever cause confusion. Item 2 returns when a widget needs
true altitude/grade or map position. Item 3 returns as soon as any feature
must distinguish the logged-in rider (the `self` endpoint aliases are already
degraded without it). Item 4 returns when event/sub-group widgets need live
event context rather than always-`Idle`.

**Decision rule.** Item 1: cleanup-only, no trigger required; do it when
touching the variant for another reason. Item 2: implement with the
world-meta table vendoring (a data-file step). Item 3: implement the moment a
feature depends on self-identity — it is the highest-priority of the four.
Item 4: implement alongside the event-subgroup fetcher when event widgets are
built.

### 20.20 — Formatter data-source deferrals (from STEP 18)

**Where it came from.** STEP 18 ported every v1/v2 payload formatter to
field-for-field parity, but several formatter fields read data ranchero does
not yet compute. The formatters emit `null` or omit those fields, which is
parity-correct because sauce4zwift does the same when its own source is
absent (see the gap discussion in `STEP-18-format-payloads.md` and the
field-by-field status in `docs/planning/STEP-18-parity-ledger.md`). Two of
the STEP 18 gaps (G3 state world-coordinates, and the event/route spread
half of G4) already have a home in 20.19 items 2 and 4 and are cross-referenced
there. The remaining STEP 18 data-source gaps have no other home and are
collected here so they are not forgotten.

1. **Athlete-profile read cache — `athlete` field and FTP/TSS (gaps G1, G2).**
   `_formatAthleteData`/`_formatAthleteDataV2` read `this._athletesCache`
   to populate the `athlete` field (name, FTP, weight, privacy) and to
   compute `tss` from FTP. Ranchero's formatters
   (`format_athlete_data_v1`, `format_athlete_v2` in `src/web/format.rs`)
   have no profile cache in `WebState`, so they pass `athlete: null` and
   `ftp: None` — which makes `tss` null everywhere. This is the **read**
   cache the formatters consume, distinct from the **write**-side
   persistence in 20.17 item 1 (`AthleteData` → `athletes.sqlite`). The
   work is to populate an in-memory profile cache in `WebState` (sourced
   from the Zwift API profile fetch and/or `athletes.sqlite`) and have the
   formatters read it. Closing this also closes G2 automatically, since
   `tss` only needs the FTP that the profile carries.
2. **`gameState` (gap G4, part).** `_formatAthleteData`/`_formatAthleteDataV2`
   include `gameState: self ? this._gameState : undefined` — emitted only
   for the logged-in rider, sourced from the game-connection state.
   Ranchero has no game-connection state object yet, so the formatters emit
   `game_state: None` (omitted). Returns when a game-connection state
   producer exists (related to, but separate from, the `gameConnection`
   subscription source stubbed in `src/web/subs/mod.rs`).
3. **`...userDefined` spread (gap G4, part).** Both formatters spread
   `...ad.userDefined` as their last step — arbitrary caller-supplied
   key/value pairs merged into the payload. Ranchero's `AthleteData` has no
   `userDefined` map and no producer for one, so nothing is spread. Returns
   when a feature needs to attach user-defined fields to the athlete payload.

**Why this might come back.** Item 1 returns the moment any widget needs the
athlete's name/FTP or a real TSS — it is the highest-impact of the three,
because two visible fields are null without it. Item 2 returns when a
game-state widget (or the `gameConnection` source) is built. Item 3 returns
only if a feature introduces user-defined athlete fields.

**Decision rule.** Item 1: implement with (or immediately after) the profile
cache wiring into `WebState`; pull 20.17 item 1 alongside it if persistence
is the chosen source. Items 2 and 3: pull into the step that introduces the
named producer; do not implement speculatively.

---

## Items found in the final implementation review (2026-05-23)

The items above (20.1–20.20) are deliberate deferrals: each was a conscious
trade-off made during a step, with the rest of that step finished around it.
The items below (20.21–20.28) are different in character. They came out of a
final cross-check of the whole implementation against sauce4zwift and the
spec, and several are **not optional polish — they block functional parity**.

The shape of the finding is consistent: the supporting libraries
(`zwift-stats` primitives, the `src/web/format.rs` formatters, the codec, the
v2 query-reduction engine) are faithful to sauce4zwift and well-tested in
isolation, which is why STEP 18 and STEP 19 passed. What is missing is the
production *wiring* that drives those libraries end-to-end: the per-tick
recording pipeline, the 1 Hz nearby/groups processor, the RPC handler
registrations, the live event-stream producers, the UDP inbound consumption,
the WorldUpdate decoders, and the profile/event/segment REST fetchers. Each
was confirmed by reading the production code and by a workspace-wide search
showing the relevant library functions are reached only from tests.

These should be triaged into real implementation steps, not left to accrete.
The priority ordering across all eight is given at the end of 20.28.

### 20.21 — Production per-tick stats recording is not wired into `route_player_state`

**Where it came from.** Final review. `zwift-stats` ports sauce4zwift's
`_recordAthleteStats` / `_preprocessState` building blocks faithfully and they
are unit-tested, but the production ingest path — `route_player_state` in
`src/web/proto_to_stats.rs` — invokes only a fraction of them. Reading the
function confirms it calls `registry.upsert`, the five `ingest_*` methods, an
in-place `smooth_grade.update` whose result is discarded, and
`apply_event_state`, and nothing else. A workspace search confirms
`most_recent_state = …`, `record_streams`, `road_history.record`,
`active_segment_check`, `auto_lap_check`, `compute_groups`, `apply_gap`,
`clone_reset`, `resize`, and `WBalAccumulator`/`ZonesAccumulator::configure`
are reached only from tests.

**Current resolution.** The library exists; the wiring does not.
Consequences in published payloads:

| Missing wiring | sauce4zwift source | Published-field impact |
|---|---|---|
| `ad.most_recent_state = state` | `_recordAthleteStats` `stats.mjs:3493` | `state` object always `null` in v1/v2 athlete records; deprives gap/group/segment of current position |
| streams recording (`record_streams`) | `_recordAthleteStats` | `streams/*` (distance/altitude/latlng/wbal) empty |
| road-history recording (`road_history.record`) | `_recordAthleteRoadHistory` `stats.mjs:3103` | gap and segment-completion have no road data |
| work/follow/solo/coffee time + kJ split | `_recordAthleteStats` `stats.mjs:3397-3463` | `workTime`/`followTime`/`soloTime`/`coffeeTime`/`workKj`/`followKj`/`soloKj` always 0 |
| W' and zones `configure` + accumulate | `_updateAthleteDataFromDatabase` `stats.mjs:2863` | `wBal` always `null`; `timeInPowerZones` always empty (also needs an FTP/CP source — see 20.26) |
| slice growth (a `resize`-equivalent) | `stats.mjs:3471-3491` | `lap`/`lastLap`/`laps`/`segments`/`events` bucket stats always empty; `lapCount` works |
| auto-lap detection (`auto_lap_check`) | `_autoLapCheck` `stats.mjs:3092` | no automatic laps |
| active-segment detection (`active_segment_check`) | `_activeSegmentCheck` `stats.mjs:3077` | `segments[]` always empty |
| grade publication | `_preprocessState` | `state.grade` never published (computed then discarded) |
| stale/duplicate-state guard | `_preprocessState` `stats.mjs:3146` (rejects `elapsed<0`/`==0`) | out-of-order/duplicate packets are ingested unconditionally; risk to rolling-window sums |

Two structural notes. `DataSlice::new_from` calls `clone_reset()`, producing
an empty bucket, and `DataCollector` has no `resize` method — so even when a
slice is created it cannot grow. And (verify) `ProtoView` road_time does not
apply sauce's reverse adjustment (`reverse ? 1005000 - roadTime : roadTime -
5000`, `zwift.mjs:321`), so road positions would be wrong for reverse riders
once road history is recorded.

**Why this might come back.** Every overlay widget that reads `state.*`,
per-lap/segment/event numbers, W'bal, zone time, or work/draft kJ shows blank
or zero today. This is the largest single block of missing parity.

**Decision rule.** Not optional for parity. Frame as a dedicated step that
ports `_recordAthleteStats` + `_preprocessState` into `route_player_state` (or
a sibling), drawing the W'/zone configuration from the profile source in
20.26. Until then `zwift-stats` is exercised only by its own tests.

### 20.22 — 1 Hz states-processor: nearby, groups, gap, group identity, event rank

**Where it came from.** Final review. sauce4zwift runs a 1000 ms
`_statesProcessor` loop (`stats.mjs:4182`) that calls `_computeNearby`
(`stats.mjs:4427`) then `_computeGroups` (`stats.mjs:4542`), sets each
athlete's `gap`/`gapDistance`/`isGapEst`/`groupId`, and emits the `nearby` and
`groups` events (v1 and v2). ranchero has no equivalent loop — the only
periodic web-layer task is `gc_tick_loop` in `src/web/state.rs:95`.
`compute_groups` (`crates/zwift-stats/src/groups.rs`) and `apply_gap`
(`crates/zwift-stats/src/gap.rs`) are never called outside tests.

**Current resolution.** Three distinct gaps:

1. **No periodic processor.** `gap`/`gapDistance`/`isGapEst` are always
   `None`; `group_id` is always `None`. The HTTP `/nearby/*` and `/groups/*`
   routes compute on-demand from the registry, so they return *something*, but
   nearby is unsorted (HashMap order, `src/web/http/mod.rs:247`) with no gap
   filtering, and groups group by `group_id` which is always `None`, so groups
   always come back empty (`src/web/http/mod.rs:297`).
2. **No `nearby`/`groups` event source, plus a latent wrong-data bug.** Over
   WebSocket there is no producer for `nearby`/`groups`/`nearby/v2`/`groups/v2`.
   Worse, `event_matches_athlete` (`src/web/subs/mod.rs`) returns `true` for
   these non-athlete event names, so a client that subscribes to `nearby`
   currently receives a stream of single-athlete v1 payloads (one per inbound
   `PlayerState`) instead of the expected sorted array. `emit_v2` formats one
   athlete, not an array.
3. **Incremental gap estimation not ported.** `_computeNearby` splits riders
   into ahead/behind, sorts by `gapDistance`, and walks adjacent riders to
   infer each missing gap (`refSpeedForEst`, `incRP` chaining). ranchero's
   `apply_gap` implements only the simple per-athlete case (direct road
   comparison, else a single speed-EMA fallback).

Related: `ServerToClient.eventPositions` / `EventSubgroupPlacements`
(`stats.mjs:2530-2551`; proto field `ev_subgroup_ps = 23`) is never processed,
so `eventPosition`/`eventParticipants` are always absent even though the
formatters read them.

**Why this might come back.** `nearby` and `groups` drive the most-used
overlay widgets; both are blank/empty today, and the WS `nearby` subscription
delivers wrong-shaped data.

**Decision rule.** Not optional for parity. Add a 1 Hz tick task (sibling to
`gc_tick_loop`) that runs nearby + groups, plus `nearby`/`groups` event
sources in `src/web/subs/`; fix `event_matches_athlete` so the array streams
are not mis-delivered as single athletes. Depends on 20.21 (needs
`most_recent_state` and road history).

### 20.23 — RPC handler surface: only `getVersion` is registered

**Where it came from.** Final review. The spec (§6.1, line 125) notes
sauce4zwift registers "~50 RPC handlers". ranchero's `RpcRegistry::new`
(`src/web/rpc.rs:17`) registers exactly one in-scope handler, `getVersion`.
The RPC plumbing itself — HTTP `/api/rpc/v1` and `/v2`, the WebSocket `rpc`
method, dispatch, argument coercion, base64url decoding — is present and
correct; there is simply nothing registered behind it.

**Current resolution.** Roughly 50 core in-scope handlers are missing (≈75
once borderline ones are included). Any widget calling an RPC gets
`unknown rpc handler`. Grouped by area (excluding window/mod/hotkey/updater/
Electron-shell/companion handlers, which are out of scope per spec §6):

- **Athlete data / control:** `getAthlete`, `getAthletes`, `updateAthlete`,
  `getAthleteData`, `getAthletesData`, `updateAthleteData`, `getAthleteLaps`,
  `getAthleteSegments`, `getAthleteEvents`, `getAthleteStreams`,
  `getPlayerState`, `startLap`, `resetStats`, `getPowerZones`,
  `getPowerProfile`.
- **Nearby / groups (RPC twins of the routes):** `getNearbyData`,
  `getGroupsData`.
- **Social / following:** `getFollowingAthletes`, `getFollowerAthletes`,
  `getMarkedAthletes`, `searchAthletes`, `setFollowing`, `setNotFollowing`,
  `giveRideon`, `toggleMarkedAthlete`, `removeFollower` (the write actions are
  borderline — they write to Zwift, beyond the read-only live-data core).
- **Events:** `getCachedEvent(s)`, `getEvent`, `getEventSubgroup`,
  `getEventSubgroupEntrants`, `getEventSubgroupResults`, `addEventSubgroupSignup`,
  `deleteEventSignup`, `loadOlderEvents`, `loadNewerEvents` (signup actions
  borderline). Ties to 20.19 item 4 and 20.26.
- **Segments / chat / game state:** `getSegmentResults` (ties to 20.26),
  `getChatHistory` (ties to 20.24), `getGameState` (ties to 20.20 item 2).
- **World/route/segment geometry (`Env`):** `getWorldMetas`, `getCourseId`,
  `getRoad`, `getCourseRoads`, `getRoute`, `getCourseRoutes`, `getSegment`,
  `getCourseSegments`, `getRoadSegments` (ties to 20.27 route/world-meta
  tables).
- **App / settings / connection:** `getSetting`, `setSetting` (emits
  `setting-change` on the `app` source — ties to 20.24), `getDebugInfo`,
  `getWebServerURL`, `getZwiftLoginInfo`, `getZwiftConnectionInfo`,
  `reconnectZwift`, `zwiftLogout`, `resetStorageState`, `resetAthletesDB`.
- **Borderline / lower value:** workout handlers (`getWorkouts`, `getWorkout`,
  `getWorkoutCollection(s)`, `getWorkoutSchedule`), file-replay handlers
  (`fileReplayLoad`/`Play`/`Stop`/… — ranchero has a CLI replay path instead),
  `getIRLMapTile`, `putState`, `getQueue`, deprecated `getAthleteStats`/
  `updateAthleteStats`, `exportFIT` (FIT is spec-deferred, see 20.17 item 4).

**Why this might come back.** Browser widgets mix WebSocket subscriptions with
RPC calls; the RPC half is almost entirely unavailable.

**Decision rule.** Not optional for parity, but stage it: implement the
read-only athlete/nearby/groups/event/segment/geometry getters first (these
are what widgets call most), then settings, then the write actions
(`setFollowing`, `giveRideon`, …) once a decision is made on whether ranchero
performs write-back to Zwift at all. Many getters depend on 20.21/20.22 (data
to return), 20.26 (REST fetchers), and 20.27 (geometry tables).

### 20.24 — Live event-stream producers: chat, rideon, game-state, watching-athlete-change

**Where it came from.** Final review. sauce4zwift's `stats` emitter produces
`rideon` (`stats.mjs:2591`), `chat` (`stats.mjs:2650`), `game-state`
(`stats.mjs:1250`), and `watching-athlete-change` (`stats.mjs:2659`) in
addition to the per-athlete streams. ranchero produces only the per-athlete
streams (`athlete/{id}`, `athlete/watching`, `athlete/self`, and their v2
forms, via `bridge_player_state_event` → `GameEvent::PlayerState`).

**Current resolution.**

- **`chat` / `rideon`.** Inbound `WorldUpdate`s are iterated in the recv loop
  (`src/daemon/relay.rs:3354-3372`) only to advance `last_world_update_ts`;
  the payloads (RideOn, SocialAction/chat) are never decoded. There is no
  `GameEvent` variant for them (the enum has only `PlayerState`, `Latency`,
  `StateChange`, `PoolSwap`) and no subs handling. This shares its root cause
  with the relay-side WorldUpdate decoding gap in 20.25.
- **`game-state` / `watching-athlete-change`.** No producer. `watching_id` is
  set once at boot (`src/daemon/runtime.rs:305`) and never changes, so no
  watched-athlete-change event ever fires; there is no game-state object. This
  is adjacent to 20.20 item 2 (the `gameState` *formatter field* and the
  stubbed `gameConnection` subscription source) but distinct: those are the
  field and the source registration, not these two emitter streams.
- **Subscription sources.** `create_delegation` (`src/web/subs/mod.rs`)
  recognises only `source == "stats"` (real) and `source == "gameConnection"`
  (parks forever). A widget subscribing to the `app` source for
  `setting-change` (sauce `app.mjs:142`) gets `unknown source`; this depends
  on `setSetting` from 20.23.

**Why this might come back.** Chat-overlay, ride-on notification, and
game-state widgets receive nothing; widgets that re-render on a
watched-athlete switch never update.

**Decision rule.** Implement the WorldUpdate decoders and new `GameEvent`
variants alongside 20.25 (same relay-side decode), then add the corresponding
subs producers. `game-state`/`watching-athlete-change` come with the
watched-athlete-following work in 20.25 and the game-state producer in 20.20
item 2.

### 20.25 — Relay live-data path completeness

**Where it came from.** Final review of `src/daemon/relay.rs` against
`zwift.mjs`. Distinct from the relay items already parked (20.11–20.16): these
concern whether the live telemetry path actually functions in production.

**Current resolution.**

1. **Inbound UDP `ServerToClient` is decoded but discarded.** sauce processes
   UDP inbound identically to TCP (`zwift.mjs:1860` `inPacket` handler); the
   per-rider live stream arrives primarily over UDP at 10+ Hz (spec
   §4.10/§4.11). In ranchero the UDP channel exists only as a heartbeat sink:
   the recv-loop UDP arm is a no-op (`relay.rs:3448`,
   `ChannelEvent::Inbound(_stc)`), and the only sender into the UDP event
   channel in production is the test-only `inject_udp_event`. All telemetry
   reaching the web bridge comes from the TCP inbound branch plus the 3 s
   state-refresher poll. **High impact.**
2. **TCP reconnect does not re-establish UDP.** `connection_manager`
   (`relay.rs:2851`) reconnects TCP and re-sends the hello but explicitly
   discards `watched_id`/`game_events_tx` (`relay.rs:3056-3061`) and never
   opens a new UDP channel or heartbeat; `resume_udp` is single-shot. After
   any TCP drop the daemon runs on TCP only for the rest of the session. sauce
   rebuilds UDP on every reconnect (`_schedConnectRetry`, `zwift.mjs:1869`).
3. **Watched-athlete position is never updated from the stream.** sauce's
   `_updateWatchingState` (`zwift.mjs:2260`) feeds the rider's live
   `(x, y, courseId, portal)` into `findBestUDPServer` on every state.
   ranchero's `observe_watched_player_state` and `switch_watched_athlete`
   (`relay.rs:2512`, `:2530`) are `#[cfg(test)]`; the initial
   `WatchedAthleteState` seeds position `(0,0)` / course 0 even though startup
   already polled the real world. So `recompute_udp_selection` always
   evaluates against `(0,0)`. `find_best_udp_server` is real code fed stale
   zeros — the "UDP server follows the rider" mechanism is inert. This is the
   upstream cause that makes 20.13/20.14 moot until fixed.
4. **WorldUpdate payloads are never decoded or dispatched.** sauce decodes
   every `WorldUpdate` (`zwift.mjs:2164-2187`): payloadType < 100 by nested
   protobuf name (RideOn, SocialAction, PlayerLeftWorld, PlayerRegisteredFor­
   Event, NotableMoment, …), ≥ 100 via `binaryWorldUpdateDecoders`
   (SegmentResult = 105, etc.). ranchero reads only the timestamp. No decoders,
   no `GameEvent` variants. Source for the `chat`/`rideon` streams (20.24) and
   live `SegmentResult` (spec §4.12, §8).
5. **Heartbeat omits portal/roadId/eventSubgroup.** `broadcastPlayerState`
   (`zwift.mjs:1942-1957`) forwards the watched athlete's `portal`, `_flags2`
   (roadId), and `eventSubgroupId`. `HeartbeatScheduler::next_state`
   (`relay.rs:797`) sends only id/just-watching/watching-id/world/world_time,
   with `course_id` fixed at construction. Distinct from 20.11 (that is
   receive-side portal-pool selection; this is send-side content).
6. **No `multipleLogins` detection.** sauce warns when `pb.multipleLogins` is
   set (the monitor account logged in elsewhere, `zwift.mjs:2144`). No
   reference anywhere in ranchero. Diagnostic only, but it is the signal that
   another client has displaced this session.
7. **State-refresher only polls the watched athlete.** sauce also polls self
   when self ≠ watching (`_refreshStates`, `zwift.mjs:1998`), and suppresses
   logging on HTTP 429. ranchero issues one `get_player_state(watched_id)` and
   treats all errors alike. Low impact under the single-athlete model; matters
   once item 3 lands.

Lower-confidence observations to weigh, not yet assert: `find_best_udp_server`
falls through to nearest-Euclidean when `use_first_in_bounds` matches nothing,
where sauce returns "no swap"; and sauce drops player states for
`activePowerUp === 'NINJA'` (`zwift.mjs:2194`), which ranchero does not — a
deliberate decision is warranted on the NINJA privacy drop.

**Why this might come back.** Items 1–4 mean the UDP path is effectively
non-functional for live telemetry, server-following, post-reconnect recovery,
and world events. The daemon's reason for existing is the live stream.

**Decision rule.** Items 1, 2, 4 are not optional for parity — pull into a
relay-completion step. Item 3 is the prerequisite for 20.13/20.14 and should
land with them. Items 5–7 are smaller and can ride along.

### 20.26 — REST fetchers for live data

**Where it came from.** Final review of `crates/zwift-api/src/lib.rs` against
sauce4zwift's `ZwiftAPI`. ranchero implements OAuth (login/refresh, with 50%
preemptive refresh and 401 inline retry — verified at parity), `get_profile_me`,
`get_player_state`, `logout`, `leave`. The live-data fetchers below are absent.
These are the *producers* that several already-parked caches assume exist.

**Current resolution.**

| Missing method | sauce4zwift | Consumer / why it matters |
|---|---|---|
| `getProfiles` (batch, protobuf `/api/profiles`) | `zwift.mjs:559`, driven on every state via `_maybeUpdateAthleteFromServer` `stats.mjs:3080` | The producer beneath 20.20 item 1 (read cache → `athlete` field, name/FTP), 20.17 item 1 (write to `athletes.sqlite`), and the W'/zone configure in 20.21. Without it `athlete:null` and `tss:null` permanently. **Highest impact.** |
| `getEvent` (protobuf `/api/events/{id}`) | `zwift.mjs:808`, via `getEventSubgroup` `stats.mjs:1332` | The producer beneath 20.19 item 4 (event-subgroup cache); without it `eventLeader`/`eventSweeper`/`remaining*` stay absent and event detection (20.27) has no metadata. |
| `getSegmentResults` (`/api/segment-results`) + `getLiveSegmentLeaders` + `getLiveSegmentLeaderboard` | `zwift.mjs:633-645` | The only writers `segments.sqlite` was built for. 20.17 item 2 names only the evictor. Note sauce caches leaderboards in memory (2 s TTL), so ranchero's `segments.sqlite` is a ranchero-original design, not a sauce parity requirement. |
| `getProfile` (single, `/api/profiles/{id}`) | `zwift.mjs:541` | Backs the on-demand `getAthlete` RPC (20.23). |
| `getActivities` (`/api/profiles/{id}/activities`) | `zwift.mjs:599` | Backs activity-list RPCs. Lower priority. |
| `getGameInfo` (`/api/game_info`) | `zwift.mjs:681` | World/segment metadata sync; relates to the world-meta vendoring in 20.27 / 20.19 item 2. |

**Why this might come back.** `getProfiles` and `getEvent` are blocking
dependencies for whole clusters of already-parked work (20.17/20.19/20.20/20.21).
Those items quietly assume a fetcher that does not exist.

**Decision rule.** Implement `getProfiles` first — it unblocks the athlete
profile cache, FTP/TSS, and W'/zone configuration in one move. `getEvent` next
(events). Segment-leaderboard fetchers only if segment leaderboards are kept in
scope; otherwise reconsider whether `segments.sqlite` should exist at all (see
20.28). `getProfile`/`getActivities`/`getGameInfo` are on-demand and lower
priority.

### 20.27 — Proto fields and static-table vendoring

**Where it came from.** Final review. Several computations are structurally
dead because the data they read was never vendored.

**Current resolution.**

1. **`eventSubgroupId` / `eventDistance` proto fields missing.**
   `apply_event_state` *is* called in production (`proto_to_stats.rs:104`), but
   `ProtoView::event_subgroup_id()` is hardcoded to `0` and `event_distance()`
   to `0.0` because the vendored `udp-node-msgs.proto` `PlayerState` has no
   such field (sauce's proto3 fork does). So `apply_event_state` always sees
   `0` and returns `Idle`; events are never detected from telemetry, and
   event end-by-distance / event privacy flags never fire. Distinct from
   20.19 item 4, which covers the event-subgroup metadata *cache*, not the
   missing *proto fields* that feed it. Needs a decision on extending the
   vendored proto2 schema.
2. **`EventSubgroupPlacements` not processed.** Proto field `ev_subgroup_ps =
   23` exists but is unused; `eventPosition`/`eventParticipants` never written
   (see also 20.22).
3. **Route tables / `zwift-routes` crate absent.** Spec §7.2 lists
   `zwift-routes` ("on demand") and §7.8 makes segment/route detection depend
   on `shared/routes.mjs` + `shared/curves.mjs`. The crate does not exist in
   `crates/`. Without it, `_computeRouteDistance` (`stats.mjs:3197`) and the
   route branch of `_getEventOrRouteInfo` (`stats.mjs:4293`) cannot be ported:
   `routeDistance`, route %, and `remaining`/`remainingMetric`/`remainingType`/
   `remainingEnd` for routes stay absent (the formatters hardcode them `None`
   with a "requires route/event metadata" comment). The event half of
   `_getEventOrRouteInfo` is referenced in 20.19 item 4; the route half is new
   here.
4. **World-meta tables.** Needed for altitude adjustment, lat/lng projection,
   and `state.x`/`y`/`roadCompletion`/`progress` — already parked in 20.19
   item 2; cross-referenced here because `getGameInfo` (20.26) and
   `getWorldMetas` (20.23) are the same data family.

**Why this might come back.** Items 1–2 block all event detection; item 3
blocks route progress and is a prerequisite for faithful segment detection.

**Decision rule.** Item 1 (proto fields) is cheap once the schema decision is
made and unblocks the whole event chain — do it early. Item 3 (route tables)
is a data-vendoring step on the scale of the spec's `zwift-routes` crate;
schedule it deliberately. Item 2 rides along with 20.22.

### 20.28 — Persistence schema and live usage

**Where it came from.** Final review of `crates/zwift-store` and
`src/daemon/stores.rs` against sauce's `db.mjs`/`storage.mjs` and the DB
definitions in `stats.mjs`. Beyond the deferrals already in 20.17:

**Current resolution.**

1. **`athletes` table schema cannot hold the full athlete object.** sauce
   stores each athlete as a JSON blob (`athletes(id INTEGER PK, data TEXT)`)
   and queries it with `json_each(data, '$.marked')` to load marked athletes
   (`stats.mjs:2440-2447`). ranchero uses fixed columns (`fname, lname, ftp,
   weight, badges, last_seen`), which cannot represent `marked`, `following`,
   `gender`, `type`, `avatar`, privacy flags, power-source, etc. The
   marked-athletes user feature has no column at all. 20.17 item 1 assumes the
   existing schema is adequate; for sauce parity it is not.
2. **`event_subgroups.sqlite` is missing entirely.** sauce persists
   subgroup→event mappings (`stats.mjs:3582-3597`) so event context survives
   restarts. ranchero has no such DB; `WebState.event_subgroups` is in-memory
   only (20.19 item 4 covers populating that in-memory cache, not persisting
   it). A fourth sauce DB with no ranchero counterpart.
3. **The three store DBs are opened but never read or written in production.**
   `Stores::open` runs at daemon start, but `run_daemon` binds the result as
   `_stores` and nothing in the runtime/relay/web layers calls
   `upsert`/`touch`/`put`/`get`/`evict_expired`. In practice the SQLite layer
   is exercised only by its own crate tests. This is the broad version of the
   specific writers noted in 20.17 items 1–2.

**Why this might come back.** Item 1 blocks marked/followed-athlete features;
item 3 means restarts lose all cached state (settings, athlete profiles).

**Decision rule.** Decide item 1's schema before wiring 20.17 item 1's writer
(a JSON-blob `data` column matches sauce and avoids re-migration). Item 2 only
if event persistence across restarts is wanted. Item 3 resolves naturally as
20.17/20.20/20.26 wire real readers and writers; until then, note in
`ranchero status` (or a comment) that persistence is structurally present but
inert.

---

### Priority ordering across 20.21–20.28

A suggested order, by how much each unblocks:

1. **20.26 `getProfiles`** — unblocks athlete profile cache, FTP/TSS, and
   W'/zone configuration in one move.
2. **20.21 per-tick recording pipeline** — turns most published fields from
   blank/zero into real values (depends on 1).
3. **20.25 items 1, 2, 4 (UDP inbound, reconnect UDP, WorldUpdate decode)** —
   makes the live telemetry path actually function.
4. **20.22 1 Hz nearby/groups processor** — the most-used overlay widgets
   (depends on 2).
5. **20.27 item 1 (event proto fields)** + **20.26 `getEvent`** — event
   detection chain.
6. **20.23 RPC handlers** (read-only getters first) — the RPC half of the
   widget API (depends on 2/4 for data).
7. **20.24 chat/rideon/game-state streams** — alongside 20.25 item 4.
8. **20.27 item 3 (route tables)** and **20.28 (persistence schema)** —
   larger, more independent pieces; schedule deliberately.

The honest summary: items 1–4 above are the difference between a daemon that
serves a faithful-but-mostly-empty payload shape (today) and one that serves
live data comparable to sauce4zwift. STEP 18/19 verified the shapes and the
isolated math; they did not — and were not designed to — verify the
end-to-end production data path, which is what 20.21–20.28 record.

---

## How to use this file

When a step encounters a decision that is acceptable in this version
but worth revisiting later:

- Add a numbered subsection under **Open items** (`20.N — short
  title`).
- State where it came from, the current resolution, why it might come
  back, and a decision rule for when to revisit. Keep it concise:
  parking-lot entries should be readable within a minute.
- When an item is resolved or pulled into a step, move it to a
  **Resolved** section at the bottom, or delete it if the resolution
  was to retain the current approach.
