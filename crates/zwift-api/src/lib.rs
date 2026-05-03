// SPDX-License-Identifier: AGPL-3.0-only
//
// Zwift OAuth2 (Keycloak password-grant) and authenticated REST client.
//
// Ported from sauce4zwift `src/zwift.mjs` (`ZwiftAPI` class, lines
// ~327-500). See docs/plans/STEP-07-auth-and-rest.md and
// docs/ARCHITECTURE-AND-RUST-SPEC.md §3 / §7.5 for the contract.

use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Default Keycloak host used for token issuance and refresh.
pub const DEFAULT_AUTH_HOST: &str = "secure.zwift.com";

/// Default Zwift game-API host used for authenticated REST calls.
pub const DEFAULT_API_HOST: &str = "us-or-rly101.zwift.com";

/// Keycloak `client_id`. The literal space is intentional and is what
/// Zwift's first-party game client sends; serde_urlencoded encodes it
/// as `Zwift+Game+Client` in the form body.
pub const CLIENT_ID: &str = "Zwift Game Client";

/// Default `Source` header sent on every authenticated REST call.
///
/// We mimic a real Zwift desktop client (matching sauce4zwift's
/// `zwift.mjs:458`, which sends the same value) because Zwift's API
/// is suspected to inspect this header and may reject requests that
/// identify themselves as a third-party tool. The architecture spec
/// §3.3 mistakenly documented `"Sauce for Zwift"`; the actual
/// upstream code — and the value real game clients send — is
/// `"Game Client"`.
///
/// Override via [`Config::source`] when you want to identify
/// honestly (e.g. once Zwift's tolerance has been confirmed against
/// real servers, or when targeting a self-hosted `zwift-offline`
/// instance that doesn't care).
pub const DEFAULT_SOURCE: &str = "Game Client";

/// Default `User-Agent` header sent on every authenticated REST call.
///
/// Matches the full Zwift game-client string from sauce4zwift
/// (`zwift.mjs:459`). STEP-12.14 §C7: Zwift's API is suspected to
/// inspect the UA and degrade responses for unknown clients.
pub const DEFAULT_USER_AGENT: &str =
    "CNL/3.44.0 (Darwin Kernel 23.2.0) zwift/1.0.122968 game/1.54.0 curl/8.4.0";

/// Path of the Keycloak token endpoint (used for both `password` and
/// `refresh_token` grants).
pub const TOKEN_PATH: &str = "/auth/realms/zwift/protocol/openid-connect/token";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("HTTP transport error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("invalid token response: {0}")]
    InvalidTokenResponse(String),

    /// 401 from the token or profile endpoint — bad credentials or expired
    /// token.
    #[error("authentication failed: unauthorized: {0}")]
    AuthFailedUnauthorized(String),

    /// 403 from the token or profile endpoint — credentials valid but access
    /// denied.
    #[error("authentication failed: forbidden: {0}")]
    AuthFailedForbidden(String),

    /// 200 from the profile endpoint but the body cannot be decoded as a
    /// [`Profile`] (missing or wrong-type `id` field).
    #[error("authentication failed: unexpected response shape: {0}")]
    AuthFailedBadSchema(String),

    /// Any other non-success status from the token or profile endpoint.
    #[error("authentication failed: {0}")]
    AuthFailedUnknown(String),

    #[error("no tokens available; call login() first")]
    NotAuthenticated,

    #[error("token refresh failed: {0}")]
    RefreshFailed(String),

    #[error("HTTP {status}: {body}")]
    Status { status: u16, body: String },

    /// 200 from a protobuf endpoint but the body cannot be decoded.
    #[error("protobuf decode failed: {0}")]
    ProtobufDecode(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Tokens parsed from a Keycloak token response. Field names mirror the
/// Keycloak/OAuth2 wire format.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Lifetime of `access_token` in seconds.
    pub expires_in: u64,
    /// Lifetime of `refresh_token` in seconds.
    #[serde(default)]
    pub refresh_expires_in: u64,
    #[serde(default)]
    pub token_type: String,
}

/// Minimal athlete profile from `GET /api/profiles/me`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct Profile {
    pub id: i64,
}

/// Direction tag passed to a [`CaptureSink`] for each HTTP exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureDirection {
    Inbound,
    Outbound,
}

/// Transport tag passed to a [`CaptureSink`]. `zwift-api` only
/// produces `Http`, but the enum is left open so a future shared
/// `CaptureSink` consumer can pattern-match on the same variants the
/// daemon's wire-capture writer uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureTransport {
    Http,
}

/// Sink that receives every HTTP request and response body issued by
/// a [`ZwiftAuth`] instance. The daemon's adapter forwards these into
/// the wire-capture file (`zwift_relay::capture::CaptureWriter`).
///
/// `zwift-relay` already depends on `zwift-api`, so the inverse
/// dependency would be a cycle. The trait lives here and the daemon
/// implements it for an adapter type.
pub trait CaptureSink: Send + Sync + 'static {
    fn record(&self, direction: CaptureDirection, transport: CaptureTransport, payload: &[u8]);
}

/// Owned HTTP response surface returned by [`ZwiftAuth::post`] and
/// [`ZwiftAuth::fetch`]. The body is read inside the auth client so
/// the [`CaptureSink`] can record it; callers consume the bytes via
/// [`Self::bytes`] (kept async for source compatibility with the
/// previous `reqwest::Response`-returning API).
#[derive(Debug)]
pub struct HttpResponse {
    status: reqwest::StatusCode,
    body: Vec<u8>,
}

impl HttpResponse {
    pub fn status(&self) -> reqwest::StatusCode {
        self.status
    }

    pub async fn bytes(self) -> Result<Vec<u8>> {
        Ok(self.body)
    }
}

/// Hosts and fixed-header configuration for a `ZwiftAuth`. Tests inject
/// the wiremock URI here (with scheme); production constructs via
/// `Config::default()`.
#[derive(Debug, Clone)]
pub struct Config {
    /// Base URL (scheme + host) for the Keycloak auth host. Production
    /// default: `https://secure.zwift.com`.
    pub auth_base: String,
    /// Base URL (scheme + host) for the game REST API host. Production
    /// default: `https://us-or-rly101.zwift.com`.
    pub api_base: String,
    pub source: String,
    pub user_agent: String,
    /// `Platform` header value. Sauce4zwift sends `"OSX"` on every
    /// authenticated request (STEP-12.14 §C6). Default `"OSX"`.
    pub platform: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auth_base: format!("https://{DEFAULT_AUTH_HOST}"),
            api_base: format!("https://{DEFAULT_API_HOST}"),
            source: DEFAULT_SOURCE.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
            platform: "OSX".to_string(),
        }
    }
}

/// OAuth2 client for the Zwift Keycloak realm plus an authenticated
/// `fetch()` helper that retries once on 401 by refreshing the access
/// token inline.
///
/// Cloning is cheap: the inner state is `Arc`-wrapped so multiple tasks
/// (e.g. main + monitor accounts elsewhere; the background refresh task
/// here) share the same token store.
#[derive(Clone)]
pub struct ZwiftAuth {
    inner: Arc<Inner>,
}

struct Inner {
    http: reqwest::Client,
    config: Config,
    tokens: RwLock<Option<Tokens>>,
    /// Cached from `GET /api/profiles/me` during `login()`.
    profile: RwLock<Option<Profile>>,
    /// Handle to the in-flight preemptive-refresh task, if any.
    /// `std::sync::Mutex` is fine here: the critical section is just a
    /// `take`/`replace` and never crosses an `.await`.
    refresh_task: Mutex<Option<JoinHandle<()>>>,
    /// Optional sink for raw HTTP request and response bytes, set by
    /// the daemon at `start_all_inner` time so the wire-capture file
    /// can replay HTTP exchanges. Default is `None` (no capture).
    capture_sink: Mutex<Option<Arc<dyn CaptureSink>>>,
}

impl Inner {
    fn record_outbound(&self, payload: &[u8]) {
        if let Some(sink) = self.capture_sink.lock().expect("capture_sink mutex").as_ref() {
            sink.record(CaptureDirection::Outbound, CaptureTransport::Http, payload);
        }
    }

    fn record_inbound(&self, payload: &[u8]) {
        if let Some(sink) = self.capture_sink.lock().expect("capture_sink mutex").as_ref() {
            sink.record(CaptureDirection::Inbound, CaptureTransport::Http, payload);
        }
    }
}

impl ZwiftAuth {
    /// Build a new auth client with the given `Config`. A fresh
    /// `reqwest::Client` is created internally.
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::builder()
            .build()
            .expect("reqwest::Client::build with default config should never fail");
        Self::with_client(http, config)
    }

    /// Build a new auth client using a caller-supplied `reqwest::Client`.
    /// Useful for sharing connection pools across multiple `ZwiftAuth`
    /// instances (e.g. main + monitor accounts).
    pub fn with_client(http: reqwest::Client, config: Config) -> Self {
        Self {
            inner: Arc::new(Inner {
                http,
                config,
                tokens: RwLock::new(None),
                profile: RwLock::new(None),
                refresh_task: Mutex::new(None),
                capture_sink: Mutex::new(None),
            }),
        }
    }

    /// Attach a [`CaptureSink`]. Subsequent HTTP exchanges (token
    /// grant, refresh, profile fetch, [`Self::post`], [`Self::fetch`])
    /// forward request bodies as outbound and response bodies as
    /// inbound capture records. Calling again replaces the previous
    /// sink; passing `None` disables capture (use [`Self::clear_capture_sink`]).
    pub fn set_capture_sink(&self, sink: Arc<dyn CaptureSink>) {
        *self.inner.capture_sink.lock().expect("capture_sink mutex") = Some(sink);
    }

    /// Detach any previously attached [`CaptureSink`].
    pub fn clear_capture_sink(&self) {
        *self.inner.capture_sink.lock().expect("capture_sink mutex") = None;
    }

    /// Perform the OAuth2 password grant against the Keycloak token
    /// endpoint. On success, fetches `GET /api/profiles/me` and caches
    /// the result so [`Self::athlete_id`] is available without further
    /// I/O. If either step fails the whole call returns an error and no
    /// state is committed.
    pub async fn login(&self, username: &str, password: &str) -> Result<()> {
        let url = format!("{}{}", self.inner.config.auth_base, TOKEN_PATH);

        tracing::info!(
            target: "ranchero::relay",
            username,
            grant_type = "password",
            "relay.auth.token.requested",
        );

        let form_bytes = serde_urlencoded::to_string([
            ("client_id", CLIENT_ID),
            ("grant_type", "password"),
            ("username", username),
            ("password", password),
        ])
        .map_err(|e| Error::InvalidTokenResponse(e.to_string()))?
        .into_bytes();
        self.inner.record_outbound(&form_bytes);

        let resp = self
            .inner
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json") // STEP-12.14 §N3 — zwift.mjs:346
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform) // STEP-12.14 §C6
            .header("User-Agent", &self.inner.config.user_agent)
            .body(form_bytes)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        self.inner.record_inbound(&bytes);
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).to_string();
            return Err(match status.as_u16() {
                401 => Error::AuthFailedUnauthorized(body),
                403 => Error::AuthFailedForbidden(body),
                _ => Error::AuthFailedUnknown(body),
            });
        }
        let tokens: Tokens = serde_json::from_slice(&bytes)
            .map_err(|e| Error::InvalidTokenResponse(e.to_string()))?;
        let expires_in = tokens.expires_in;
        let refresh_expires_in = tokens.refresh_expires_in;

        tracing::info!(
            target: "ranchero::relay",
            expires_in_s = expires_in,
            refresh_expires_in_s = refresh_expires_in,
            "relay.auth.token.granted",
        );

        // Store the token temporarily so get_profile_me can call bearer().
        *self.inner.tokens.write().await = Some(tokens);

        // Eagerly fetch the profile. Roll back the token on failure so
        // callers never see a half-committed state.
        let profile = match self.get_profile_me().await {
            Ok(p) => p,
            Err(e) => {
                *self.inner.tokens.write().await = None;
                return Err(e);
            }
        };

        // Both steps succeeded — commit the refresh schedule and profile.
        Inner::schedule_refresh(self.inner.clone(), Duration::from_secs(expires_in / 2));
        *self.inner.profile.write().await = Some(profile);
        Ok(())
    }

    /// Fetch `GET /api/profiles/me` using the current bearer token and
    /// return the decoded [`Profile`]. Unlike [`Self::fetch`] this method
    /// does NOT retry on 401; all non-success statuses map directly to
    /// typed [`Error`] variants so callers can match exhaustively.
    pub async fn get_profile_me(&self) -> Result<Profile> {
        let bearer = self.bearer().await?;
        let url = format!("{}/api/profiles/me", self.inner.config.api_base);
        // GET requests have an empty body; record an empty payload so
        // a downstream replay sees the exchange as request → response.
        self.inner.record_outbound(&[]);
        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        self.inner.record_inbound(&bytes);
        match status.as_u16() {
            200 => match serde_json::from_slice::<Profile>(&bytes) {
                Ok(profile) => {
                    tracing::debug!(
                        target: "ranchero::relay",
                        athlete_id = profile.id,
                        "relay.auth.profile.ok",
                    );
                    Ok(profile)
                }
                Err(e) => {
                    tracing::warn!(
                        target: "ranchero::relay",
                        status = 200,
                        variant = "BadSchema",
                        "relay.auth.profile.failed",
                    );
                    Err(Error::AuthFailedBadSchema(e.to_string()))
                }
            },
            401 => {
                tracing::warn!(
                    target: "ranchero::relay",
                    status = 401,
                    variant = "Unauthorized",
                    "relay.auth.profile.failed",
                );
                Err(Error::AuthFailedUnauthorized(
                    String::from_utf8_lossy(&bytes).into_owned(),
                ))
            }
            403 => {
                tracing::warn!(
                    target: "ranchero::relay",
                    status = 403,
                    variant = "Forbidden",
                    "relay.auth.profile.failed",
                );
                Err(Error::AuthFailedForbidden(
                    String::from_utf8_lossy(&bytes).into_owned(),
                ))
            }
            other => {
                tracing::warn!(
                    target: "ranchero::relay",
                    status = other,
                    variant = "Unknown",
                    "relay.auth.profile.failed",
                );
                Err(Error::AuthFailedUnknown(format!(
                    "HTTP {}: {}",
                    status,
                    String::from_utf8_lossy(&bytes),
                )))
            }
        }
    }

    /// Return the authenticated athlete's Zwift profile ID. Populated by
    /// [`Self::login`]; returns [`Error::NotAuthenticated`] if `login` has
    /// not yet been called.
    pub async fn athlete_id(&self) -> Result<i64> {
        self.inner
            .profile
            .read()
            .await
            .as_ref()
            .map(|p| p.id)
            .ok_or(Error::NotAuthenticated)
    }

    /// Fetch the current `PlayerState` for `athlete_id` from
    /// `GET /relay/worlds/1/players/{id}` (protobuf-lite). Mirrors
    /// sauce4zwift's `getPlayerState` (`zwift.mjs:613`); a 404
    /// response — meaning the athlete is not currently in a game —
    /// returns `Ok(None)` rather than an error so the daemon's
    /// course-gate can branch cleanly.
    pub async fn get_player_state(
        &self,
        athlete_id: i64,
    ) -> Result<Option<zwift_proto::PlayerState>> {
        use prost::Message as _;

        let urn = format!("/relay/worlds/1/players/{athlete_id}");
        let resp = self.fetch_pb(&urn).await?;
        let status = resp.status();
        if status.as_u16() == 404 {
            return Ok(None);
        }
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            return Err(Error::Status {
                status: status.as_u16(),
                body: String::from_utf8_lossy(&bytes).into_owned(),
            });
        }
        let state = zwift_proto::PlayerState::decode(&bytes[..])
            .map_err(|e| Error::ProtobufDecode(e.to_string()))?;
        Ok(Some(state))
    }

    /// GET helper that mirrors [`Self::fetch`] but adds
    /// `Accept: application/x-protobuf-lite` so the server returns
    /// the protobuf-lite encoding sauce uses for `fetchPB` endpoints
    /// (`zwift.mjs:447-451`).
    async fn fetch_pb(&self, urn: &str) -> Result<HttpResponse> {
        let url = format!("{}{}", self.inner.config.api_base, urn);
        let bearer = self.bearer().await?;

        self.inner.record_outbound(&[]);
        tracing::debug!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            body_size = 0,
            "relay.auth.http.request",
        );
        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Accept", "application/x-protobuf-lite")
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        let status = resp.status();
        let resp_bytes = resp.bytes().await?;
        self.inner.record_inbound(&resp_bytes);
        tracing::debug!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            status = status.as_u16(),
            body_size = resp_bytes.len(),
            retried = false,
            "relay.auth.http.response",
        );
        if status != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(HttpResponse {
                status,
                body: resp_bytes.to_vec(),
            });
        }
        tracing::info!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            "relay.auth.http.retry",
        );
        self.refresh().await?;
        let bearer = self.bearer().await?;

        self.inner.record_outbound(&[]);
        let retry = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Accept", "application/x-protobuf-lite")
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        let retry_status = retry.status();
        let retry_bytes = retry.bytes().await?;
        self.inner.record_inbound(&retry_bytes);
        tracing::debug!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            status = retry_status.as_u16(),
            body_size = retry_bytes.len(),
            retried = true,
            "relay.auth.http.response",
        );
        Ok(HttpResponse {
            status: retry_status,
            body: retry_bytes.to_vec(),
        })
    }

    /// Use the current `refresh_token` to obtain a fresh `access_token`
    /// from the Keycloak token endpoint. Reschedules the next preemptive
    /// refresh on success.
    pub async fn refresh(&self) -> Result<()> {
        Inner::do_refresh(self.inner.clone()).await
    }

    /// Snapshot of the current tokens, if any. Returns `None` before
    /// the first successful `login()`.
    pub async fn tokens(&self) -> Option<Tokens> {
        self.inner.tokens.read().await.clone()
    }

    /// Return the current `access_token`. Errors with
    /// `NotAuthenticated` when no tokens are present.
    ///
    /// Note: this method does not itself trigger a refresh; the
    /// background scheduler handles preemptive refresh at half-life,
    /// and `fetch()` handles the 401 fallback.
    pub async fn bearer(&self) -> Result<String> {
        self.inner
            .tokens
            .read()
            .await
            .as_ref()
            .map(|t| t.access_token.clone())
            .ok_or(Error::NotAuthenticated)
    }

    /// POST `body` to `{api_base}{urn}` with the supplied
    /// `Content-Type`. Includes `Authorization: Bearer …`, `Source`,
    /// and `User-Agent` exactly like [`Self::fetch`]. On a 401 response,
    /// transparently triggers a token refresh and retries the request
    /// once. Used by `zwift-relay` for the protobuf POSTs to
    /// `/api/users/login` and `/relay/session/refresh`.
    pub async fn post(
        &self,
        urn: &str,
        content_type: &str,
        body: Vec<u8>,
    ) -> Result<HttpResponse> {
        let url = format!("{}{}", self.inner.config.api_base, urn);
        let bearer = self.bearer().await?;

        // STEP-12.14 §C8 — sauce appends `; version=2.0` to the
        // protobuf content-type (`zwift.mjs:445`). Normalise here so
        // callers that pass the bare constant get the correct wire value.
        let ct_owned;
        let content_type = if content_type == "application/x-protobuf-lite" {
            ct_owned = "application/x-protobuf-lite; version=2.0";
            ct_owned
        } else {
            content_type
        };

        // STEP-12.14 §N4 — sauce's fetchPB sets Accept: application/x-protobuf-lite
        let is_protobuf = content_type.starts_with("application/x-protobuf-lite");

        self.inner.record_outbound(&body);
        tracing::debug!(
            target: "ranchero::relay",
            method = "POST",
            urn,
            content_type,
            body_size = body.len(),
            "relay.auth.http.request",
        );
        let mut builder = self
            .inner
            .http
            .post(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform)
            .header("User-Agent", &self.inner.config.user_agent)
            .header("Content-Type", content_type);
        if is_protobuf {
            builder = builder.header("Accept", "application/x-protobuf-lite");
        }
        let resp = builder
            .body(body.clone())
            .send()
            .await?;
        let status = resp.status();
        let resp_bytes = resp.bytes().await?;
        self.inner.record_inbound(&resp_bytes);
        tracing::debug!(
            target: "ranchero::relay",
            method = "POST",
            urn,
            status = status.as_u16(),
            body_size = resp_bytes.len(),
            retried = false,
            "relay.auth.http.response",
        );
        if status != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(HttpResponse {
                status,
                body: resp_bytes.to_vec(),
            });
        }
        tracing::info!(
            target: "ranchero::relay",
            method = "POST",
            urn,
            "relay.auth.http.retry",
        );
        self.refresh().await?;
        let bearer = self.bearer().await?;

        self.inner.record_outbound(&body);
        tracing::debug!(
            target: "ranchero::relay",
            method = "POST",
            urn,
            content_type,
            body_size = body.len(),
            "relay.auth.http.request",
        );
        let mut retry_builder = self
            .inner
            .http
            .post(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform)
            .header("User-Agent", &self.inner.config.user_agent)
            .header("Content-Type", content_type);
        if is_protobuf {
            retry_builder = retry_builder.header("Accept", "application/x-protobuf-lite");
        }
        let retry = retry_builder.body(body).send().await?;
        let retry_status = retry.status();
        let retry_bytes = retry.bytes().await?;
        self.inner.record_inbound(&retry_bytes);
        tracing::debug!(
            target: "ranchero::relay",
            method = "POST",
            urn,
            status = retry_status.as_u16(),
            body_size = retry_bytes.len(),
            retried = true,
            "relay.auth.http.response",
        );
        Ok(HttpResponse {
            status: retry_status,
            body: retry_bytes.to_vec(),
        })
    }

    /// Issue a GET against the API host with `Authorization: Bearer …`,
    /// `Source`, and `User-Agent` set. On a 401 response, transparently
    /// trigger a token refresh and retry the request once.
    pub async fn fetch(&self, urn: &str) -> Result<HttpResponse> {
        let url = format!("{}{}", self.inner.config.api_base, urn);
        let bearer = self.bearer().await?;

        self.inner.record_outbound(&[]);
        tracing::debug!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            body_size = 0,
            "relay.auth.http.request",
        );
        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        let status = resp.status();
        let resp_bytes = resp.bytes().await?;
        self.inner.record_inbound(&resp_bytes);
        tracing::debug!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            status = status.as_u16(),
            body_size = resp_bytes.len(),
            retried = false,
            "relay.auth.http.response",
        );
        if status != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(HttpResponse {
                status,
                body: resp_bytes.to_vec(),
            });
        }
        tracing::info!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            "relay.auth.http.retry",
        );
        self.refresh().await?;
        let bearer = self.bearer().await?;

        self.inner.record_outbound(&[]);
        tracing::debug!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            body_size = 0,
            "relay.auth.http.request",
        );
        let retry = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("Platform", &self.inner.config.platform)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        let retry_status = retry.status();
        let retry_bytes = retry.bytes().await?;
        self.inner.record_inbound(&retry_bytes);
        tracing::debug!(
            target: "ranchero::relay",
            method = "GET",
            urn,
            status = retry_status.as_u16(),
            body_size = retry_bytes.len(),
            retried = true,
            "relay.auth.http.response",
        );
        Ok(HttpResponse {
            status: retry_status,
            body: retry_bytes.to_vec(),
        })
    }
}

impl Inner {
    /// Replace any in-flight scheduled refresh with a fresh one that
    /// fires after `delay`. Aborting the previous handle is safe even
    /// when called from inside that very task: `JoinHandle::abort()`
    /// only sets a cancellation flag, checked at the next await point,
    /// which the calling task no longer reaches.
    fn schedule_refresh(self: Arc<Self>, delay: Duration) {
        let mut slot = self.refresh_task.lock().expect("refresh_task mutex");
        if let Some(prev) = slot.take() {
            prev.abort();
        }
        let inner = self.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Err(e) = Inner::do_refresh(inner).await {
                tracing::warn!(error = %e, "scheduled token refresh failed");
            }
        });
        *slot = Some(handle);
    }

    async fn do_refresh(self: Arc<Self>) -> Result<()> {
        let refresh_token = {
            let guard = self.tokens.read().await;
            guard
                .as_ref()
                .ok_or(Error::NotAuthenticated)?
                .refresh_token
                .clone()
        };
        let url = format!("{}{}", self.config.auth_base, TOKEN_PATH);

        let form_bytes = serde_urlencoded::to_string([
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
        ])
        .map_err(|e| Error::InvalidTokenResponse(e.to_string()))?
        .into_bytes();
        self.record_outbound(&form_bytes);

        let resp = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .header("Accept", "application/json") // STEP-12.14 §N3 — zwift.mjs:346
            .header("Source", &self.config.source)
            .header("Platform", &self.config.platform) // STEP-12.14 §C6
            .header("User-Agent", &self.config.user_agent)
            .body(form_bytes)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        self.record_inbound(&bytes);
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).to_string();
            return Err(Error::RefreshFailed(body));
        }
        let tokens: Tokens = serde_json::from_slice(&bytes)
            .map_err(|e| Error::InvalidTokenResponse(e.to_string()))?;
        let expires_in = tokens.expires_in;
        let next_refresh_in = expires_in / 2;
        *self.tokens.write().await = Some(tokens);
        Inner::schedule_refresh(self, Duration::from_secs(next_refresh_in));
        tracing::info!(
            target: "ranchero::relay",
            expires_in_s = expires_in,
            next_refresh_in_s = next_refresh_in,
            "relay.auth.refresh.completed",
        );
        Ok(())
    }
}
