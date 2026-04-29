//! Daemon lifecycle: PID file management, control socket protocol,
//! foreground/background process orchestration.
//!
//! Three public entry points are wired into the CLI:
//! - [`start`] launches the long-running daemon (foreground or backgrounded).
//! - [`stop`] sends a shutdown request to the running daemon (or reports none).
//! - [`status`] queries the running daemon and prints a human-readable line.
//!
//! The daemon itself is currently a placeholder event loop that reports only
//! `uptime_ms`, `pid`, and `state`; richer counters arrive in later steps.

pub mod control;
pub mod pidfile;
pub mod probe;
pub mod relay;
pub mod runtime;
pub mod validate;

use std::path::PathBuf;
use std::process::ExitCode;

pub use control::{ControlRequest, ControlResponse, ShutdownResponse, StatusResponse,
    format_not_running, format_status_response, control_socket_path};
pub use pidfile::Pidfile;
pub use probe::{OsProcessProbe, ProcessProbe};

use crate::config::ResolvedConfig;
use crate::logging::LogOpts;

/// Errors returned from daemon operations to the CLI dispatcher.
#[derive(Debug)]
pub enum DaemonError {
    /// I/O error (PID file, socket, fork).
    Io(std::io::Error),
    /// JSON encode/decode error on the control protocol.
    Protocol(String),
    /// User asked to start a daemon while another is already alive.
    AlreadyRunning(u32),
    /// User asked to stop or query a daemon that isn't running.
    NotRunning,
    /// Backgrounding requested on a platform that does not support it yet.
    BackgroundUnsupported,
    /// One or more pre-start validation checks failed.
    StartupValidation(validate::StartupValidationErrors),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DaemonError::Io(e) => write!(f, "I/O error: {e}"),
            DaemonError::Protocol(m) => write!(f, "control protocol error: {m}"),
            DaemonError::AlreadyRunning(pid) =>
                write!(f, "ranchero is already running (pid {pid})"),
            DaemonError::NotRunning => write!(f, "ranchero is not running"),
            DaemonError::BackgroundUnsupported =>
                write!(f, "backgrounding is not supported on this platform; \
                       pass --foreground"),
            DaemonError::StartupValidation(errs) => errs.fmt(f),
        }
    }
}

impl std::error::Error for DaemonError {}

impl From<std::io::Error> for DaemonError {
    fn from(e: std::io::Error) -> Self { DaemonError::Io(e) }
}

impl From<serde_json::Error> for DaemonError {
    fn from(e: serde_json::Error) -> Self { DaemonError::Protocol(e.to_string()) }
}

/// Summary of paths the daemon needs to operate. Derived from the resolved
/// configuration so tests can hand in fake paths.
#[derive(Debug, Clone)]
pub struct DaemonPaths {
    pub pidfile: PathBuf,
    pub socket: PathBuf,
}

impl DaemonPaths {
    pub fn from_config(cfg: &ResolvedConfig) -> Self {
        let pidfile = cfg.pidfile.clone();
        let socket = control_socket_path(&pidfile);
        Self { pidfile, socket }
    }
}

/// `ranchero start` entry point.
pub fn start(
    cfg: &ResolvedConfig,
    foreground: bool,
    log_opts: LogOpts,
    capture_path: Option<PathBuf>,
) -> Result<ExitCode, DaemonError> {
    runtime::start(cfg, foreground, log_opts, capture_path)
}

/// `ranchero stop` entry point.
pub fn stop(cfg: &ResolvedConfig) -> Result<ExitCode, DaemonError> {
    runtime::stop(cfg)
}

/// `ranchero status` entry point.
pub fn status(cfg: &ResolvedConfig) -> Result<ExitCode, DaemonError> {
    runtime::status(cfg)
}
