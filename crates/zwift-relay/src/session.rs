// SPDX-License-Identifier: AGPL-3.0-only
//
// Relay session: HTTPS login + periodic refresh supervisor. Mirrors
// `GameMonitor.login` (`sauce4zwift/src/zwift.mjs:1633-1658`) and the
// refresh scheduler at `zwift.mjs:1766-1932`.
//
// This file currently exposes the public surface as stubs so
// `tests/session.rs` compiles. Behavior lands in the green-state
// implementation. See `docs/plans/STEP-09-relay-session.md`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use prost::Message;
use rand::RngCore;
use tokio::sync::{RwLock, broadcast};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use zwift_proto::{LoginRequest, LoginResponse, RelaySessionRefreshResponse};

use crate::consts::{
    LOGIN_PATH, MIN_REFRESH_INTERVAL, PROTOBUF_CONTENT_TYPE, SESSION_REFRESH_FRACTION,
    SESSION_REFRESH_PATH,
};
use zwift_api::{DEFAULT_SOURCE, DEFAULT_USER_AGENT};

// --- POD types ----------------------------------------------------

/// One TCP relay endpoint from `LoginResponse.info.nodes.nodes`,
/// already filtered to the `lb_realm == 0 && lb_course == 0` generic
/// pool. The proto `TcpAddress.port` field is not stored here — the
/// listener port is always [`crate::TCP_PORT_SECURE`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpServer {
    pub ip: String,
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
    /// Sleep performed at the end of `login()` to let the relay
    /// servers stabilize before subsequent traffic. From sauce's
    /// `zwift.mjs:1651`:
    ///
    /// > "No joke this is required (100ms works about 50% of the time)"
    ///
    /// Production default 1 s. Tests against a wiremock can safely
    /// set this to `Duration::ZERO`.
    pub post_login_settle: Duration,
}

impl Default for RelaySessionConfig {
    fn default() -> Self {
        Self {
            source: DEFAULT_SOURCE.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            min_refresh_interval: MIN_REFRESH_INTERVAL,
            refresh_fraction: SESSION_REFRESH_FRACTION,
            post_login_settle: Duration::from_secs(1),
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
/// decode the response into a `RelaySession`. Honors
/// `config.post_login_settle` after the response is parsed (spec §4.1;
/// see STEP-09 plan "Open verification points" §4).
pub async fn login(
    auth: &zwift_api::ZwiftAuth,
    config: &RelaySessionConfig,
) -> Result<RelaySession> {
    let mut aes_key = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut aes_key);

    let req = LoginRequest {
        properties: None,
        key: aes_key.to_vec(),
    };
    let body = req.encode_to_vec();

    let athlete_id = auth.athlete_id().await.unwrap_or(0);
    tracing::info!(
        target: "ranchero::relay",
        athlete_id,
        "relay.session.login.started",
    );

    let resp = auth.post(LOGIN_PATH, PROTOBUF_CONTENT_TYPE, body).await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        return Err(Error::Status {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&bytes).to_string(),
        });
    }
    let parsed = LoginResponse::decode(bytes.as_ref())?;
    let relay_id = parsed
        .relay_session_id
        .ok_or(Error::MissingField("relay_session_id"))?;
    let expiration_min = parsed.expiration.ok_or(Error::MissingField("expiration"))?;
    let server_time_ms = parsed.info.time;

    let tcp_servers: Vec<TcpServer> = parsed
        .info
        .nodes
        .map(|cfg| cfg.nodes)
        .unwrap_or_default()
        .into_iter()
        .filter(|n| n.lb_realm.unwrap_or(0) == 0 && n.lb_course.unwrap_or(0) == 0)
        .filter_map(|n| Some(TcpServer { ip: n.ip? }))
        .collect();

    let servers_joined = tcp_servers
        .iter()
        .map(|s| s.ip.as_str())
        .collect::<Vec<_>>()
        .join(",");
    tracing::debug!(
        target: "ranchero::relay",
        servers = %servers_joined,
        "relay.session.tcp_servers",
    );
    tracing::info!(
        target: "ranchero::relay",
        relay_id,
        tcp_server_count = tcp_servers.len(),
        server_time_ms = server_time_ms.unwrap_or(0),
        expiration_min,
        "relay.session.login.ok",
    );

    if !config.post_login_settle.is_zero() {
        // sauce4zwift `zwift.mjs:1651`:
        //   "No joke this is required (100ms works about 50% of the time)"
        // Configurable so tests against a wiremock pay no settle cost.
        tokio::time::sleep(config.post_login_settle).await;
    }

    Ok(RelaySession {
        aes_key,
        relay_id,
        tcp_servers,
        expires_at: Instant::now() + Duration::from_secs(u64::from(expiration_min) * 60),
        server_time_ms,
    })
}

/// POST `RelaySessionRefreshRequest { relay_session_id: relay_id }` to
/// the relay host. Returns the new `expiration` (minutes) on success.
pub async fn refresh(
    auth: &zwift_api::ZwiftAuth,
    _config: &RelaySessionConfig,
    relay_id: u32,
) -> Result<u32> {
    // `RelaySessionRefreshRequest` is missing from the vendored
    // upstream proto (sauce has it, zoffline/zwift-offline doesn't —
    // see STEP-09 plan "Open verification points" §1). The body is a
    // single varint field: tag = (1 << 3) | wire_type_varint(0) = 0x08,
    // then the varint-encoded relay_session_id.
    let mut body = Vec::with_capacity(6);
    prost::encoding::encode_key(1, prost::encoding::WireType::Varint, &mut body);
    prost::encoding::encode_varint(u64::from(relay_id), &mut body);

    let resp = auth
        .post(SESSION_REFRESH_PATH, PROTOBUF_CONTENT_TYPE, body)
        .await?;
    let status = resp.status();
    let bytes = resp.bytes().await?;
    if !status.is_success() {
        return Err(Error::Status {
            status: status.as_u16(),
            body: String::from_utf8_lossy(&bytes).to_string(),
        });
    }
    let parsed = RelaySessionRefreshResponse::decode(bytes.as_ref())?;
    tracing::info!(
        target: "ranchero::relay",
        relay_id,
        new_expiration_min = parsed.expiration,
        "relay.session.refresh.ok",
    );
    Ok(parsed.expiration)
}

// --- supervisor ---------------------------------------------------

/// Owns the periodic refresh task. Not `Clone` — clients use
/// `current()` (snapshot) + `events()` (broadcast subscription).
pub struct RelaySessionSupervisor {
    inner: Arc<SupervisorInner>,
}

struct SupervisorInner {
    auth: zwift_api::ZwiftAuth,
    config: RelaySessionConfig,
    current: RwLock<Arc<RelaySession>>,
    events_tx: broadcast::Sender<SessionEvent>,
    refresh_task: Mutex<Option<JoinHandle<()>>>,
}

impl RelaySessionSupervisor {
    /// Performs the initial login synchronously, then spawns a
    /// background task that drives subsequent refreshes. The initial
    /// `LoggedIn` event fires on the spawned task (after `start`
    /// returns) so that callers have a chance to subscribe via
    /// [`Self::events`] before it lands.
    pub async fn start(
        auth: zwift_api::ZwiftAuth,
        config: RelaySessionConfig,
    ) -> Result<Self> {
        let session = login(&auth, &config).await?;
        let (events_tx, _) = broadcast::channel(64);
        let inner = Arc::new(SupervisorInner {
            auth,
            config,
            current: RwLock::new(Arc::new(session)),
            events_tx,
            refresh_task: Mutex::new(None),
        });

        let inner_for_task = inner.clone();
        let handle = tokio::spawn(async move {
            // Emit the initial LoggedIn from the spawned task. On a
            // current-thread runtime (which is what `#[tokio::test]`
            // gives us), this guarantees `start()` has returned and
            // the test has had its turn to subscribe via `events()`
            // before the event lands.
            let snapshot: RelaySession = {
                let arc = inner_for_task.current.read().await.clone();
                (*arc).clone()
            };
            tracing::info!(
                target: "ranchero::relay",
                relay_id = snapshot.relay_id,
                "relay.supervisor.logged_in",
            );
            let _ = inner_for_task
                .events_tx
                .send(SessionEvent::LoggedIn(snapshot));
            refresh_loop(inner_for_task).await;
        });
        *inner.refresh_task.lock().expect("refresh_task mutex") = Some(handle);

        Ok(Self { inner })
    }

    /// Snapshot of the currently-active session.
    pub async fn current(&self) -> RelaySession {
        let arc = self.inner.current.read().await.clone();
        (*arc).clone()
    }

    /// Subscribe to lifecycle events. Subscribers attached after a
    /// transition fires miss it; tests should subscribe before the
    /// event of interest is expected.
    pub fn events(&self) -> broadcast::Receiver<SessionEvent> {
        self.inner.events_tx.subscribe()
    }

    /// Cancels the background refresh task. The current snapshot
    /// remains readable until the supervisor is dropped.
    pub fn shutdown(&self) {
        if let Some(handle) = self
            .inner
            .refresh_task
            .lock()
            .expect("refresh_task mutex")
            .take()
        {
            handle.abort();
        }
    }
}

async fn refresh_loop(inner: Arc<SupervisorInner>) {
    let mut attempt: u32 = 0;
    loop {
        // Compute the next refresh deadline from the current session.
        let session_arc = inner.current.read().await.clone();
        let now = Instant::now();
        let remaining = session_arc.expires_at.saturating_duration_since(now);
        let scheduled =
            Duration::from_secs_f64(remaining.as_secs_f64() * inner.config.refresh_fraction);
        let delay = scheduled.max(inner.config.min_refresh_interval);
        tracing::info!(
            target: "ranchero::relay",
            scheduled_delay_ms = delay.as_millis() as u64,
            relay_id = session_arc.relay_id,
            "relay.supervisor.refresh.fire",
        );
        tokio::time::sleep(delay).await;

        match refresh(&inner.auth, &inner.config, session_arc.relay_id).await {
            Ok(new_expiration_min) => {
                let new_expires_at =
                    Instant::now() + Duration::from_secs(u64::from(new_expiration_min) * 60);
                let next = RelaySession {
                    expires_at: new_expires_at,
                    ..(*session_arc).clone()
                };
                let relay_id = next.relay_id;
                *inner.current.write().await = Arc::new(next);
                tracing::info!(
                    target: "ranchero::relay",
                    relay_id,
                    new_expires_in_s = new_expires_at
                        .saturating_duration_since(Instant::now())
                        .as_secs(),
                    "relay.supervisor.refreshed",
                );
                let _ = inner.events_tx.send(SessionEvent::Refreshed {
                    relay_id,
                    new_expires_at,
                });
                attempt = 0;
                continue;
            }
            Err(e) => {
                tracing::warn!(
                    target: "ranchero::relay",
                    relay_id = session_arc.relay_id,
                    error = %e,
                    "relay.supervisor.refresh_failed",
                );
                let _ = inner
                    .events_tx
                    .send(SessionEvent::RefreshFailed(e.to_string()));
            }
        }

        // Refresh failed — fall back to a full re-login, with
        // exponential backoff on consecutive failures.
        loop {
            // `attempt` is the number of consecutive failures so far;
            // the upcoming attempt's display number is `attempt + 1`,
            // and the backoff already paid before this attempt is
            // derived from the previous failure (zero on the first try).
            let attempt_no = attempt + 1;
            let backoff_ms = if attempt == 0 {
                0
            } else {
                let shift = attempt.min(10);
                (inner.config.min_refresh_interval * (1u32 << shift)).as_millis() as u64
            };
            tracing::info!(
                target: "ranchero::relay",
                attempt = attempt_no,
                backoff_ms,
                "relay.supervisor.relogin_attempt",
            );

            match login(&inner.auth, &inner.config).await {
                Ok(new_session) => {
                    *inner.current.write().await = Arc::new(new_session.clone());
                    tracing::info!(
                        target: "ranchero::relay",
                        attempt = attempt_no,
                        relay_id = new_session.relay_id,
                        "relay.supervisor.relogin_ok",
                    );
                    let _ = inner.events_tx.send(SessionEvent::LoggedIn(new_session));
                    attempt = 0;
                    break;
                }
                Err(e) => {
                    attempt += 1;
                    let shift = attempt.min(10);
                    let backoff = inner.config.min_refresh_interval * (1u32 << shift);
                    tracing::warn!(
                        target: "ranchero::relay",
                        attempt,
                        error = %e,
                        backoff_next_ms = backoff.as_millis() as u64,
                        "relay.supervisor.login_failed",
                    );
                    let _ = inner.events_tx.send(SessionEvent::LoginFailed {
                        attempt,
                        error: e.to_string(),
                    });
                    // Exponential backoff capped to keep the supervisor
                    // responsive after long outages. Cap exponent at 10
                    // to avoid runaway sleeps.
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
}
