//! Integration tests for ranchero's tracing subscriber wiring.
//!
//! These spawn the binary so we exercise the live subscriber, not just
//! the directive-string helper. They rely on the STEP-04 emission
//! contract documented in `docs/plans/STEP-04-logging.md`:
//!
//! - `tracing::info!(pid = N, "ranchero started")`  on daemon startup
//! - `tracing::info!("ranchero stopped")`            on daemon shutdown
//! - `tracing::debug!(req = ?req, "control request received")` per
//!   control-socket request
//!
//! Until that contract is implemented every test here fails red.

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const READY_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const FLUSH_TIMEOUT: Duration = Duration::from_secs(3);

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_ranchero")
}

struct LogHarness {
    _dir: tempfile::TempDir,
    config_path: PathBuf,
    pidfile_path: PathBuf,
    socket_path: PathBuf,
    logfile_path: PathBuf,
    child: Option<Child>,
}

impl LogHarness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("ranchero.toml");
        // Short subdir so the UDS path stays under macOS's ~104 char limit.
        let state = dir.path().join("s");
        std::fs::create_dir_all(&state).unwrap();
        let pidfile_path = state.join("ranchero.pid");
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

        LogHarness {
            _dir: dir,
            config_path,
            pidfile_path,
            socket_path,
            logfile_path,
            child: None,
        }
    }

    fn config_args(&self) -> Vec<String> {
        vec![
            "--config".into(),
            self.config_path.to_string_lossy().into_owned(),
        ]
    }

    fn run(&self, extra: &[&str]) -> Output {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args()).args(extra);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.output().expect("spawn")
    }

    fn run_with_env(&self, env: &[(&str, &str)], extra: &[&str]) -> Child {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args()).args(extra);
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.spawn().expect("spawn")
    }

    fn spawn_foreground(&mut self, flags: &[&str]) -> &mut Child {
        let mut cmd = Command::new(binary_path());
        cmd.args(self.config_args());
        // Force foreground unless the caller already passed -D / --debug
        // (which implies --foreground via GlobalOpts::finalize).
        if !flags.iter().any(|&f| f == "-D" || f == "--debug") {
            cmd.arg("--foreground");
        }
        cmd.args(flags).arg("start");
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

impl Drop for LogHarness {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Ok(s) = std::fs::read_to_string(&self.pidfile_path)
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

fn stderr_string(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

/// Poll the logfile until it contains `needle` (case-insensitive) or the
/// flush timeout expires. Returns whatever was last read.
fn read_log_until(path: &Path, needle: &str) -> String {
    let needle_lc = needle.to_lowercase();
    let deadline = Instant::now() + FLUSH_TIMEOUT;
    let mut last = String::new();
    while Instant::now() < deadline {
        if let Ok(s) = std::fs::read_to_string(path) {
            if s.to_lowercase().contains(&needle_lc) {
                return s;
            }
            last = s;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    last
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn verbose_flag_emits_startup_info_to_stderr() {
    let mut h = LogHarness::new();
    h.spawn_foreground(&["-v"]);
    h.wait_for_pidfile().expect("daemon should start");

    let stop = h.run(&["stop"]);
    assert!(stop.status.success(), "stop failed: {:?}", stderr_string(&stop));
    h.wait_for_pidfile_gone();

    let child = h.child.take().unwrap();
    let out = child.wait_with_output().expect("wait");
    let stderr = stderr_string(&out);

    assert!(
        stderr.to_lowercase().contains("started"),
        "verbose foreground should log a startup info event to stderr, got: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("stopped"),
        "verbose foreground should log a shutdown info event to stderr, got: {stderr}"
    );
}

#[test]
fn default_silences_info_on_stderr() {
    // No flags → default level is warn. The startup/shutdown info events
    // should not reach stderr. (The user-facing `println!` lines go to
    // stdout, so they don't pollute this assertion.)
    let mut h = LogHarness::new();
    h.spawn_foreground(&[]);
    h.wait_for_pidfile().expect("daemon should start");

    let stop = h.run(&["stop"]);
    assert!(stop.status.success(), "stop failed: {:?}", stderr_string(&stop));
    h.wait_for_pidfile_gone();

    let child = h.child.take().unwrap();
    let out = child.wait_with_output().expect("wait");
    let stderr = stderr_string(&out);

    assert!(
        !stderr.contains("INFO"),
        "default level (warn) should suppress INFO events on stderr, got: {stderr}"
    );
}

#[test]
fn debug_flag_emits_control_debug_to_stderr() {
    let mut h = LogHarness::new();
    // -D implies --foreground.
    h.spawn_foreground(&["-D"]);
    h.wait_for_pidfile()
        .expect("daemon should start under --debug");

    // Trigger a control request so a debug event fires.
    let status = h.run(&["status"]);
    assert!(
        status.status.success(),
        "status failed: {:?}",
        stderr_string(&status)
    );

    let stop = h.run(&["stop"]);
    assert!(stop.status.success());
    h.wait_for_pidfile_gone();

    let child = h.child.take().unwrap();
    let out = child.wait_with_output().expect("wait");
    let stderr = stderr_string(&out);

    assert!(
        stderr.contains("DEBUG"),
        "debug flag should emit at least one DEBUG event to stderr, got: {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("control"),
        "debug-level control event expected in stderr, got: {stderr}"
    );
}

#[test]
fn rust_log_env_overrides_default_filter() {
    // Without RUST_LOG the default is warn and info is suppressed.
    // RUST_LOG=ranchero=info should let the startup event through even
    // with no `-v` flag.
    let h = LogHarness::new();
    let child = h.run_with_env(
        &[("RUST_LOG", "ranchero=info")],
        &["--foreground", "start"],
    );

    h.wait_for_pidfile().expect("daemon should start");

    let stop = h.run(&["stop"]);
    assert!(stop.status.success(), "stop failed: {:?}", stderr_string(&stop));
    h.wait_for_pidfile_gone();

    let out = child.wait_with_output().expect("wait");
    let stderr = stderr_string(&out);

    assert!(
        stderr.to_lowercase().contains("started"),
        "RUST_LOG=ranchero=info should let startup event through, got: {stderr}"
    );
}

#[test]
fn backgrounded_daemon_writes_lifecycle_to_logfile_without_flags() {
    // Regression: a plain `ranchero start` (no -v, no -D) must still
    // record startup and shutdown to the configured logfile. Lifecycle
    // events are operational, not diagnostic, and shouldn't be gated
    // behind a verbosity flag.
    let h = LogHarness::new();
    let out = h.run(&["start"]);
    assert!(
        out.status.success(),
        "backgrounded start failed: {:?}",
        stderr_string(&out)
    );
    h.wait_for_pidfile().expect("daemon should be running");

    let stop = h.run(&["stop"]);
    assert!(stop.status.success(), "stop failed: {:?}", stderr_string(&stop));
    h.wait_for_pidfile_gone();

    let log = read_log_until(&h.logfile_path, "stopped");
    assert!(
        log.to_lowercase().contains("started"),
        "logfile should contain the startup event under default flags, got: {log}"
    );
    assert!(
        log.to_lowercase().contains("stopped"),
        "logfile should contain the shutdown event under default flags, got: {log}"
    );
}

#[test]
fn logfile_is_appended_across_two_runs() {
    let h = LogHarness::new();

    // First cycle, no flags — relies on the default backgrounded filter.
    let out1 = h.run(&["start"]);
    assert!(out1.status.success(), "first start failed: {:?}", stderr_string(&out1));
    h.wait_for_pidfile().expect("first daemon should start");
    let stop1 = h.run(&["stop"]);
    assert!(stop1.status.success(), "first stop failed: {:?}", stderr_string(&stop1));
    h.wait_for_pidfile_gone();
    let _ = read_log_until(&h.logfile_path, "stopped");

    // Second cycle on the same logfile.
    let out2 = h.run(&["start"]);
    assert!(out2.status.success(), "second start failed: {:?}", stderr_string(&out2));
    h.wait_for_pidfile().expect("second daemon should start");
    let stop2 = h.run(&["stop"]);
    assert!(stop2.status.success(), "second stop failed: {:?}", stderr_string(&stop2));
    h.wait_for_pidfile_gone();

    let log = read_log_until(&h.logfile_path, "stopped");
    let started_count = log.to_lowercase().matches("started").count();
    assert_eq!(
        started_count, 2,
        "logfile should preserve both runs' started events (append, not truncate); got: {log}"
    );
}
