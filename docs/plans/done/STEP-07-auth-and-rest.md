# Step 07 — `zwift-api`: OAuth2 + REST client

**Status:** complete (2026-04-26).

## Goal

Port `ZwiftAPI` from `sauce4zwift/src/zwift.mjs` (lines 327-500
roughly) into a `zwift-api` crate: Keycloak password-grant login,
pre-emptive refresh at 50% of `expires_in`, and a `fetch()` helper
that retries on 401 with a refresh.

## What was built

### Crate layout

```
crates/zwift-api/
├── Cargo.toml          — workspace member, AGPL-3.0-only
├── src/lib.rs          — public API + implementation (single file)
└── tests/auth.rs       — 8 wiremock-driven integration tests
```

Workspace `Cargo.toml`'s glob (`members = ["crates/*"]`) picks the
crate up automatically. The root `ranchero` crate depends on
`zwift-api` by path (used today only by the `auth-check` CLI
subcommand described below; the daemon will adopt it in STEP 09).

### Public API (`crates/zwift-api/src/lib.rs`)

Constants (override-points are flagged):

| Const | Value | Override |
|---|---|---|
| `DEFAULT_AUTH_HOST` | `"secure.zwift.com"` | `Config::auth_base` |
| `DEFAULT_API_HOST` | `"us-or-rly101.zwift.com"` | `Config::api_base` |
| `CLIENT_ID` | `"Zwift Game Client"` (literal space) | not applicable |
| `DEFAULT_SOURCE` | `"Game Client"` (game-client mimicry; see §3.3) | `Config::source` |
| `DEFAULT_USER_AGENT` | `"CNL/4.2.0"` (game-client mimicry) | `Config::user_agent` |
| `TOKEN_PATH` | `"/auth/realms/zwift/protocol/openid-connect/token"` | not applicable |

Types:

- `Config { auth_base, api_base, source, user_agent }`: `auth_base`
  and `api_base` are full URLs **with scheme** (for example,
  `"https://secure.zwift.com"`), not bare hostnames. This is a
  deliberate departure from sauce's `host` plus `scheme` split: full
  URLs allow `wiremock::MockServer::uri()` to plug in directly without
  test scaffolding, and force production callers to be explicit about
  HTTPS.
- `Tokens { access_token, refresh_token, expires_in,
  refresh_expires_in, token_type }`: `serde::Deserialize` directly
  from a Keycloak token response. Both `expires_in` fields are in
  seconds.
- `Error`: `Http` (transport), `InvalidTokenResponse` (JSON parse),
  `AuthFailed` (login non-2xx), `RefreshFailed` (refresh non-2xx),
  `NotAuthenticated` (no tokens stored), `Status { status, body }`
  (catch-all for non-2xx responses on `fetch()`).
- `ZwiftAuth { Arc<Inner> }`: the public handle. Cloneable so that
  multiple tasks (the background refresher and future main and monitor
  pairs) share the same token store.

`ZwiftAuth` methods:

- `new(Config) -> Self` and `with_client(reqwest::Client, Config) -> Self`:
  the `with_client` form allows callers to share a connection pool
  (anticipating two concurrent `ZwiftAuth` instances in STEP 09 for the
  main and monitor accounts).
- `async login(username, password) -> Result<()>`: POSTs the
  password-grant form, parses tokens, and schedules the first
  preemptive refresh.
- `async refresh() -> Result<()>`: POSTs the refresh-token form,
  rotates tokens, and reschedules.
- `async tokens() -> Option<Tokens>`: snapshot of the stored tokens.
- `async bearer() -> Result<String>`: current access token, or
  `NotAuthenticated`. Does *not* refresh on its own; the background
  scheduler handles the half-life refresh and `fetch()` handles the
  401 fallback.
- `async fetch(urn) -> Result<reqwest::Response>`: GET against
  `{api_base}{urn}` with `Authorization: Bearer …`, `Source`, and
  `User-Agent`. On 401 the response is dropped, `refresh()` runs
  inline, and the request is retried once with the new bearer.

### Background refresh scheduler

`Inner::schedule_refresh(self: Arc<Self>, delay: Duration)`:

1. Locks the `refresh_task: Mutex<Option<JoinHandle<()>>>` slot
   (`std::sync::Mutex`; the critical section is only `take` and
   `replace`, and never crosses an `.await`).
2. Calls `abort()` on any previous handle. This is safe even when
   called from inside the task being aborted: `JoinHandle::abort()`
   only sets a cancellation flag, which is checked at the next await
   point, and the calling task no longer reaches an await point before
   exiting.
3. Spawns a new task that performs `tokio::time::sleep(delay)` and
   then `Inner::do_refresh`. `do_refresh` reschedules on success and
   logs via `tracing::warn` on failure; there is no caller to surface
   failures to, and a 401 on the next `fetch()` will trigger a fresh
   inline refresh in any case.

### `auth-check` CLI subcommand

Added during STEP 07 as a no-network pre-flight diagnostic; retained
permanently rather than discarded. See `src/cli.rs::print_auth_check`.

- Loads configuration and credentials in the same way `start` does,
  constructs a `ZwiftAuth` (without calling `.login()`), and prints
  the literal HTTP request that `login()` would issue: the form body
  (with the password slot rendered as `[redacted]`), the Content-Type,
  the fixed headers, and the example authenticated call shape.
- Reads `cfg.source` and `cfg.user_agent` (rather than the constants)
  so that any `Config` override flows through to the diagnostic.
- Useful as a gate before `start`: Zwift will lock the account after
  a few bad-password attempts, so confirming the wiring without
  consuming Keycloak attempts has real value.

## Tests

All tests are in `crates/zwift-api/tests/auth.rs` and run against a
`wiremock::MockServer`. Both `auth_base` and `api_base` are pointed at
the same mock server (routing happens by URN); nothing in CI ever
reaches a real Zwift host.

| Test | Contract |
|---|---|
| `login_success_parses_tokens_and_exposes_bearer` | Successful login → `Tokens` parsed → `bearer()` returns the access token |
| `login_sends_password_grant_form_body_with_literal_client_id` | Form body shape: exact field set, `client_id=Zwift+Game+Client` (literal space → `+`) |
| `authed_fetch_sends_bearer_source_and_user_agent_headers` | Authed GET carries `Authorization: Bearer …`, `Source`, `User-Agent` |
| `authed_fetch_401_triggers_inline_refresh_and_retries` | 401 → inline refresh → retry with new bearer → 200 |
| `refresh_failure_surfaces_error` | Refresh non-2xx → `Error::RefreshFailed` or `Error::Status` |
| `login_failure_401_surfaces_auth_error` | Login 401 → `Error::AuthFailed` or `Error::Status`, with no tokens stored |
| `bearer_without_login_returns_not_authenticated` | `bearer()` before `login()` → `Error::NotAuthenticated` |
| `preemptive_refresh_fires_at_half_expires_in` | Background scheduler rotates the access token at `expires_in / 2` |

Plus root-crate parser tests in `tests/cli_args.rs`:

- `parses_auth_check_subcommand`
- `dispatch_auth_check_stub`

## Design decisions worth remembering

- **Full URL hosts in `Config`.** `auth_base` and `api_base` are full
  URLs with scheme, not `host` plus `scheme` pairs. The trade-off is
  discussed above and is primarily about wiremock ergonomics.
- **No tokio re-export.** Callers that wish to construct a
  `reqwest::Client` themselves use their own dependency on `reqwest`;
  `with_client` accepts any `reqwest::Client`. The crate does not pin
  a reqwest version through its public API.
- **`thiserror` for `Error`.** Added `thiserror = "1"` (the workspace
  did not have it before). Trivially replaceable with a custom
  `Display` implementation if the dependency ever becomes inconvenient.
- **Background-task lifecycle.** The scheduled refresh task holds an
  `Arc<Inner>` to keep the auth state alive while it sleeps. When
  `ZwiftAuth` is dropped, the task survives until its sleep wakes (or
  until the runtime is shut down). This is acceptable for the daemon:
  the single `ZwiftAuth` instance lives as long as the process. Worth
  revisiting if `ZwiftAuth` instances are ever constructed and dropped
  dynamically.
- **`Source: Game Client` default.** Explicit identification (for
  example, `"Ranchero"`) is one `Config::source = …` away, but the
  conservative default mimics a real Zwift client because Zwift's API
  is *suspected* to inspect this header; this will not be known until
  STEP 09 reaches real servers. See ARCHITECTURE-AND-RUST-SPEC.md §3.3.

## Known follow-ups

Tracked in `docs/plans/STEP-20-additional-considerations.md`:

- **§20.1**: the `preemptive_refresh_fires_at_half_expires_in` test
  uses a 2 s real-time wait instead of `tokio::time::pause()` plus
  `advance()`. The virtual-time approach deadlocks under
  `current_thread` because reqwest IO needs a turn of the reactor that
  the test loop never permits. The current approach remains in place
  until the cumulative real-time test budget becomes uncomfortable.

Not yet implemented (deferred to later steps that need them):

- Typed wrappers for specific REST endpoints (`/api/profiles/me`,
  `/api/profiles/{id}`, `/relay/session/refresh`, and similar). At
  present, `fetch()` returns a raw `reqwest::Response`; STEP 09 and
  later will add typed methods as each endpoint is needed.
- Connection-pool sharing between two `ZwiftAuth` instances (the
  `with_client` constructor exists for this purpose; no caller uses it
  yet).
- CLI and configuration exposure of `Config::source` and
  `Config::user_agent` overrides. The override hook is in code; no
  configuration surface exists yet.
