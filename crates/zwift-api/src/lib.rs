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
/// Same rationale as [`DEFAULT_SOURCE`]: mimics a real Zwift game
/// client. Override via [`Config::user_agent`] for honest
/// self-identification when the API is known to tolerate it.
pub const DEFAULT_USER_AGENT: &str = "CNL/4.2.0";

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            auth_base: format!("https://{DEFAULT_AUTH_HOST}"),
            api_base: format!("https://{DEFAULT_API_HOST}"),
            source: DEFAULT_SOURCE.to_string(),
            user_agent: DEFAULT_USER_AGENT.to_string(),
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
            }),
        }
    }

    /// Perform the OAuth2 password grant against the Keycloak token
    /// endpoint. On success, fetches `GET /api/profiles/me` and caches
    /// the result so [`Self::athlete_id`] is available without further
    /// I/O. If either step fails the whole call returns an error and no
    /// state is committed.
    pub async fn login(&self, username: &str, password: &str) -> Result<()> {
        let url = format!("{}{}", self.inner.config.auth_base, TOKEN_PATH);
        let resp = self
            .inner
            .http
            .post(&url)
            .form(&[
                ("client_id", CLIENT_ID),
                ("grant_type", "password"),
                ("username", username),
                ("password", password),
            ])
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
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
        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        match status.as_u16() {
            200 => serde_json::from_slice::<Profile>(&bytes)
                .map_err(|e| Error::AuthFailedBadSchema(e.to_string())),
            401 => Err(Error::AuthFailedUnauthorized(
                String::from_utf8_lossy(&bytes).into_owned(),
            )),
            403 => Err(Error::AuthFailedForbidden(
                String::from_utf8_lossy(&bytes).into_owned(),
            )),
            _ => Err(Error::AuthFailedUnknown(format!(
                "HTTP {}: {}",
                status,
                String::from_utf8_lossy(&bytes),
            ))),
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
    ) -> Result<reqwest::Response> {
        let url = format!("{}{}", self.inner.config.api_base, urn);
        let bearer = self.bearer().await?;
        let resp = self
            .inner
            .http
            .post(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("User-Agent", &self.inner.config.user_agent)
            .header("Content-Type", content_type)
            .body(body.clone())
            .send()
            .await?;
        if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        drop(resp);
        self.refresh().await?;
        let bearer = self.bearer().await?;
        let retry = self
            .inner
            .http
            .post(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("User-Agent", &self.inner.config.user_agent)
            .header("Content-Type", content_type)
            .body(body)
            .send()
            .await?;
        Ok(retry)
    }

    /// Issue a GET against the API host with `Authorization: Bearer …`,
    /// `Source`, and `User-Agent` set. On a 401 response, transparently
    /// trigger a token refresh and retry the request once.
    pub async fn fetch(&self, urn: &str) -> Result<reqwest::Response> {
        let url = format!("{}{}", self.inner.config.api_base, urn);
        let bearer = self.bearer().await?;
        let resp = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        if resp.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(resp);
        }
        // Drop the 401 response (and its body) before refreshing so we
        // don't tie up the connection during the token round-trip.
        drop(resp);
        self.refresh().await?;
        let bearer = self.bearer().await?;
        let retry = self
            .inner
            .http
            .get(&url)
            .bearer_auth(&bearer)
            .header("Source", &self.inner.config.source)
            .header("User-Agent", &self.inner.config.user_agent)
            .send()
            .await?;
        Ok(retry)
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
        let resp = self
            .http
            .post(&url)
            .form(&[
                ("client_id", CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token.as_str()),
            ])
            .send()
            .await?;
        let status = resp.status();
        let bytes = resp.bytes().await?;
        if !status.is_success() {
            let body = String::from_utf8_lossy(&bytes).to_string();
            return Err(Error::RefreshFailed(body));
        }
        let tokens: Tokens = serde_json::from_slice(&bytes)
            .map_err(|e| Error::InvalidTokenResponse(e.to_string()))?;
        let expires_in = tokens.expires_in;
        *self.tokens.write().await = Some(tokens);
        Inner::schedule_refresh(self, Duration::from_secs(expires_in / 2));
        Ok(())
    }
}
