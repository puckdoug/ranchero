// SPDX-License-Identifier: AGPL-3.0-only
//! 17.34-T — Daemon boot starts the web server; `GET /api/` returns 200.
//!
//! The test spawns the full daemon in foreground mode, waits for the
//! pidfile and control socket to appear (daemon is up), then polls
//! `GET /api/` until it receives a 200 response or times out.
//!
//! A free port is reserved before the daemon launches by binding a
//! `TcpListener` to port 0, reading the OS-assigned port, closing the
//! listener, and writing that port into the daemon's TOML under
//! `[server] port = N`. This avoids needing to parse the daemon log to
//! discover the bound address.
//!
//! The test fails before 17.34-I because `run_daemon` does not yet call
//! `web::start`, so nothing is listening on the allocated port.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.34-T.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL:    Duration = Duration::from_millis(50);
const READY_TIMEOUT:    Duration = Duration::from_secs(15);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Isolated keyring service so the daemon never touches real credentials.
const TEST_KEYRING_SERVICE: &str = "ranchero-test-isolated";

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_ranchero")
}

/// Bind to port 0, let the OS pick a free port, then release it.
/// The returned port is available for the daemon to bind immediately after.
fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

struct DaemonHarness {
    _dir:        tempfile::TempDir,
    config_path: PathBuf,
    pidfile:     PathBuf,
    socket:      PathBuf,
    data_dir:    PathBuf,
    pages_dir:   PathBuf,
    web_port:    u16,
    child:       Option<Child>,
}

impl DaemonHarness {
    fn new() -> Self {
        let web_port = pick_free_port();

        let dir = tempfile::tempdir().expect("tempdir");
        let config_path = dir.path().join("ranchero.toml");
        let state = dir.path().join("s");
        std::fs::create_dir_all(&state).unwrap();
        let pidfile  = state.join("ranchero.pid");
        let socket   = state.join("ranchero.sock");
        let data_dir = dir.path().join("d");
        std::fs::create_dir_all(&data_dir).unwrap();
        let pages_dir = dir.path().join("p");
        std::fs::create_dir_all(&pages_dir).unwrap();

        let toml = format!(
            "schema_version = 1\n\
             [daemon]\n\
             pidfile = \"{}\"\n\
             [server]\n\
             port = {web_port}\n\
             [relay]\n\
             enabled = false\n\
             [keyring]\n\
             service = \"{TEST_KEYRING_SERVICE}\"\n",
            pidfile.display(),
        );
        std::fs::write(&config_path, toml).unwrap();

        DaemonHarness { _dir: dir, config_path, pidfile, socket, data_dir, pages_dir, web_port, child: None }
    }

    fn config_args(&self) -> Vec<String> {
        vec!["--config".into(), self.config_path.to_string_lossy().into_owned()]
    }

    fn spawn_foreground(&mut self) -> &mut Child {
        let mut cmd = Command::new(binary_path());
        cmd.env("RANCHERO_DATA_DIR",  &self.data_dir)
           .env("RANCHERO_PAGES_ROOT", &self.pages_dir)
           .args(self.config_args())
           .arg("--foreground")
           .arg("start")
           .stdin(Stdio::null())
           .stdout(Stdio::piped())
           .stderr(Stdio::piped());
        let child = cmd.spawn().expect("spawn ranchero");
        self.child = Some(child);
        self.child.as_mut().unwrap()
    }

    /// Poll until the pidfile and control socket appear (daemon event loop is running).
    fn wait_for_ready(&self) -> bool {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.pidfile.exists() && self.socket.exists() {
                return true;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        false
    }

    /// Poll until `GET /api/` returns `HTTP/1.1 200` or the timeout expires.
    fn wait_for_http_200(&self) -> bool {
        let addr = format!("127.0.0.1:{}", self.web_port);
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(mut stream) = TcpStream::connect(&addr) {
                let req = format!("GET /api/ HTTP/1.0\r\nHost: {addr}\r\n\r\n");
                if stream.write_all(req.as_bytes()).is_ok() {
                    let mut resp = String::new();
                    if stream.read_to_string(&mut resp).is_ok()
                        && resp.starts_with("HTTP/1.1 200")
                    {
                        return true;
                    }
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        false
    }

    fn stop(&self) {
        Command::new(binary_path())
            .env("RANCHERO_DATA_DIR",  &self.data_dir)
            .env("RANCHERO_PAGES_ROOT", &self.pages_dir)
            .args(self.config_args())
            .arg("stop")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok();

        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            if !self.pidfile.exists() {
                break;
            }
            std::thread::sleep(POLL_INTERVAL);
        }
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

// ---------------------------------------------------------------------------
// Test
// ---------------------------------------------------------------------------

#[test]
#[ignore = "slow: full daemon boot"]
fn daemon_starts_web_server_and_api_root_returns_200() {
    let mut harness = DaemonHarness::new();
    harness.spawn_foreground();

    assert!(
        harness.wait_for_ready(),
        "daemon must write its pidfile and control socket within {READY_TIMEOUT:?}",
    );
    assert!(
        harness.wait_for_http_200(),
        "GET /api/ on port {} must return HTTP 200 within {READY_TIMEOUT:?} of daemon ready; \
         web server is not started by run_daemon until 17.34-I is implemented",
        harness.web_port,
    );

    harness.stop();
}
