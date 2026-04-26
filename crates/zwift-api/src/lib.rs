// SPDX-License-Identifier: AGPL-3.0-only
//
// Zwift OAuth2 (Keycloak password-grant) and authenticated REST client.
//
// Ported from sauce4zwift `src/zwift.mjs` (`ZwiftAPI` class, lines
// ~327-500). See docs/plans/STEP-07-auth-and-rest.md and
// docs/ARCHITECTURE-AND-RUST-SPEC.md §3 / §7.5 for the contract.
//
// This file currently exposes the public API surface as stubs so the
// `tests/auth.rs` suite compiles. Behavior is implemented in a later
// pass; until then every method panics via `unimplemented!()` and the
// tests will fail loudly. This is the TDD scaffold, not the
// implementation.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Default Keycloak host used for token issuance and refresh.
pub const DEFAULT_AUTH_HOST: &str = "secure.zwift.com";

/// Default Zwift game-API host used for authenticated REST calls.
pub const DEFAULT_API_HOST: &str = "us-or-rly101.zwift.com";

/// Keycloak `client_id`. The literal space is intentional and is what
/// Zwift's first-party game client sends; serde_urlencoded will encode
/// it as `Zwift+Game+Client` in the form body.
pub const CLIENT_ID: &str = "Zwift Game Client";

/// `Source` header value sent on every authenticated REST call.
pub const DEFAULT_SOURCE: &str = "Sauce for Zwift";

/// `User-Agent` header value sent on every authenticated REST call.
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

    #[error("authentication failed: {0}")]
    AuthFailed(String),

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
/// (e.g. main + monitor accounts elsewhere; a background refresh task
/// here) share the same token store.
#[derive(Clone)]
pub struct ZwiftAuth {
    inner: Arc<Inner>,
}

struct Inner {
    #[allow(dead_code)]
    http: reqwest::Client,
    #[allow(dead_code)]
    config: Config,
    #[allow(dead_code)]
    tokens: RwLock<Option<Tokens>>,
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
            }),
        }
    }

    /// Perform the OAuth2 password grant against the Keycloak token
    /// endpoint. On success, stores the resulting tokens and schedules
    /// a background refresh at `expires_in / 2` seconds from now.
    pub async fn login(&self, _username: &str, _password: &str) -> Result<()> {
        unimplemented!("STEP-07: implement Keycloak password-grant login")
    }

    /// Use the current `refresh_token` to obtain a fresh `access_token`
    /// from the Keycloak token endpoint. Reschedules the next preemptive
    /// refresh on success.
    pub async fn refresh(&self) -> Result<()> {
        unimplemented!("STEP-07: implement Keycloak refresh_token grant")
    }

    /// Snapshot of the current tokens, if any. Returns `None` before
    /// the first successful `login()`.
    pub async fn tokens(&self) -> Option<Tokens> {
        self.inner.tokens.read().await.clone()
    }

    /// Return the current `access_token`, refreshing if it has aged
    /// past 50% of its `expires_in`. Errors with `NotAuthenticated`
    /// when no tokens are present.
    pub async fn bearer(&self) -> Result<String> {
        unimplemented!("STEP-07: return access_token, refreshing at half-life if needed")
    }

    /// Issue a GET against the API host with `Authorization: Bearer …`,
    /// `Source`, and `User-Agent` set. On a 401 response, transparently
    /// trigger a token refresh and retry the request once.
    pub async fn fetch(&self, _urn: &str) -> Result<reqwest::Response> {
        unimplemented!("STEP-07: implement authed fetch with 401-refresh-retry")
    }
}
