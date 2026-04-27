// SPDX-License-Identifier: AGPL-3.0-only
//
// Relay session: HTTPS login + periodic refresh supervisor. Mirrors
// `GameMonitor.login` (`sauce4zwift/src/zwift.mjs:1633-1658`) and the
// refresh scheduler at `zwift.mjs:1766-1932`.
//
// This file currently exposes the public surface as stubs so
// `tests/session.rs` compiles. Behavior lands in the green-state
// implementation. See `docs/plans/STEP-09-relay-session.md`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, broadcast};
use tokio::time::Instant;

use crate::consts::{DEFAULT_RELAY_HOST, MIN_REFRESH_INTERVAL, SESSION_REFRESH_FRACTION};
use zwift_api::{DEFAULT_SOURCE, DEFAULT_USER_AGENT};

// --- POD types ----------------------------------------------------

/// One TCP relay endpoint from `LoginResponse.info.nodes.nodes`,
/// already filtered to the `lb_realm == 0 && lb_course == 0` generic
/// pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpServer {
    pub ip: String,
    pub port: u16,
}

/// Everything STEP 10 / 11 channels need to construct an IV and open
/// a socket.
#[derive(Debug, Clone)]
pub struct RelaySession {
    /// Client-chosen 16-byte AES session key. Used by the codec for
    /// every TCP/UDP packet on this session.
    pub aes_key: [u8; 16],
    /// `LoginResponse.relay_session_id` — the IV's `relayId` component.
    pub relay_id: u32,
    /// Filtered to `lb_realm == 0 && lb_course == 0`.
    pub tcp_servers: Vec<TcpServer>,
    /// `Instant` after which the session must have been refreshed.
    /// Computed as `logged_in_at + (expiration_minutes * 60s)`.
    pub expires_at: Instant,
    /// `LoginResponse.info.time` in milliseconds — server wall clock
    /// at login time. STEP 12's `WorldTimer` uses this for initial
    /// clock alignment.
    pub server_time_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct RelaySessionConfig {
    /// Base URL (with scheme). Production default
    /// `https://us-or-rly101.zwift.com`. Tests inject a wiremock URI.
    pub api_base: String,
    pub source: String,
    pub user_agent: String,
    pub min_refresh_interval: Duration,
    /// Fraction of the session's announced lifetime at which the
    /// supervisor schedules its refresh. Production default
    /// `SESSION_REFRESH_FRACTION` (0.90). Exposed as a config field so
    /// integration tests can tighten it without waiting tens of
    /// seconds of wall clock per scenario; production callers should
    /// leave it at the default unless they have a reason.
    pub refresh_fraction: f64,
}

impl Default for RelaySessionConfig {
    fn default() -> Self {
        Self {
            api_base: format!("https://{DEFAULT_RELAY_HOST}"),
            source: DEFAULT_SOURCE.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            min_refresh_interval: MIN_REFRESH_INTERVAL,
            refresh_fraction: SESSION_REFRESH_FRACTION,
        }
    }
}

#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// Initial login succeeded.
    LoggedIn(RelaySession),
    /// Periodic refresh extended the existing session.
    Refreshed {
        relay_id: u32,
        new_expires_at: Instant,
    },
    /// `/relay/session/refresh` failed; the supervisor will fall back
    /// to a full re-login.
    RefreshFailed(String),
    /// Re-login after a refresh failure also failed; supervisor is
    /// backing off and will retry. `attempt` increments per
    /// consecutive failure.
    LoginFailed { attempt: u32, error: String },
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("auth error: {0}")]
    Auth(#[from] zwift_api::Error),

    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },

    #[error("LoginResponse missing required field: {0}")]
    MissingField(&'static str),

    #[error("protobuf decode error: {0}")]
    Decode(#[from] prost::DecodeError),
}

pub type Result<T> = std::result::Result<T, Error>;

// --- single-shot async functions ----------------------------------

/// Generate a fresh AES key, POST `LoginRequest` to the relay host,
/// decode the response into a `RelaySession`. Includes the sauce
/// `await sleep(1000)` after login (spec §4.1; see STEP-09 plan
/// "Open verification points" §4).
pub async fn login(
    _auth: &zwift_api::ZwiftAuth,
    _config: &RelaySessionConfig,
) -> Result<RelaySession> {
    unimplemented!("STEP-09: relay login (16-byte AES key + LoginRequest POST + LoginResponse decode)")
}

/// POST `RelaySessionRefreshRequest { relay_session_id: relay_id }` to
/// the relay host. Returns the new `expiration` (minutes) on success.
pub async fn refresh(
    _auth: &zwift_api::ZwiftAuth,
    _config: &RelaySessionConfig,
    _relay_id: u32,
) -> Result<u32> {
    unimplemented!("STEP-09: POST RelaySessionRefreshRequest, decode RelaySessionRefreshResponse")
}

// --- supervisor ---------------------------------------------------

/// Owns the periodic refresh task. Cheap to clone? No — clients use
/// `current()` (snapshot) + `events()` (broadcast subscription).
pub struct RelaySessionSupervisor {
    #[allow(dead_code)]
    inner: Arc<SupervisorInner>,
}

#[allow(dead_code)]
struct SupervisorInner {
    auth: zwift_api::ZwiftAuth,
    config: RelaySessionConfig,
    current: RwLock<Arc<RelaySession>>,
    events_tx: broadcast::Sender<SessionEvent>,
}

impl RelaySessionSupervisor {
    /// Performs the initial login synchronously, then spawns a
    /// background task that drives subsequent refreshes.
    pub async fn start(
        _auth: zwift_api::ZwiftAuth,
        _config: RelaySessionConfig,
    ) -> Result<Self> {
        unimplemented!("STEP-09: initial login + spawn refresh-supervisor task")
    }

    /// Snapshot of the currently-active session.
    pub async fn current(&self) -> RelaySession {
        unimplemented!("STEP-09: clone the inner session via RwLock<Arc<RelaySession>>")
    }

    /// Subscribe to lifecycle events.
    pub fn events(&self) -> broadcast::Receiver<SessionEvent> {
        unimplemented!("STEP-09: broadcast::Sender::subscribe")
    }

    /// Cancels the background refresh task. The current snapshot
    /// remains readable until the supervisor is dropped.
    pub fn shutdown(&self) {
        unimplemented!("STEP-09: abort the refresh task")
    }
}
