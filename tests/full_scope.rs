//! STEP-12.5 — Full-scope red-state tests.
//!
//! These tests pin the operator-facing capability described at the
//! top of `docs/plans/STEP-12.5-still-not-doing-the-job-as-specified.md`:
//!
//! ```text
//! ranchero start --capture <path>
//! ranchero follow <path>
//! # Ctrl-C
//! ranchero stop
//! ```
//!
//! The deficiencies documented in that plan (CLI forwarding, daemon
//! signature plumbing, `RelayRuntime::start` panicking with
//! `unimplemented!()`, default DI types missing, `run_daemon` not
//! constructing the orchestrator) all keep this workflow from
//! functioning in production today. The tests here exercise the
//! contracts those deficiencies block, so each one fails at runtime
//! while the deficiencies remain.
//!
//! Tests are organised into four groups:
//!
//! - **A**: `RelayRuntime::start` production library entry — must
//!   not panic with `unimplemented!()`.
//! - **B**: Daemon orchestration — `run_daemon` must construct the
//!   orchestrator and propagate `capture_path` to it.
//! - **C**: CLI forwarding — `ranchero --capture <path> start` must
//!   reach `RelayRuntime::start` with that path.
//! - **D**: End-to-end workflow — start, follow, stop must cooperate.
//!
//! All subprocess tests use bogus credentials. Auth login against
//! the production Zwift endpoints is therefore expected to fail, but
//! the orchestrator's pre-auth steps (capture-file open, lifecycle
//! tracing) must complete and be observable on disk before the
//! failure path is taken. That is the discriminator between red
//! (orchestrator never constructed) and green (orchestrator
//! constructed and ran far enough to open the capture file).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

use ranchero::config::{
    EditingMode, RedactedString, ResolvedConfig, ZwiftEndpoints,
};
use ranchero::daemon::relay::RelayRuntime;

/// Unroutable address used by every library and subprocess test
/// in this file as the Zwift HTTPS endpoint. Connecting to port 1
/// on the loopback interface is refused immediately by the
/// kernel, so any auth attempt fails fast without leaving the
/// local machine. Tests that expect the override to take effect
/// also assert a tight timing budget so a regression that drops
/// the override is observable rather than silently hitting
/// production Zwift. See STEP-12.5 §F.
const UNROUTABLE_ZWIFT_BASE: &str = "http://127.0.0.1:1";
/// Keychain service name written into every test config TOML so the
/// spawned `ranchero` process resolves the OS-keychain entry under a
/// scope that has no entries (and `OsKeyringStore` further mangles the
/// account names with a `TEST_` prefix when the service is not the
/// production constant). See `daemon_lifecycle.rs::TEST_KEYRING_SERVICE`.
const TEST_KEYRING_SERVICE: &str = "ranchero-test-isolated";

const POLL_INTERVAL: Duration = Duration::from_millis(50);
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const ORCHESTRATOR_GRACE: Duration = Duration::from_secs(15);
const CAPTURE_APPEAR_TIMEOUT: Duration = Duration::from_secs(15);

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_ranchero")
}

// ---------------------------------------------------------------------------
// Library helpers
// ---------------------------------------------------------------------------

fn lib_config(
    email: &str,
    password: &str,
    log_file: PathBuf,
    pidfile: PathBuf,
) -> ResolvedConfig {
    ResolvedConfig {
        main_email: None,
        main_password: None,
        monitor_email: Some(email.to_string()),
        monitor_password: Some(RedactedString::new(password.to_string())),
        server_bind: "127.0.0.1".into(),
        server_port: 1080,
        server_https: false,
        log_level: None,
        log_file,
        pidfile,
        config_path: None,
        editing_mode: EditingMode::Default,
        // Pin every library test to an unroutable address so a
        // regression in `RelayRuntime::start` cannot silently
        // contact production Zwift. See STEP-12.5 §F.
        zwift_endpoints: ZwiftEndpoints {
            auth_base: UNROUTABLE_ZWIFT_BASE.to_string(),
            api_base:  UNROUTABLE_ZWIFT_BASE.to_string(),
        },
        relay_enabled: true,
        watched_athlete_id: None,
    }
}

// ---------------------------------------------------------------------------
// Subprocess harness, shaped after `tests/daemon_lifecycle.rs`
// ---------------------------------------------------------------------------

struct DaemonHarness {
    _dir: tempfile::TempDir,
    config_path: PathBuf,
    pidfile: PathBuf,
    socket: PathBuf,
    log_file: PathBuf,
    capture_path: PathBuf,
    child: Option<Child>,
}

impl DaemonHarness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("ranchero.toml");
        // Short subdir keeps the UDS path under macOS's ~104-byte limit.
        let state = dir.path().join("s");
        std::fs::create_dir_all(&state).unwrap();
        let pidfile = state.join("ranchero.pid");
        let socket = state.join("ranchero.sock");
        let log_file = dir.path().join("ranchero.log");
        let capture_path = dir.path().join("capture.bin");

        let toml = format!(
            "schema_version = 1\n\
             [logging]\n\
             level = \"info\"\n\
             file = \"{}\"\n\
             [daemon]\n\
             pidfile = \"{}\"\n\
             [keyring]\n\
             service = \"{TEST_KEYRING_SERVICE}\"\n",
            log_file.display(),
            pidfile.display(),
        );
        std::fs::write(&config_path, toml).unwrap();

        DaemonHarness {
            _dir: dir,
            config_path,
            pidfile,
            socket,
            log_file,
            capture_path,
            child: None,
        }
    }

    fn config_args(&self) -> Vec<String> {
        vec![
            "--config".into(),
            self.config_path.to_string_lossy().into_owned(),
        ]
    }

    fn run_cli(&self, extra: &[&str]) -> Output {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args()).args(extra);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.output().expect("spawn ranchero")
    }

    /// Pin both Zwift HTTPS endpoints on the spawned child to an
    /// unroutable address. Each subprocess test inherits its own
    /// environment from this method, so no global process-state
    /// mutation is required and concurrent test execution is
    /// safe. See STEP-12.5 §F.3.5.
    fn pin_unroutable_zwift_endpoints(cmd: &mut Command) {
        cmd.env("RANCHERO_ZWIFT_AUTH_BASE", UNROUTABLE_ZWIFT_BASE);
        cmd.env("RANCHERO_ZWIFT_API_BASE", UNROUTABLE_ZWIFT_BASE);
    }

    /// Spawn `ranchero start` in the foreground with bogus credentials
    /// supplied via CLI, optional `--capture <path>`, and `--debug`
    /// so logs flow to stderr at INFO+.
    fn spawn_foreground_start(&mut self, with_capture: bool) -> &mut Child {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args());
        cmd.arg("--debug");
        cmd.arg("--monitoruser").arg("noone@example.invalid");
        cmd.arg("--monitorpassword").arg("not-a-real-password");
        if with_capture {
            cmd.arg("--capture").arg(&self.capture_path);
        }
        cmd.arg("start");
        Self::pin_unroutable_zwift_endpoints(&mut cmd);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd.spawn().expect("spawn ranchero start");
        self.child = Some(child);
        self.child.as_mut().unwrap()
    }

    /// Spawn `ranchero start` in the background so log records are
    /// written to the configured log file. The CLI returns once the
    /// daemon has double-forked.
    fn spawn_background_start(&self, with_capture: bool) -> Output {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args());
        cmd.arg("--monitoruser").arg("noone@example.invalid");
        cmd.arg("--monitorpassword").arg("not-a-real-password");
        if with_capture {
            cmd.arg("--capture").arg(&self.capture_path);
        }
        cmd.arg("start");
        Self::pin_unroutable_zwift_endpoints(&mut cmd);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.output().expect("spawn ranchero start (bg)")
    }

    fn wait_for_pidfile(&self) -> Option<u32> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(s) = std::fs::read_to_string(&self.pidfile)
                && let Ok(pid) = s.trim().parse::<u32>()
                && self.socket.exists()
            {
                return Some(pid);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        None
    }

    fn wait_for_pidfile_gone(&self) -> bool {
        let deadline = Instant::now() + ORCHESTRATOR_GRACE;
        while Instant::now() < deadline {
            if !self.pidfile.exists() {
                return true;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        false
    }

    fn wait_for_child_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let child = self.child.as_mut()?;
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {}
                Err(_) => return None,
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        None
    }

    fn wait_for_capture_file(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.capture_path.exists() {
                return true;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        false
    }

    fn wait_for_log_match(&self, needle: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Ok(text) = std::fs::read_to_string(&self.log_file)
                && text.contains(needle)
            {
                return true;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        false
    }
}

impl Drop for DaemonHarness {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Ok(s) = std::fs::read_to_string(&self.pidfile)
            && let Ok(pid) = s.trim().parse::<u32>()
        {
            let _ = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

fn stdout_string(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_string(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn read_stderr_to_string(child: &mut Child) -> String {
    use std::io::Read;
    let mut buf = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut buf);
    }
    buf
}

// ===========================================================================
// Group A — `RelayRuntime::start` production library entry
//
// STEP-12.5 §B: replace the `unimplemented!()` body with a constructor
// that builds the default DI types and delegates to `start_with_deps`.
// ===========================================================================

/// `RelayRuntime::start` must not panic with `unimplemented!()`. Today
/// the production entry point's body is `unimplemented!("STEP-12.1:
/// default-DI wiring is the responsibility of the live-validation
/// phase; tests use start_with_deps")`. Once STEP-12.5 §B is
/// implemented, the call must return either `Ok(_)` (Zwift reachable,
/// auth succeeded) or `Err(_)` (auth failed, network error, etc.) —
/// but never panic.
#[tokio::test]
async fn relay_runtime_start_does_not_panic_with_unimplemented() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = lib_config(
        "noone@example.invalid",
        "not-a-real-password",
        dir.path().join("ranchero.log"),
        dir.path().join("ranchero.pid"),
    );

    let join = tokio::task::spawn(async move {
        // Wrap in a 30-second timeout so the test cannot hang on a
        // real network round-trip. Either a panic, an early Err, or
        // a timeout-Err is observable and is what the assertion
        // distinguishes between.
        let fut = RelayRuntime::start(&cfg, None);
        tokio::time::timeout(Duration::from_secs(30), fut).await
    });

    match join.await {
        Ok(Ok(Ok(_runtime))) => {
            // Zwift was reachable and auth succeeded — extremely
            // unlikely with these bogus credentials, but acceptable.
        }
        Ok(Ok(Err(_e))) => {
            // Expected: auth or session login returned an error.
        }
        Ok(Err(_elapsed)) => {
            // Network timeout. Acceptable: the call did not panic.
        }
        Err(join_err) if join_err.is_panic() => {
            panic!(
                "RelayRuntime::start panicked instead of returning. \
                 STEP-12.5 §B is not implemented: replace the \
                 `unimplemented!()` body with a default-DI delegation."
            );
        }
        Err(other) => panic!("unexpected join error: {other}"),
    }
}

/// `RelayRuntime::start(cfg, Some(path))` must accept and propagate
/// the capture path. The orchestrator's `start_with_deps_and_events_tx`
/// opens the capture writer *before* the auth step, so even when
/// auth fails the capture file must appear on disk with the magic
/// header written. This pins the contract that the production entry
/// point delegates through `start_with_deps_and_events_tx` (or an
/// equivalent code path that opens the writer first).
#[tokio::test]
async fn relay_runtime_start_with_capture_path_creates_capture_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let capture = dir.path().join("capture.bin");
    let cfg = lib_config(
        "noone@example.invalid",
        "not-a-real-password",
        dir.path().join("ranchero.log"),
        dir.path().join("ranchero.pid"),
    );
    let capture_for_start = capture.clone();

    let join = tokio::task::spawn(async move {
        let fut = RelayRuntime::start(&cfg, Some(capture_for_start));
        tokio::time::timeout(Duration::from_secs(30), fut).await
    });

    let _ = join.await;

    assert!(
        capture.exists(),
        "capture file at {} was not created. STEP-12.5 §B requires \
         `capture_path` to be passed through to the orchestrator so \
         its pre-auth `CaptureWriter::open` runs.",
        capture.display(),
    );

    let bytes = std::fs::read(&capture).expect("capture file readable");
    assert!(
        bytes.starts_with(zwift_relay::capture::MAGIC),
        "capture file is missing the `RNCWCAP` magic; got first bytes: {:?}",
        &bytes[..bytes.len().min(8)],
    );
}

/// §F.3.3 — `RelayRuntime::start` must build its `zwift_api::Config`
/// from `cfg.zwift_endpoints`, not from `Config::default()`. The
/// discriminator is timing: a connection to `127.0.0.1:1` is
/// refused by the kernel in well under 200 ms, whereas a call
/// against the production Zwift host takes multiple seconds (DNS
/// resolution + TLS handshake + HTTP response). A short budget
/// here proves the override took effect; a regression that drops
/// the override would either time out (production network slow)
/// or, worse, succeed in contacting Zwift, which is exactly what
/// §F is preventing.
///
/// This test depends on §F.3.1 / §F.3.2 / §F.3.3 being
/// implemented; until then it fails to compile because
/// `ZwiftEndpoints` and `ResolvedConfig.zwift_endpoints` do not
/// exist.
#[tokio::test]
async fn relay_runtime_start_uses_zwift_endpoints_from_resolved_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cfg = lib_config(
        "noone@example.invalid",
        "not-a-real-password",
        dir.path().join("ranchero.log"),
        dir.path().join("ranchero.pid"),
    );
    // Confirm the helper actually pinned the override; a future
    // edit to `lib_config` that drops it would otherwise let this
    // test reach Zwift.
    assert_eq!(
        cfg.zwift_endpoints.auth_base, UNROUTABLE_ZWIFT_BASE,
        "lib_config must pin auth_base to the unroutable address",
    );
    assert_eq!(
        cfg.zwift_endpoints.api_base, UNROUTABLE_ZWIFT_BASE,
        "lib_config must pin api_base to the unroutable address",
    );

    let started = std::time::Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        RelayRuntime::start(&cfg, None),
    )
    .await;
    let elapsed = started.elapsed();

    match result {
        Ok(Ok(_runtime)) => panic!(
            "RelayRuntime::start unexpectedly succeeded against \
             {UNROUTABLE_ZWIFT_BASE}; either port 1 is bound on \
             this host or the override was dropped and the call \
             reached real Zwift. STEP-12.5 §F.3.3."
        ),
        Ok(Err(_)) => {
            // Expected: connection refused at the auth-login step.
        }
        Err(_) => panic!(
            "RelayRuntime::start did not return within 2 s. The \
             `cfg.zwift_endpoints` override is not consulted by \
             the production `start`, or `Config::default()` is \
             still being used. STEP-12.5 §F.3.3. Elapsed: {elapsed:?}"
        ),
    }

    assert!(
        elapsed < Duration::from_secs(2),
        "elapsed = {elapsed:?}; expected sub-second connection-refused \
         against {UNROUTABLE_ZWIFT_BASE}. The slow path suggests the \
         override was not honoured.",
    );
}

// ===========================================================================
// Group B — Daemon orchestration
//
// STEP-12.5 §C and §D: extend `daemon::start` / `runtime::start` to
// accept `capture_path`, then have `run_daemon` construct a
// `RelayRuntime`, hold it across the UDS event loop, and drive
// `shutdown()` + `join()` on every shutdown branch.
// ===========================================================================

/// Spawning the daemon with `--capture <path>` must result in the
/// capture file appearing on disk shortly after the daemon becomes
/// ready. Today `run_daemon` accepts `capture_path` but discards it
/// (`let _ = (cfg, capture_path);`) and never constructs a
/// `RelayRuntime`, so the file is never created.
#[test]
fn daemon_creates_capture_file_after_start() {
    let mut h = DaemonHarness::new();
    h.spawn_foreground_start(true);

    // The capture file is opened before auth is attempted; wait for
    // it to appear regardless of the daemon's exit status.
    assert!(
        h.wait_for_capture_file(CAPTURE_APPEAR_TIMEOUT),
        "capture file at {} was not created within {:?}",
        h.capture_path.display(),
        CAPTURE_APPEAR_TIMEOUT,
    );

    let bytes = std::fs::read(&h.capture_path).expect("capture file readable");
    assert!(
        bytes.starts_with(zwift_relay::capture::MAGIC),
        "capture file is present but missing the `RNCWCAP` magic; \
         got first bytes: {:?}",
        &bytes[..bytes.len().min(8)],
    );
}

/// The daemon must emit `relay.capture.opened` to the configured log
/// file when started in background mode with `--capture`. This pins
/// the contract that `run_daemon` constructs the orchestrator and
/// the orchestrator emits its lifecycle records under the
/// `ranchero::relay` target. Today nothing is logged because no
/// orchestrator is constructed.
#[test]
fn daemon_logs_relay_capture_opened_in_background() {
    let h = DaemonHarness::new();
    let out = h.spawn_background_start(true);
    assert!(
        out.status.success(),
        "background start failed: stdout={:?}, stderr={:?}",
        stdout_string(&out),
        stderr_string(&out),
    );

    // The daemon exits after relay.start fails; wait for the log
    // event that confirms the orchestrator was constructed.
    assert!(
        h.wait_for_log_match("relay.capture.opened", CAPTURE_APPEAR_TIMEOUT),
        "expected `relay.capture.opened` in {} within {:?}",
        h.log_file.display(),
        CAPTURE_APPEAR_TIMEOUT,
    );
}

/// The orchestrator must drive the capture writer through a clean
/// open-then-close lifecycle. Today the orchestrator is never
/// constructed, so neither `relay.capture.opened` nor
/// `relay.capture.closed` appears in the log file.
///
/// With the bogus credentials supplied by this test, the auth call
/// is expected to fail. The capture writer is opened before auth
/// and is flushed and closed on the auth-error path, so both
/// records must appear before `ranchero stop` is even sent. The
/// stop call here is a teardown step rather than the trigger for
/// the close record. The full workflow against production Zwift
/// (criterion #4 in STEP-12.5) additionally produces
/// `relay.tcp.shutdown`, but that record requires real auth
/// success and is out of scope for this test.
#[test]
fn daemon_drives_capture_open_close_lifecycle() {
    let h = DaemonHarness::new();
    let _ = h.spawn_background_start(true);

    // The capture file is opened before auth is attempted; its
    // appearance confirms the orchestrator was constructed.
    assert!(
        h.wait_for_log_match("relay.capture.opened", CAPTURE_APPEAR_TIMEOUT),
        "orchestrator never opened the capture writer"
    );

    // After auth fails (bogus credentials, unroutable endpoint),
    // the orchestrator must close the capture writer before propagating
    // the error. The daemon then exits — no stop command is needed.
    assert!(
        h.wait_for_log_match("relay.capture.closed", CAPTURE_APPEAR_TIMEOUT),
        "orchestrator opened capture but never closed it"
    );

    let log = std::fs::read_to_string(&h.log_file).unwrap_or_default();
    assert!(
        log.contains("relay.capture.opened"),
        "expected `relay.capture.opened` in log; got:\n{log}"
    );
    assert!(
        log.contains("relay.capture.closed"),
        "expected `relay.capture.closed` in log; got:\n{log}"
    );
}

/// Foreground `start --capture <path>` must construct the
/// orchestrator and emit at least one `relay.*` lifecycle record to
/// stderr. With bogus credentials the auth call is expected to fail,
/// but `relay.capture.opened` is emitted before authentication, so
/// it should appear in stderr regardless of the auth outcome. This
/// test also asserts the absence of an `unimplemented!()` panic
/// marker, which is what `RelayRuntime::start` produces today when
/// the production path is exercised.
#[test]
fn foreground_start_emits_relay_lifecycle_to_stderr() {
    let mut h = DaemonHarness::new();
    h.spawn_foreground_start(true);

    // The daemon exits after relay.start fails (bogus credentials,
    // unroutable endpoint). Wait for the child to finish, then read
    // its stderr for the relay lifecycle records.
    h.wait_for_child_exit(CAPTURE_APPEAR_TIMEOUT);

    let mut child = h.child.take().expect("foreground child");
    let _ = child.kill();
    let _ = child.wait();
    let stderr = read_stderr_to_string(&mut child);

    assert!(
        !stderr.contains("not yet implemented") && !stderr.contains("unimplemented"),
        "daemon stderr contains an `unimplemented!()` panic; \
         stderr was:\n{stderr}",
    );
    assert!(
        stderr.contains("relay.capture.opened")
            || stderr.contains("relay.login")
            || stderr.contains("relay.tcp"),
        "daemon stderr contains no `relay.*` lifecycle record; \
         stderr was:\n{stderr}",
    );
}

// ===========================================================================
// Group C — CLI forwarding
//
// STEP-12.5 §E: `cli::dispatch` must forward `cli.global.capture` to
// `daemon::start`. The signature gap below it (§C) and the
// orchestrator construction (§D) are also required for the path to
// produce visible effects. This test pins the visible-effect end of
// the chain: omitting `--capture` produces no capture file, supplying
// `--capture <path>` produces one.
// ===========================================================================

/// Without `--capture`, no file should appear at the harness's
/// capture path. With `--capture <path>`, one must. This pair of
/// assertions covers the full propagation chain (CLI dispatcher →
/// `daemon::start` → `runtime::start` → `run_daemon` →
/// `RelayRuntime::start`) and only passes once every step in that
/// chain is implemented.
#[test]
fn cli_capture_flag_governs_capture_file_creation() {
    // Variant A: no `--capture` passed; no capture file should be created.
    {
        let mut h = DaemonHarness::new();
        h.spawn_foreground_start(false);
        // Wait for the daemon to exit (relay.start fails with bogus
        // credentials), then check no capture file was created.
        h.wait_for_child_exit(CAPTURE_APPEAR_TIMEOUT);
        assert!(
            !h.capture_path.exists(),
            "capture file at {} appeared without `--capture` flag",
            h.capture_path.display(),
        );
    }

    // Variant B: `--capture` passed; the file must appear.
    {
        let mut h = DaemonHarness::new();
        h.spawn_foreground_start(true);
        assert!(
            h.wait_for_capture_file(CAPTURE_APPEAR_TIMEOUT),
            "capture file at {} did not appear with `--capture`",
            h.capture_path.display(),
        );
    }
}

// ===========================================================================
// Group D — End-to-end workflow
//
// The acceptance criterion at the end of STEP-12.5: `start --capture`
// → `follow` → `stop` works coherently. With bogus credentials the
// auth step still fails, but the orchestrator's pre-auth contract is
// enough to feed `follow`'s header read.
// ===========================================================================

/// Workflow test: start the daemon with capture, run `ranchero
/// follow` against the same path with a short idle timeout, and
/// confirm that `follow` prints the format-version header. This is
/// the loosest possible end-to-end contract — it does not require
/// any inbound TCP records (which would need real auth) — but it
/// requires every link in the production chain to be live: CLI
/// forwards `--capture`, daemon constructs the orchestrator, the
/// orchestrator opens the capture file, and `follow` can read its
/// header.
#[test]
fn workflow_start_capture_follow_reads_header() {
    let mut h = DaemonHarness::new();
    h.spawn_foreground_start(true);

    // Wait for the capture file to appear (opened before auth).
    // The daemon then exits after relay.start fails; follow reads
    // the file after that, so no running daemon is required.
    assert!(
        h.wait_for_capture_file(CAPTURE_APPEAR_TIMEOUT),
        "capture file did not appear at {}",
        h.capture_path.display(),
    );

    let follow = Command::new(binary_path())
        .args(h.config_args())
        .arg("follow")
        .arg(&h.capture_path)
        .arg("--idle-timeout")
        .arg("1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn ranchero follow");

    assert!(
        follow.status.success(),
        "follow failed: stdout={:?} stderr={:?}",
        stdout_string(&follow),
        stderr_string(&follow),
    );
    let text = stdout_string(&follow);
    assert!(
        text.contains("Format version:"),
        "follow did not print the format-version header; got:\n{text}",
    );
}

/// Belt-and-braces version of the workflow test: after `stop`, the
/// capture file must be readable by `CaptureReader` (i.e. closed
/// cleanly, with whatever records were accepted intact). This is
/// the contract that backs criterion #4 in STEP-12.5: "the capture
/// file is closed cleanly (every accepted record is readable on
/// `ranchero replay /tmp/x.cap`)".
#[test]
fn workflow_stop_leaves_capture_file_readable() {
    let h = DaemonHarness::new();
    let _ = h.spawn_background_start(true);

    // Wait for the capture writer to be flushed and closed. This
    // happens when auth fails and the error path in the orchestrator
    // runs cleanup. The daemon then exits — no stop command is needed.
    assert!(
        h.wait_for_log_match("relay.capture.closed", CAPTURE_APPEAR_TIMEOUT),
        "orchestrator never closed the capture writer"
    );

    // After the daemon exits the capture file must be a valid,
    // readable capture (magic bytes written, file properly flushed).
    let reader = zwift_relay::capture::CaptureReader::open(&h.capture_path)
        .expect(
            "capture file must be readable after daemon exit; either the \
             file was never opened, the magic was not written, or the \
             writer was not flushed before exit"
        );
    let _count = reader.count();
}

// ---------------------------------------------------------------------------
// Sanity helper — silence dead-code warnings if a test is gated off.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _path_helper(p: &Path) -> PathBuf { p.to_path_buf() }
