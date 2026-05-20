//! Control protocol between the `ranchero` CLI and the running daemon.
//!
//! The wire format is length-prefixed JSON: a 4-byte big-endian unsigned
//! length followed by that many bytes of JSON payload. The same envelope is
//! used in both directions. Length-prefixing is preferred over newline-
//! delimited JSON so future requests can carry arbitrary fields without
//! caring about embedded newlines.
//!
//! The control socket lives next to the PID file: same directory, name
//! `ranchero.sock`. This keeps the configuration surface small (one path)
//! and makes per-test isolation trivial.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A request from the CLI to the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "lowercase")]
pub enum ControlRequest {
    Status,
    Shutdown,
}

/// Live web-server figures the daemon reports over the control protocol.
/// The bind address, port, and HTTPS state are config-only and read by the
/// CLI from `ResolvedConfig`; only the active connection count is live.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WebStatus {
    pub connections: u64,
}

/// Response payload for `cmd: status`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub state: String,
    pub uptime_ms: u64,
    pub pid: u32,
    #[serde(default)]
    pub web: WebStatus,
}

/// Response payload for `cmd: shutdown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownResponse {
    pub ok: bool,
}

/// Tagged response so the client can route on `kind` without sniffing fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ControlResponse {
    Status(StatusResponse),
    Shutdown(ShutdownResponse),
}

/// Format a [`StatusResponse`] for the user-facing `ranchero status` output.
pub fn format_status_response(resp: &StatusResponse) -> String {
    format!("running (uptime {}ms, pid {})", resp.uptime_ms, resp.pid)
}

/// Canonical text for "no daemon running."
pub fn format_not_running() -> String {
    "not running".to_string()
}

/// Format the `Web server:` block for `ranchero status`.
///
/// `bind`, `port`, and `https` come from the resolved config and are known
/// whether or not the daemon is running. `connections` is `Some(n)` when the
/// daemon is up (the live count from the control protocol) and `None` when it
/// is down, in which case the connection line reads `daemon not running`.
pub fn format_web_status(
    bind: &str,
    port: u16,
    https: bool,
    connections: Option<u64>,
) -> String {
    let https_line = if https {
        format!("enabled (port {})", port + 1)
    } else {
        "disabled".to_string()
    };
    let connections_line = match connections {
        Some(n) => n.to_string(),
        None => "daemon not running".to_string(),
    };
    let mut lines = vec!["Web server:".to_string()];
    lines.push(format!("  {:<12} {bind}", "bind"));
    lines.push(format!("  {:<12} {port}", "port"));
    lines.push(format!("  {:<12} {https_line}", "https"));
    lines.push(format!("  {:<12} {connections_line}", "connections"));
    lines.join("\n")
}

/// Where the control socket lives relative to the PID file.
pub fn control_socket_path(pidfile: &Path) -> PathBuf {
    let dir = pidfile.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join("ranchero.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_request_status_serializes_round_trip() {
        let req = ControlRequest::Status;
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"cmd\""), "got {s}");
        assert!(s.contains("\"status\""), "got {s}");
        let back: ControlRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn control_request_shutdown_serializes_round_trip() {
        let req = ControlRequest::Shutdown;
        let s = serde_json::to_string(&req).unwrap();
        let back: ControlRequest = serde_json::from_str(&s).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn control_response_status_round_trip() {
        let r = ControlResponse::Status(StatusResponse {
            state: "running".into(),
            uptime_ms: 1234,
            pid: 42,
            web: WebStatus { connections: 3 },
        });
        let s = serde_json::to_string(&r).unwrap();
        let back: ControlResponse = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn control_response_is_human_printable() {
        let resp = StatusResponse {
            state: "running".into(),
            uptime_ms: 4321,
            pid: 7,
            web: WebStatus { connections: 0 },
        };
        let line = format_status_response(&resp);
        assert!(line.contains("running"), "got {line}");
        assert!(line.contains("4321"), "got {line}");
        assert!(line.contains("pid 7"), "got {line}");
    }

    #[test]
    fn not_running_message_mentions_not_running() {
        let line = format_not_running();
        assert!(line.to_lowercase().contains("not running"), "got {line}");
    }

    #[test]
    fn socket_path_lives_next_to_pidfile() {
        let pid = Path::new("/tmp/state/ranchero/ranchero.pid");
        let sock = control_socket_path(pid);
        assert_eq!(sock, Path::new("/tmp/state/ranchero/ranchero.sock"));
    }

    #[test]
    fn web_status_block_when_daemon_not_running_reports_config_and_absent_count() {
        let block = format_web_status("127.0.0.1", 1080, false, None);
        assert!(block.contains("Web server:"), "got {block}");
        assert!(block.contains("127.0.0.1"), "got {block}");
        assert!(block.contains("1080"), "got {block}");
        assert!(block.contains("disabled"), "https off should read disabled, got {block}");
        assert!(
            block.contains("daemon not running"),
            "absent connection count should read 'daemon not running', got {block}",
        );
    }

    #[test]
    fn web_status_block_when_running_reports_live_connection_count() {
        let block = format_web_status("0.0.0.0", 1080, false, Some(3));
        assert!(block.contains("0.0.0.0"), "got {block}");
        assert!(
            block.contains("connections")
                && block.lines().any(|l| l.contains("connections") && l.contains('3')),
            "running block should show the live connection count, got {block}",
        );
        assert!(
            !block.contains("daemon not running"),
            "running block must not say 'daemon not running', got {block}",
        );
    }

    #[test]
    fn web_status_block_https_enabled_names_the_tls_port() {
        let block = format_web_status("127.0.0.1", 1080, true, Some(0));
        assert!(
            block.contains("enabled (port 1081)"),
            "https on should name the TLS port (server_port + 1), got {block}",
        );
    }
}
