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
