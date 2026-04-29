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
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

use ranchero::config::{EditingMode, LogLevel, RedactedString, ResolvedConfig};
use ranchero::daemon::relay::RelayRuntime;

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
        main_email: Some(email.to_string()),
        main_password: Some(RedactedString::new(password.to_string())),
        monitor_email: None,
        monitor_password: None,
        server_bind: "127.0.0.1".into(),
        server_port: 1080,
        server_https: false,
        log_level: LogLevel::Info,
        log_file,
        pidfile,
        config_path: None,
        editing_mode: EditingMode::Default,
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
             pidfile = \"{}\"\n",
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

    /// Spawn `ranchero start` in the foreground with bogus credentials
    /// supplied via CLI, optional `--capture <path>`, and `--debug`
    /// so logs flow to stderr at INFO+.
    fn spawn_foreground_start(&mut self, with_capture: bool) -> &mut Child {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args());
        cmd.arg("--debug");
        cmd.arg("--mainuser").arg("noone@example.invalid");
        cmd.arg("--mainpassword").arg("not-a-real-password");
        if with_capture {
            cmd.arg("--capture").arg(&self.capture_path);
        }
        cmd.arg("start");
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
        cmd.arg("--mainuser").arg("noone@example.invalid");
        cmd.arg("--mainpassword").arg("not-a-real-password");
        if with_capture {
            cmd.arg("--capture").arg(&self.capture_path);
        }
        cmd.arg("start");
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

    let pid = h.wait_for_pidfile().expect("daemon must reach the UDS-ready state");
    assert!(pid > 0);

    assert!(
        h.wait_for_capture_file(CAPTURE_APPEAR_TIMEOUT),
        "capture file at {} was not created within {:?}. STEP-12.5 §D \
         requires `run_daemon` to construct a `RelayRuntime` and \
         propagate `capture_path` to its constructor.",
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

    let pid = h
        .wait_for_pidfile()
        .expect("daemon must reach the UDS-ready state under background start");
    assert!(pid > 0);

    let saw_capture_opened = h.wait_for_log_match(
        "relay.capture.opened",
        CAPTURE_APPEAR_TIMEOUT,
    );

    // Best-effort cleanup before assertion so a failed assertion
    // does not leave a stray daemon behind.
    let _ = h.run_cli(&["stop"]);

    assert!(
        saw_capture_opened,
        "expected `relay.capture.opened` in {} within {:?}. \
         STEP-12.5 §D: the daemon must construct a `RelayRuntime` \
         that emits its lifecycle records before any later step.",
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
    let _pid = h
        .wait_for_pidfile()
        .expect("daemon must reach the UDS-ready state");

    assert!(
        h.wait_for_log_match("relay.capture.opened", CAPTURE_APPEAR_TIMEOUT),
        "orchestrator never opened capture; nothing to shut down. \
         See STEP-12.5 §D.",
    );

    // Wait for the auth-error close to be logged before stopping
    // the daemon, so the assertion below does not race against
    // the orchestrator's cleanup.
    assert!(
        h.wait_for_log_match("relay.capture.closed", CAPTURE_APPEAR_TIMEOUT),
        "orchestrator opened capture but never closed it; the \
         capture cleanup path on `start` failure is not implemented. \
         See STEP-12.5 §B.",
    );

    let stop = h.run_cli(&["stop"]);
    assert!(
        stop.status.success(),
        "stop failed: {:?}",
        stderr_string(&stop),
    );
    assert!(
        h.wait_for_pidfile_gone(),
        "pidfile must be removed within the shutdown window",
    );

    let log = std::fs::read_to_string(&h.log_file).unwrap_or_default();
    assert!(
        log.contains("relay.capture.closed"),
        "expected `relay.capture.closed` in log; STEP-12.5 §D requires \
         shutdown to flush and close the capture writer. \
         Got log:\n{log}",
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
    let _ = h.wait_for_pidfile();

    // Allow the orchestrator a few seconds to reach the capture-open
    // step, which precedes the network auth call.
    std::thread::sleep(Duration::from_secs(3));

    // Send stop and collect stderr.
    let _ = h.run_cli(&["stop"]);

    let mut child = h.child.take().expect("foreground child");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        match child.try_wait().expect("try_wait") {
            Some(_) => break,
            None => std::thread::sleep(POLL_INTERVAL),
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    let stderr = read_stderr_to_string(&mut child);

    assert!(
        !stderr.contains("not yet implemented") && !stderr.contains("unimplemented"),
        "daemon stderr contains an `unimplemented!()` panic; \
         STEP-12.5 §B requires the `unimplemented!()` body in \
         `RelayRuntime::start` to be replaced with a default-DI \
         delegation. Stderr was:\n{stderr}",
    );
    assert!(
        stderr.contains("relay.capture.opened")
            || stderr.contains("relay.login")
            || stderr.contains("relay.tcp"),
        "daemon stderr contains no `relay.*` lifecycle record. \
         STEP-12.5 §D requires `run_daemon` to construct a \
         `RelayRuntime`; its pre-auth step emits \
         `relay.capture.opened` at INFO, and the auth and TCP \
         steps emit further `relay.*` records. \
         Stderr was:\n{stderr}",
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
    // Variant A: no `--capture` passed; the daemon should run, but
    // no file at the expected capture location should be created.
    {
        let mut h = DaemonHarness::new();
        h.spawn_foreground_start(false);
        let _ = h.wait_for_pidfile();
        std::thread::sleep(Duration::from_secs(2));
        let _ = h.run_cli(&["stop"]);

        assert!(
            !h.capture_path.exists(),
            "capture file at {} appeared without `--capture` flag; \
             the CLI is leaking a default capture path or the test \
             environment is not isolated.",
            h.capture_path.display(),
        );
    }

    // Variant B: `--capture` passed; the file must appear.
    {
        let mut h = DaemonHarness::new();
        h.spawn_foreground_start(true);
        let _ = h.wait_for_pidfile();
        assert!(
            h.wait_for_capture_file(CAPTURE_APPEAR_TIMEOUT),
            "capture file at {} did not appear with `--capture`; \
             STEP-12.5 §E + §D forwarding chain is broken.",
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
    let _ = h
        .wait_for_pidfile()
        .expect("daemon must reach UDS-ready state");

    assert!(
        h.wait_for_capture_file(CAPTURE_APPEAR_TIMEOUT),
        "capture file did not appear at {}",
        h.capture_path.display(),
    );

    // Run `ranchero follow` with a 1-second idle timeout against the
    // live capture file. Even without records it must print the
    // format-version header and exit cleanly.
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

    // Cleanup before assertions.
    let _ = h.run_cli(&["stop"]);

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
    let _pid = h
        .wait_for_pidfile()
        .expect("daemon must reach UDS-ready state");

    assert!(
        h.wait_for_capture_file(CAPTURE_APPEAR_TIMEOUT),
        "capture file did not appear at {}",
        h.capture_path.display(),
    );

    let stop = h.run_cli(&["stop"]);
    assert!(
        stop.status.success(),
        "stop failed: {:?}",
        stderr_string(&stop),
    );
    assert!(h.wait_for_pidfile_gone(), "pidfile must be removed after stop");

    // After clean shutdown, the file must be a valid capture.
    let reader = zwift_relay::capture::CaptureReader::open(&h.capture_path)
        .expect(
            "STEP-12.5 acceptance #4: the capture file must be readable \
             after stop. Either the file was never opened, the magic \
             was not written, or shutdown skipped flush.",
        );
    let _count = reader.count();
}

// ---------------------------------------------------------------------------
// Sanity helper — silence dead-code warnings if a test is gated off.
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn _path_helper(p: &Path) -> PathBuf { p.to_path_buf() }
