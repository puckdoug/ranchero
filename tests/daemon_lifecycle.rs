//! Integration tests for `ranchero start`, `ranchero stop`, and
//! `ranchero status`. Each test gets a unique config and PID/socket path
//! so the suite can run in parallel.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Output, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const RELAY_FAIL_TIMEOUT: Duration = Duration::from_secs(5);

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_ranchero")
}

/// Per-test state: temp dir holding a config file and the daemon's PID and
/// socket. Drop tears down any child still running.
struct DaemonHarness {
    _dir: tempfile::TempDir,
    config_path: PathBuf,
    pidfile_path: PathBuf,
    socket_path: PathBuf,
    child: Option<Child>,
}

impl DaemonHarness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("ranchero.toml");
        // Use a short subdirectory so the UDS path stays under macOS's ~104
        // char limit.
        let state = dir.path().join("s");
        std::fs::create_dir_all(&state).unwrap();
        let pidfile_path = state.join("ranchero.pid");
        let socket_path = state.join("ranchero.sock");

        let toml = format!(
            "schema_version = 1\n\
             [daemon]\n\
             pidfile = \"{}\"\n\
             [relay]\n\
             enabled = false\n",
            pidfile_path.display()
        );
        std::fs::write(&config_path, toml).unwrap();

        DaemonHarness {
            _dir: dir,
            config_path,
            pidfile_path,
            socket_path,
            child: None,
        }
    }

    /// Harness configured so that `RelayRuntime::start` will fail immediately.
    /// No credentials are present, so the missing-email check fails before any
    /// network activity. The `auth_base` URL is also set to an unreachable
    /// address as a secondary guard.
    fn new_failing_relay() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("ranchero.toml");
        let state = dir.path().join("s");
        std::fs::create_dir_all(&state).unwrap();
        let pidfile_path = state.join("ranchero.pid");
        let socket_path = state.join("ranchero.sock");

        let toml = format!(
            "schema_version = 1\n\
             [daemon]\n\
             pidfile = \"{}\"\n\
             [zwift]\n\
             auth_base = \"http://127.0.0.1:1\"\n",
            pidfile_path.display()
        );
        std::fs::write(&config_path, toml).unwrap();

        DaemonHarness { _dir: dir, config_path, pidfile_path, socket_path, child: None }
    }

    /// Harness where the pidfile lives inside a subdirectory that does not exist.
    /// The log file directory exists. Relay is disabled so the only failure is
    /// the missing pidfile directory.
    fn new_missing_pidfile_dir() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("ranchero.toml");
        let state = dir.path().join("s");
        std::fs::create_dir_all(&state).unwrap();
        // The pidfile subdirectory is deliberately not created.
        let pidfile_path = state.join("absent").join("ranchero.pid");
        let socket_path = state.join("ranchero.sock");
        let logfile_path = state.join("ranchero.log");

        let toml = format!(
            "schema_version = 1\n\
             [daemon]\n\
             pidfile = \"{}\"\n\
             [logging]\n\
             file = \"{}\"\n\
             [relay]\n\
             enabled = false\n",
            pidfile_path.display(),
            logfile_path.display(),
        );
        std::fs::write(&config_path, toml).unwrap();

        DaemonHarness { _dir: dir, config_path, pidfile_path, socket_path, child: None }
    }

    /// Harness where the log file lives inside a subdirectory that does not exist.
    /// The pidfile directory exists. Relay is disabled so the only failure is
    /// the missing log directory.
    fn new_missing_log_dir() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("ranchero.toml");
        let state = dir.path().join("s");
        std::fs::create_dir_all(&state).unwrap();
        let pidfile_path = state.join("ranchero.pid");
        let socket_path = state.join("ranchero.sock");
        // The log file subdirectory is deliberately not created.
        let logfile_path = state.join("absent").join("ranchero.log");

        let toml = format!(
            "schema_version = 1\n\
             [daemon]\n\
             pidfile = \"{}\"\n\
             [logging]\n\
             file = \"{}\"\n\
             [relay]\n\
             enabled = false\n",
            pidfile_path.display(),
            logfile_path.display(),
        );
        std::fs::write(&config_path, toml).unwrap();

        DaemonHarness { _dir: dir, config_path, pidfile_path, socket_path, child: None }
    }

    fn config_args(&self) -> Vec<String> {
        vec!["--config".into(), self.config_path.to_string_lossy().into_owned()]
    }

    fn run(&self, extra: &[&str]) -> Output {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args()).args(extra);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.output().expect("spawn")
    }

    fn spawn_foreground(&mut self, debug: bool) -> &mut Child {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args());
        if debug {
            cmd.arg("--debug");
        } else {
            cmd.arg("--foreground");
        }
        cmd.arg("start");
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let child = cmd.spawn().expect("spawn ranchero");
        self.child = Some(child);
        self.child.as_mut().unwrap()
    }

    fn wait_for_pidfile(&self) -> Option<u32> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(s) = std::fs::read_to_string(&self.pidfile_path)
                && let Ok(pid) = s.trim().parse::<u32>()
                && self.socket_path.exists()
            {
                return Some(pid);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        None
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

    fn wait_for_pidfile_gone(&self) -> bool {
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            if !self.pidfile_path.exists() {
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
        if let Ok(s) = std::fs::read_to_string(&self.pidfile_path)
            && let Ok(pid) = s.trim().parse::<u32>()
        {
            // Best-effort: SIGKILL any straggler.
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn start_writes_pid_file_and_status_reports_running() {
    let mut h = DaemonHarness::new();
    h.spawn_foreground(false);
    let pid = h.wait_for_pidfile().expect("daemon should become ready");
    assert!(pid > 0);

    let out = h.run(&["status"]);
    assert!(out.status.success(), "status exited with {:?}", out.status);
    let text = stdout_string(&out);
    assert!(
        text.contains("running"),
        "expected 'running' in status output, got: {text}"
    );
    assert!(
        text.contains(&format!("pid {pid}")),
        "expected pid {pid} in status output, got: {text}"
    );
}

#[test]
fn stop_clears_pid_file_and_status_reports_shutdown() {
    let mut h = DaemonHarness::new();
    h.spawn_foreground(false);
    h.wait_for_pidfile().expect("daemon should become ready");

    let stop = h.run(&["stop"]);
    assert!(stop.status.success(), "stop failed: {:?}", stderr_string(&stop));
    assert!(stdout_string(&stop).to_lowercase().contains("stopped"));

    assert!(h.wait_for_pidfile_gone(), "pidfile should be removed after stop");

    let status = h.run(&["status"]);
    let text = stdout_string(&status);
    assert!(
        text.to_lowercase().contains("not running"),
        "expected 'not running' after stop, got: {text}"
    );
}

#[test]
fn stop_when_not_running_reports_no_daemon() {
    let h = DaemonHarness::new();
    let out = h.run(&["stop"]);
    assert!(!out.status.success(), "stop on empty state should be non-zero");
    let combined = format!("{}{}", stdout_string(&out), stderr_string(&out));
    assert!(
        combined.to_lowercase().contains("not running"),
        "expected 'not running' message, got: {combined}"
    );
    // No panic / stack trace.
    assert!(
        !combined.contains("panicked") && !combined.contains("RUST_BACKTRACE"),
        "stop should not panic, got: {combined}"
    );
}

#[test]
fn status_when_not_running_reports_no_daemon() {
    let h = DaemonHarness::new();
    let out = h.run(&["status"]);
    let combined = format!("{}{}", stdout_string(&out), stderr_string(&out));
    assert!(
        combined.to_lowercase().contains("not running"),
        "expected 'not running' from status, got: {combined}"
    );
}

#[test]
fn start_when_already_running_refuses() {
    let mut h = DaemonHarness::new();
    h.spawn_foreground(false);
    let first_pid = h.wait_for_pidfile().expect("daemon should become ready");

    // Try to start a second daemon.
    let second = h.run(&["--foreground", "start"]);
    assert!(
        !second.status.success(),
        "second start should fail when one is already running"
    );
    let combined = format!("{}{}", stdout_string(&second), stderr_string(&second));
    assert!(
        combined.to_lowercase().contains("already running"),
        "expected 'already running' message, got: {combined}"
    );

    // First daemon must still be alive.
    let status = h.run(&["status"]);
    let text = stdout_string(&status);
    assert!(
        text.contains(&format!("pid {first_pid}")),
        "first daemon should still be running, got: {text}"
    );
}

#[test]
fn stale_pid_file_is_cleaned_up_on_start() {
    let mut h = DaemonHarness::new();
    // Plant a stale PID file. PID 999_999 is overwhelmingly unlikely to be
    // a live process — the OsProcessProbe will report it dead.
    std::fs::write(&h.pidfile_path, "999999\n").unwrap();
    assert!(h.pidfile_path.exists());

    h.spawn_foreground(false);
    let live_pid = h
        .wait_for_pidfile()
        .expect("daemon should start despite stale pidfile");
    assert_ne!(live_pid, 999_999, "pidfile should be replaced, not reused");
    assert!(live_pid > 0);
}

#[test]
fn backgrounded_start_returns_quickly_and_keeps_running() {
    let h = DaemonHarness::new();

    let start = Instant::now();
    let out = h.run(&["start"]);
    let elapsed = start.elapsed();

    assert!(out.status.success(), "backgrounded start failed: {:?}", stderr_string(&out));
    assert!(
        elapsed < Duration::from_secs(2),
        "backgrounded start should return quickly to the shell, took {elapsed:?}"
    );

    let pid = h
        .wait_for_pidfile()
        .expect("daemon should be running after backgrounded start");
    assert!(pid > 0);

    let stop = h.run(&["stop"]);
    assert!(stop.status.success(), "stop failed: {:?}", stderr_string(&stop));
}

#[test]
fn debug_flag_keeps_process_in_foreground() {
    let mut h = DaemonHarness::new();
    let child_pid = h.spawn_foreground(true).id();
    let daemon_pid = h
        .wait_for_pidfile()
        .expect("daemon should become ready under --debug");
    assert_eq!(
        child_pid, daemon_pid,
        "with --debug the spawned process should be the daemon (no fork); \
         spawned={child_pid} pidfile={daemon_pid}"
    );
}

// ---------------------------------------------------------------------------
// Defect 1 — relay.start failure must propagate (not continue in degraded mode)
// ---------------------------------------------------------------------------

#[test]
fn start_exits_nonzero_when_relay_start_fails() {
    let mut h = DaemonHarness::new_failing_relay();
    h.spawn_foreground(false);
    let status = h
        .wait_for_child_exit(RELAY_FAIL_TIMEOUT)
        .expect("process should exit within 5 s when relay.start fails; it hung instead");
    assert!(
        !status.success(),
        "expected non-zero exit when relay.start fails, got: {status:?}"
    );
}

#[test]
fn start_removes_pidfile_when_relay_start_fails() {
    let mut h = DaemonHarness::new_failing_relay();
    h.spawn_foreground(false);
    h.wait_for_child_exit(RELAY_FAIL_TIMEOUT)
        .expect("process should exit within 5 s when relay.start fails; it hung instead");
    assert!(
        !h.pidfile_path.exists(),
        "pidfile should be removed after failed relay.start"
    );
}

#[test]
fn start_removes_socket_when_relay_start_fails() {
    let mut h = DaemonHarness::new_failing_relay();
    h.spawn_foreground(false);
    h.wait_for_child_exit(RELAY_FAIL_TIMEOUT)
        .expect("process should exit within 5 s when relay.start fails; it hung instead");
    assert!(
        !h.socket_path.exists(),
        "control socket should be removed after failed relay.start"
    );
}

// ---------------------------------------------------------------------------
// Step 12.8 — startup validation runs before the process forks
// ---------------------------------------------------------------------------

const VALIDATION_TIMEOUT: Duration = Duration::from_secs(2);

/// Sub-1: missing email is reported on stderr and the process exits non-zero
/// without attempting any network I/O.
#[test]
fn start_exits_nonzero_and_prints_error_when_email_missing() {
    let mut h = DaemonHarness::new_failing_relay();
    h.spawn_foreground(false);
    let status = h
        .wait_for_child_exit(VALIDATION_TIMEOUT)
        .expect("process should exit within 2 s for a validation failure; it hung instead");
    assert!(!status.success(), "expected non-zero exit when email is missing, got: {status:?}");

    let child = h.child.take().unwrap();
    let out = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("missing main account email"),
        "expected 'missing main account email' in stderr, got: {stderr}"
    );
}

/// Sub-2: missing password is reported on stderr and the process exits non-zero.
#[test]
fn start_exits_nonzero_and_prints_error_when_password_missing() {
    let mut h = DaemonHarness::new_failing_relay();
    // Provide the email via CLI flag; password is still absent.
    let mut cmd = std::process::Command::new(binary_path());
    cmd.args(h.config_args())
        .args(["--mainuser", "user@example.com", "--foreground", "start"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = cmd.spawn().expect("spawn ranchero");
    h.child = Some(child);

    let status = h
        .wait_for_child_exit(VALIDATION_TIMEOUT)
        .expect("process should exit within 2 s for a validation failure; it hung instead");
    assert!(!status.success(), "expected non-zero exit when password is missing, got: {status:?}");

    let child = h.child.take().unwrap();
    let out = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("missing main account password"),
        "expected 'missing main account password' in stderr, got: {stderr}"
    );
}

/// Sub-3: no pidfile is written when validation fails.
#[test]
fn start_does_not_write_pidfile_when_validation_fails() {
    let mut h = DaemonHarness::new_failing_relay();
    h.spawn_foreground(false);
    h.wait_for_child_exit(VALIDATION_TIMEOUT)
        .expect("process should exit within 2 s for a validation failure; it hung instead");
    assert!(!h.pidfile_path.exists(), "pidfile must not be written when validation fails");
}

/// Sub-4: no control socket is created when validation fails.
#[test]
fn start_does_not_write_socket_when_validation_fails() {
    let mut h = DaemonHarness::new_failing_relay();
    h.spawn_foreground(false);
    h.wait_for_child_exit(VALIDATION_TIMEOUT)
        .expect("process should exit within 2 s for a validation failure; it hung instead");
    assert!(!h.socket_path.exists(), "control socket must not be created when validation fails");
}

/// Sub-5: missing pidfile directory is reported and the process exits non-zero.
#[test]
fn start_exits_nonzero_when_pidfile_directory_missing() {
    let mut h = DaemonHarness::new_missing_pidfile_dir();
    h.spawn_foreground(false);
    let status = h
        .wait_for_child_exit(VALIDATION_TIMEOUT)
        .expect("process should exit within 2 s when pidfile dir is missing; it hung instead");
    assert!(
        !status.success(),
        "expected non-zero exit when pidfile directory is missing, got: {status:?}"
    );

    let child = h.child.take().unwrap();
    let out = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("pidfile") && stderr.contains("not writable"),
        "expected 'pidfile' and 'not writable' in stderr, got: {stderr}"
    );
}

/// Sub-6: missing log directory is reported and the process exits non-zero.
#[test]
fn start_exits_nonzero_when_log_directory_missing() {
    let mut h = DaemonHarness::new_missing_log_dir();
    h.spawn_foreground(false);
    let status = h
        .wait_for_child_exit(VALIDATION_TIMEOUT)
        .expect("process should exit within 2 s when log dir is missing; it hung instead");
    assert!(
        !status.success(),
        "expected non-zero exit when log directory is missing, got: {status:?}"
    );

    let child = h.child.take().unwrap();
    let out = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(
        stderr.contains("log file") && stderr.contains("not writable"),
        "expected 'log file' and 'not writable' in stderr, got: {stderr}"
    );
}
