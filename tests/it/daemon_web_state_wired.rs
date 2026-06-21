// SPDX-License-Identifier: AGPL-3.0-only
//! 17.36-T — Daemon boot wires up the production `WebState`.
//!
//! Two behaviours can only hold once `run_daemon` builds a fully wired
//! `WebState` (with `with_rpc` and `and_game_events`):
//!
//! 1. `POST /api/rpc/v1/getVersion` returns `{"success":true,"data":"<version>"}`.
//!    Until 17.36-I, `WebState::new()` leaves `rpc: None`, so the handler
//!    returns `{"success":false,"error":"..."}`.
//!
//! 2. A WebSocket `subscribe` with `source:"stats"` returns `success:true`.
//!    Until 17.36-I, `WebState::new()` leaves `game_events_tx: None`, so
//!    the subscription engine returns `"stats source not available"`.
//!
//! See docs/plans/STEP-17-web-server.md, item 17.36-T.

#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

const POLL_INTERVAL:    Duration = Duration::from_millis(50);
const READY_TIMEOUT:    Duration = Duration::from_secs(15);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

const TEST_KEYRING_SERVICE: &str = "ranchero-test-isolated";

fn binary_path() -> &'static str {
    env!("CARGO_BIN_EXE_ranchero")
}

fn pick_free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

// ---------------------------------------------------------------------------
// Minimal sync WebSocket client helpers
// ---------------------------------------------------------------------------

/// Send an HTTP GET with WebSocket upgrade headers on `stream`.
fn ws_upgrade(stream: &mut TcpStream, host: &str, path: &str) {
    let req = format!(
        "GET {path} HTTP/1.1\r\n\
         Host: {host}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n"
    );
    stream.write_all(req.as_bytes()).expect("ws upgrade write");
    // Consume the 101 response.
    let mut buf = [0u8; 1024];
    let mut total = 0;
    loop {
        let n = stream.read(&mut buf[total..]).expect("ws upgrade read");
        total += n;
        if total >= 4 && &buf[total - 4..total] == b"\r\n\r\n" {
            break;
        }
    }
}

/// Encode a text WebSocket frame (client-to-server, masked).
fn ws_text_frame(payload: &str) -> Vec<u8> {
    let data = payload.as_bytes();
    let len = data.len();
    assert!(len < 126, "test payloads are small");
    let mask = [0xAB, 0xCD, 0xEF, 0x01u8];
    let mut frame = vec![
        0x81u8,              // FIN + opcode text
        0x80 | len as u8,   // MASK + length
        mask[0], mask[1], mask[2], mask[3],
    ];
    for (i, &b) in data.iter().enumerate() {
        frame.push(b ^ mask[i % 4]);
    }
    frame
}

/// Read one text WebSocket frame from `stream` (server-to-client, unmasked).
fn ws_read_text(stream: &mut TcpStream) -> String {
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).expect("ws frame header");
    let len = (header[1] & 0x7f) as usize;
    assert!(len < 126, "test payloads are small");
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).expect("ws frame payload");
    String::from_utf8(payload).expect("utf-8 payload")
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

        DaemonHarness {
            _dir: dir, config_path, pidfile, socket,
            data_dir, pages_dir, web_port, child: None,
        }
    }

    fn config_args(&self) -> Vec<String> {
        vec!["--config".into(), self.config_path.to_string_lossy().into_owned()]
    }

    fn spawn_foreground(&mut self) -> &mut Child {
        let mut cmd = Command::new(binary_path());
        cmd.env("RANCHERO_DATA_DIR",   &self.data_dir)
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

    fn stop(&self) {
        Command::new(binary_path())
            .env("RANCHERO_DATA_DIR",   &self.data_dir)
            .env("RANCHERO_PAGES_ROOT", &self.pages_dir)
            .args(self.config_args())
            .arg("stop")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok();
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        while Instant::now() < deadline {
            if !self.pidfile.exists() { break; }
            std::thread::sleep(POLL_INTERVAL);
        }
    }

    /// POST JSON to path, return the response body (after the blank header line).
    fn http_post_json(&self, path: &str, body: &str) -> String {
        let addr = format!("127.0.0.1:{}", self.web_port);
        let mut stream = TcpStream::connect(&addr).expect("http connect");
        let req = format!(
            "POST {path} HTTP/1.1\r\n\
             Host: {addr}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\
             \r\n\
             {body}",
            body.len(),
        );
        stream.write_all(req.as_bytes()).expect("http write");
        let mut resp = String::new();
        stream.read_to_string(&mut resp).expect("http read");
        // Split at the blank line separating headers from body.
        resp.split("\r\n\r\n")
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_owned()
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
// Tests
// ---------------------------------------------------------------------------

/// `POST /api/rpc/v1/getVersion` returns `{"success":true,"data":"<version>"}`.
///
/// Fails until 17.36-I registers `RpcRegistry` in `run_daemon`; until then
/// the handler returns `success:false` because `WebState.rpc` is `None`.
#[test]
#[ignore = "slow: full daemon boot (~1 s)"]
fn rpc_getversion_returns_success_after_daemon_boot() {
    let mut harness = DaemonHarness::new();
    harness.spawn_foreground();
    assert!(harness.wait_for_ready(), "daemon must reach ready state");

    let body = harness.http_post_json("/api/rpc/v1/getVersion", "[]");
    let val: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("response must be valid JSON; got: {body}"));

    assert_eq!(
        val["success"], true,
        "getVersion must return success:true once run_daemon registers RpcRegistry \
         (17.36-I); got: {val}",
    );
    assert!(
        val["data"].is_string(),
        "getVersion data must be a version string; got: {val}",
    );

    harness.stop();
}

/// A WebSocket `subscribe` with `source:"stats"` returns `success:true`.
///
/// Fails until 17.36-I attaches `game_events_tx` in `run_daemon`; until then
/// `WebState.game_events_tx` is `None` and the subscription engine replies
/// with `"stats source not available"`.
#[test]
#[ignore = "slow: full daemon boot (~1 s)"]
fn ws_subscribe_stats_returns_success_after_daemon_boot() {
    let mut harness = DaemonHarness::new();
    harness.spawn_foreground();
    assert!(harness.wait_for_ready(), "daemon must reach ready state");

    let addr = format!("127.0.0.1:{}", harness.web_port);
    let mut stream = TcpStream::connect(&addr).expect("ws connect");
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

    ws_upgrade(&mut stream, &addr, "/api/ws/events");

    let subscribe = serde_json::json!({
        "type": "request",
        "uid": 1,
        "data": {
            "method": "subscribe",
            "arg": { "event": "athlete/watching", "source": "stats", "subId": 7 }
        }
    });
    stream.write_all(&ws_text_frame(&subscribe.to_string())).expect("ws send");

    let response = ws_read_text(&mut stream);
    let val: serde_json::Value = serde_json::from_str(&response)
        .unwrap_or_else(|_| panic!("response must be valid JSON; got: {response}"));

    assert_eq!(
        val["success"], true,
        "stats subscribe must return success:true once run_daemon attaches \
         game_events_tx (17.36-I); got: {val}",
    );

    harness.stop();
}
