# Step 12.5 — Still not doing the job as specified

**Status:** review (2026-04-29).

This document records the gap between what was reported as complete
under STEP-12 and what the system actually does at runtime. The
gap surfaced on 2026-04-29 with the question of whether the
workflow

```
ranchero start --capture <path>
ranchero follow <path>
# Ctrl-C
ranchero stop
```

would log into Zwift, write the live stream to disk, print it to
the screen, and shut down cleanly. The honest answer is no:
nothing is written to the capture file because the daemon never
opens a session or a TCP channel, and `ranchero follow` therefore
tails a file that does not exist.

The numerical prefix `12.5` is a separate-file label and is not
related to the internal sub-step `12.5` inside
`STEP-12-game-monitor.md` (idle suspension, watched-athlete
switching, `GameEvent` emission). The two share digits by
coincidence; their content is unrelated.

## Why this happened

I built STEP-12's test surface against `RelayRuntime::start_with_deps`
with stub `AuthLogin`, `SessionLogin`, and `TcpTransportFactory`
implementations supplied by the tests. That covered every test
named in the plan and produced the 36 / 36 green count I
reported. It did not cover the production code path: no default
implementations of the dependency-injection traits exist, the
production entry point `RelayRuntime::start` deliberately panics
with `unimplemented!()`, and the daemon's run loop never
constructs the orchestrator at all.

The plan documents this as "the live-validation phase" of
sub-step 12.1, and that note is technically present in the
written plan. The plain-language workflow expected to work —
start the daemon, watch the stream — was implied throughout
the discussion that produced STEP-12. Reporting
"STEP-12 complete" without explicitly flagging that the
end-to-end workflow still does not function was misleading.

## Deficiencies

### 1. `cli::dispatch` does not forward `--capture` to the daemon

In `src/cli.rs`, the `Command::Start` arm of `dispatch` reads:

```rust
Command::Start => {
    let log_opts = crate::logging::LogOpts {
        verbose: cli.global.verbose,
        debug: cli.global.debug,
    };
    Ok(daemon::start(&resolved, cli.global.foreground, log_opts)?)
}
```

`cli.global.capture` is parsed by clap and is available in scope,
but it is never passed onward. Removing the STEP-11.6 Fix-D
guard (done) was only half of the rewire; the value still has
to be propagated.

### 2. `daemon::start` and `runtime::start` signatures do not accept the capture path

In `src/daemon/mod.rs`:

```rust
pub fn start(
    cfg: &ResolvedConfig,
    foreground: bool,
    log_opts: LogOpts,
) -> Result<ExitCode, DaemonError> {
    runtime::start(cfg, foreground, log_opts)
}
```

The `Option<PathBuf>` capture-path parameter that the STEP-12.1
plan specifies is missing. The signatures of both
`daemon::start` and `runtime::start` need to be extended.

### 3. `runtime::run_daemon` does not construct a `RelayRuntime`

In `src/daemon/runtime.rs`, `run_daemon` is still the STEP-03
placeholder event loop. It binds a Unix domain socket for the
control protocol, waits for `Status` and `Shutdown` requests,
and exits on `SIGTERM`, `Ctrl-C`, or a UDS shutdown. It does not
call `RelayRuntime::start` and therefore does not perform any
auth, relay-session, or TCP-channel work.

The plan's required structure is documented inside
`STEP-12-game-monitor.md` (sub-step 12.1, "CLI and daemon
changes"): the orchestrator is constructed before the loop, its
`shutdown()` is invoked on any shutdown branch, and `join()` is
awaited before the function returns.

### 4. `RelayRuntime::start` panics with `unimplemented!()`

In `src/daemon/relay.rs`:

```rust
pub async fn start(
    cfg: &ResolvedConfig,
    capture_path: Option<PathBuf>,
) -> Result<Self, RelayRuntimeError> {
    // … credential validation …
    let _ = capture_path;
    unimplemented!(
        "STEP-12.1: default-DI wiring is the responsibility of \
         the live-validation phase; tests use `start_with_deps`",
    )
}
```

Tests substitute their own dependency-injection types via
`start_with_deps` and `start_with_deps_and_writer`. A production
build, which has no test stubs in scope, has no working entry
point.

### 5. No default `AuthLogin`, `SessionLogin`, or `TcpTransportFactory` implementations exist

The three traits exist as public surface and the tests implement
them with stubs. There are no default implementations that
delegate to the real network types:

- `zwift_api::ZwiftAuth::new(Config::default())` followed by
  `auth.login(email, password)` is the production auth path; no
  type wraps it for the trait.
- `zwift_relay::login(&auth, &config)` (or the
  `RelaySessionSupervisor`) is the production session path; no
  type wraps it for the trait.
- `zwift_relay::TokioTcpTransport::connect(addr, timeout)` is the
  production TCP transport; no type wraps it for the trait.

Without these, `RelayRuntime::start` cannot construct the deps
it needs to call `start_with_deps`.

## Required remediation

Each item below corresponds to one of the deficiencies above.
The estimated size is the production code only; tests for the
production path (live validation) are separate and described in
the parent STEP-12 plan.

### A. Default dependency-injection implementations

In `src/daemon/relay.rs`, add three small types:

```rust
pub struct DefaultAuthLogin {
    auth: Arc<zwift_api::ZwiftAuth>,
}
impl AuthLogin for DefaultAuthLogin {
    async fn login(&self, email: &str, password: &str) -> Result<(), zwift_api::Error> {
        self.auth.login(email, password).await
    }
}

pub struct DefaultSessionLogin {
    auth: Arc<zwift_api::ZwiftAuth>,
    config: zwift_relay::RelaySessionConfig,
}
impl SessionLogin for DefaultSessionLogin {
    async fn login(&self) -> Result<zwift_relay::RelaySession, zwift_relay::SessionError> {
        zwift_relay::login(&self.auth, &self.config).await
    }
}

pub struct DefaultTcpTransportFactory;
impl TcpTransportFactory for DefaultTcpTransportFactory {
    type Transport = zwift_relay::TokioTcpTransport;
    async fn connect(&self, addr: SocketAddr) -> std::io::Result<Self::Transport> {
        zwift_relay::TokioTcpTransport::connect(addr, std::time::Duration::from_secs(10)).await
    }
}
```

The auth handle is shared between the auth-login and
session-login types via `Arc` so that the relay-session login can
pick up the bearer token deposited by the OAuth login.

Approximate size: 50 lines.

### B. Implement `RelayRuntime::start`

Replace the `unimplemented!()` body with a constructor that
builds the three default DI types and calls `start_with_deps`:

```rust
pub async fn start(
    cfg: &ResolvedConfig,
    capture_path: Option<PathBuf>,
) -> Result<Self, RelayRuntimeError> {
    let auth_config = zwift_api::Config::default();
    let auth = Arc::new(zwift_api::ZwiftAuth::new(auth_config));
    let session_config = zwift_relay::RelaySessionConfig::default();
    Self::start_with_deps(
        cfg,
        capture_path,
        DefaultAuthLogin { auth: auth.clone() },
        DefaultSessionLogin { auth, config: session_config },
        DefaultTcpTransportFactory,
    )
    .await
}
```

Approximate size: 15 lines.

### C. Extend `daemon::start` and `runtime::start` signatures

Both signatures gain a fourth parameter:

```rust
pub fn start(
    cfg: &ResolvedConfig,
    foreground: bool,
    log_opts: LogOpts,
    capture_path: Option<PathBuf>,
) -> Result<ExitCode, DaemonError>;
```

`runtime::start` propagates the path into `run_daemon`.
`daemon::start` becomes a thin wrapper. Approximate size: 5
lines per file.

### D. `runtime::run_daemon` constructs and owns the orchestrator

Inside the existing `select!` loop, before entering the loop,
call `RelayRuntime::start(cfg, capture_path)`. Hold the runtime
across the loop. On any shutdown branch (UDS shutdown, `Ctrl-C`,
`SIGTERM`), call `runtime.shutdown()` and `runtime.join().await`
before returning.

The placeholder shape from STEP-12.1 is roughly:

```rust
let runtime = RelayRuntime::start(cfg, capture_path).await?;
loop {
    tokio::select! {
        biased;
        _ = shutdown_rx.recv() => break,
        _ = tokio::signal::ctrl_c() => break,
        _ = sigterm.recv() => break,
        accept = listener.accept() => { /* existing UDS handling */ }
    }
}
runtime.shutdown();
let _ = runtime.join().await;
```

The orchestrator's start can fail (auth or session error). The
return type of `run_daemon` already accommodates `io::Error`;
`RelayRuntimeError` needs to convert into the daemon's error
type, or `run_daemon` needs to map it to a clear log line and
exit gracefully. Approximate size: 30 lines including error
plumbing.

### E. Forward `cli.global.capture` from dispatch into `daemon::start`

A one-line change inside `cli::dispatch`:

```rust
Ok(daemon::start(
    &resolved,
    cli.global.foreground,
    log_opts,
    cli.global.capture.clone(),
)?)
```

## Acceptance criteria

The workflow described at the top of this document must
complete cleanly against production Zwift:

1. `ranchero configure` (or pre-existing config) holds valid
   credentials.
2. `ranchero start --foreground --capture /tmp/x.cap`:
   - Exits the foreground process only on a shutdown signal,
     not on an `unimplemented!()` panic.
   - Produces `relay.login.ok` and `relay.tcp.established`
     records in the configured log file.
   - Creates `/tmp/x.cap` on disk and writes records to it as
     inbound TCP packets arrive.
3. `ranchero follow /tmp/x.cap` (in a second terminal):
   - Prints the format-version header line.
   - Prints one summary line per record as records arrive.
   - Exits with status 0 on `Ctrl-C`.
4. `ranchero stop` (in a third terminal, or after `Ctrl-C`):
   - Triggers the orchestrator's graceful shutdown sequence:
     `relay.tcp.shutdown`, then `relay.capture.closed`.
   - The capture file is closed cleanly (every accepted record
     is readable on `ranchero replay /tmp/x.cap`).
5. The bounded TCP-only validation window of roughly 30 s per
   the spec §7.12 client-driven liveness model applies, until
   the rest of STEP-12 (sub-step 12.3 onward) lands a real
   `UdpChannel` and the 1 Hz heartbeat. A server-side timeout
   inside that window is acceptable and confirms the gap that
   STEP-12 sub-step 12.3 closes; it does not invalidate this
   acceptance criterion.

## Honest framing

STEP-12 was reported as 36 / 36 tests green. That count is
accurate. The misleading framing was treating the test count as
a proxy for the operator-facing capability. The capability
requires the production wiring described above; the tests do
not exercise it because they substitute stubs at the
dependency-injection layer. Future progress reports on STEP-12
should state the live validation status separately from the
test count.

## Cross-references

- `docs/plans/STEP-12-game-monitor.md` — the parent plan. The
  "Implementation outline" inside sub-step 12.1 already lists
  these production wiring steps as items 1, 2, 5, 6, and 7.
- `docs/plans/STEP-12.1-tcp-end-to-end-smoke.md` — note: this
  file was folded into STEP-12 earlier in the project history;
  if a stale copy is still present on disk it can be removed.
- `docs/plans/STEP-12.2-follow-command.md` — the `ranchero
  follow` command, which is fully implemented and is the only
  half of the workflow at the top of this document that
  actually works today.
- `docs/ARCHITECTURE-AND-RUST-SPEC.md` — §4.4 (`ClientToServer`
  hello fields), §7.12 (client-driven liveness model).

## Addendum (2026-04-29) — testability of `RelayRuntime::start`

The remediation described in §A through §E is necessary but not
sufficient. After §A through §E are applied, the production
entry point `RelayRuntime::start` exists, compiles, and is wired
into the daemon — but it cannot be exercised by an automated
test without making real HTTPS calls to `secure.zwift.com`. That
is a code design problem, not a test-infrastructure problem.
This addendum names the design problem, surveys the existing
code that creates it, and specifies the production-code changes
needed to close it. The remediation here is labelled §F to
continue the §A–§E numbering.

### F.1 Where the gap sits in the source

`RelayRuntime::start` (after §A and §B) constructs its three
dependency-injection types from default values that are baked
into the `zwift_api` and `zwift_relay` crates:

```rust
pub async fn start(
    cfg: &ResolvedConfig,
    capture_path: Option<PathBuf>,
) -> Result<Self, RelayRuntimeError> {
    let auth = Arc::new(zwift_api::ZwiftAuth::new(zwift_api::Config::default()));
    let session_config = zwift_relay::RelaySessionConfig::default();
    Self::start_with_deps(
        cfg,
        capture_path,
        DefaultAuthLogin::new(auth.clone()),
        DefaultSessionLogin::new(auth, session_config),
        DefaultTcpTransportFactory,
    )
    .await
}
```

`zwift_api::Config::default()` resolves to:

```rust
Config {
    auth_base: format!("https://{DEFAULT_AUTH_HOST}"),  // secure.zwift.com
    api_base:  format!("https://{DEFAULT_API_HOST}"),   // us-or-rly101.zwift.com
    source:    DEFAULT_SOURCE.to_string(),
    user_agent: DEFAULT_USER_AGENT.to_string(),
}
```

There is no parameter on `RelayRuntime::start` that lets a
caller supply a different `Config`. Any test that calls
`RelayRuntime::start(&cfg, None)` therefore reaches
`secure.zwift.com` over the network. The same applies to the
relay-session login, because `zwift_relay::login(&auth, &cfg)`
reuses the auth handle's HTTP client and inherits its
`api_base`. Redirecting the auth `Config` redirects both.

### F.2 What is already injectable, and what tests already exist

Analysis of the workspace shows that the lower-level types
already expose URL injection cleanly. The only crate where the
URL is hard-coded into a public function is `ranchero` itself.

- `zwift_api::Config` is a public struct whose `auth_base` and
  `api_base` are plain `String`s. The crate's own tests build a
  `Config` from a `wiremock::MockServer::uri()` and pass it to
  `ZwiftAuth::new`. See `crates/zwift-api/tests/auth.rs`. No
  test in `zwift-api` makes a real network call.

- `zwift_relay::login` and `RelaySessionSupervisor::start` both
  accept a pre-built `ZwiftAuth`, so redirecting the auth's
  `Config` redirects them too. The crate's own tests build a
  mock-server-pointed `ZwiftAuth`, perform an OAuth handshake
  against the mock, and pass that handle into the relay
  session functions. See `crates/zwift-relay/tests/session.rs`.

- `zwift_relay::TcpTransport` is a public trait. The
  `TokioTcpTransport` is one implementation; tests substitute a
  channel-backed `MockTcpTransport`. The orchestrator already
  takes a `TcpTransportFactory`, so this layer is fully
  testable already.

- The `ranchero` crate already has an `Env` abstraction in
  `src/config/mod.rs` that resolves several knobs from CLI →
  env → file with explicit precedence. `RANCHERO_LOG_FILE`,
  `RANCHERO_PIDFILE`, `RANCHERO_SERVER_PORT`, and others use
  this pattern. The Zwift HTTP endpoints are not currently
  surfaced through it.

- `RelayRuntime::start_with_deps` (and the `_and_writer`,
  `_and_events_tx` variants) already accept fully substituted
  DI types. Every test in `tests/relay_runtime.rs` and the
  inline test module of `src/daemon/relay.rs` uses these
  variants with stubs (`StubAuth`, `StubSession`,
  `NoopTcpTransport`) and never touches the network. There is
  no testability gap below `RelayRuntime::start`.

The gap is exactly one method wide: `RelayRuntime::start`
itself. It accepts a `&ResolvedConfig` that carries no Zwift
endpoint information and therefore has no way to forward an
override into the `Config` it builds.

### F.3 Required changes

Each item below changes production code to remove the
hard-coded endpoint. Tests that need to exercise
`RelayRuntime::start` end-to-end can then point the daemon at
a mock server or an unroutable address and observe behaviour
without contacting Zwift.

#### F.3.1 Add a `[zwift]` section to the configuration schema

In `src/config/mod.rs`, extend `ConfigFile` with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZwiftConfig {
    pub auth_base: String,
    pub api_base: String,
}

impl Default for ZwiftConfig {
    fn default() -> Self {
        Self {
            auth_base: format!("https://{}", zwift_api::DEFAULT_AUTH_HOST),
            api_base:  format!("https://{}", zwift_api::DEFAULT_API_HOST),
        }
    }
}
```

Wire `ZwiftConfig` into `ConfigFile` as `pub zwift: ZwiftConfig`
with `#[serde(default)]`. Existing config files without a
`[zwift]` section continue to resolve to the production
defaults.

#### F.3.2 Resolve the endpoints through the existing CLI → env → file pattern

In `src/config/mod.rs::ResolvedConfig::resolve`, add:

```rust
let auth_base = env.get("RANCHERO_ZWIFT_AUTH_BASE")
    .unwrap_or_else(|| file.zwift.auth_base.clone());
let api_base = env.get("RANCHERO_ZWIFT_API_BASE")
    .unwrap_or_else(|| file.zwift.api_base.clone());
```

`ResolvedConfig` gains a `zwift_endpoints: ZwiftEndpoints`
field that holds the resolved values. There is no CLI flag for
these — they are operator configuration, not per-invocation
options.

#### F.3.3 Build the `zwift_api::Config` from `ResolvedConfig`

In `src/daemon/relay.rs::RelayRuntime::start`, replace
`Config::default()` with a constructor that reads from
`cfg.zwift_endpoints`:

```rust
let auth_config = zwift_api::Config {
    auth_base:  cfg.zwift_endpoints.auth_base.clone(),
    api_base:   cfg.zwift_endpoints.api_base.clone(),
    source:     zwift_api::DEFAULT_SOURCE.to_string(),
    user_agent: zwift_api::DEFAULT_USER_AGENT.to_string(),
};
let auth = Arc::new(zwift_api::ZwiftAuth::new(auth_config));
```

`source` and `user_agent` stay at their library defaults; the
two URLs are the only operator-relevant knobs at this stage.

#### F.3.4 Side benefit: operator-facing endpoint override

The change above is not test-only. It also lets an operator
point ranchero at a staging Zwift environment, a
self-hosted relay server, or an offline mirror without
recompiling. That capability is currently absent and is a real
operator gap; §F.3.1–§F.3.3 close it as a side effect of
making `RelayRuntime::start` testable.

#### F.3.5 Adjust the red-state tests

Two adjustments in `tests/full_scope.rs`:

- The library-level tests that call `RelayRuntime::start`
  directly (Group A) become subprocess tests, or they remain
  as library tests but use an unroutable address resolved
  through a hand-built `ResolvedConfig` whose
  `zwift_endpoints.auth_base` is `http://127.0.0.1:1`. The
  test must wait for the connect-refused failure path and
  assert no panic occurred and that the capture writer (if a
  capture path was supplied) was opened then closed cleanly.

- Subprocess tests pass
  `RANCHERO_ZWIFT_AUTH_BASE=http://127.0.0.1:1` and
  `RANCHERO_ZWIFT_API_BASE=http://127.0.0.1:1` via
  `Command::env(...)` to the spawned `ranchero` binary. No
  global process-state mutation is required because each
  subprocess has its own environment block.

#### F.3.6 Update existing `daemon_lifecycle.rs` baseline

The tests in `tests/daemon_lifecycle.rs` spawn the binary with
no credentials configured. After §A–§E, the orchestrator's
credential check returns `MissingEmail` immediately and no
HTTP call is attempted. These tests are unaffected by §F and
should not change. Verify with a single representative run
after §F lands.

### F.4 Order of operations

§F is a prerequisite for safely re-running the green-state
tests for §A through §E. The correct sequence is:

1. Apply §F.3.1–§F.3.4 (configuration schema + resolution +
   `RelayRuntime::start` construction).
2. Apply §F.3.5 (test rewrites against the new injection
   point).
3. Verify §F.3.6 (daemon_lifecycle baseline still green).
4. Re-run the full-scope suite. All assertions in §A through
   §E that were red can now be exercised without contacting
   Zwift.

### F.5 Out of scope for §F

Two larger redesigns are explicitly **not** part of §F. They
are noted here so they are not silently re-introduced as
"obvious next steps":

- Injecting a `reqwest::Client` (or a higher-level HTTP-client
  trait) into `ZwiftAuth`. The crate already exposes
  `ZwiftAuth::with_client` for connection-pool sharing; that
  is sufficient. A trait-based client would let tests
  short-circuit HTTP entirely, but the URL-only injection is
  the path the rest of the workspace already uses, and it is
  enough to make `RelayRuntime::start` testable.

- Surfacing `source` and `user_agent` to operator
  configuration. These are policy values inside `zwift_api`
  and have no operator-relevant effect on testability; they
  stay at the library defaults until a future spec change
  requires otherwise.
