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
subcommand below; the daemon will pick it up in STEP 09).

### Public API (`crates/zwift-api/src/lib.rs`)

Constants (override-points are flagged):

| Const | Value | Override |
|---|---|---|
| `DEFAULT_AUTH_HOST` | `"secure.zwift.com"` | `Config::auth_base` |
| `DEFAULT_API_HOST` | `"us-or-rly101.zwift.com"` | `Config::api_base` |
| `CLIENT_ID` | `"Zwift Game Client"` (literal space) | — |
| `DEFAULT_SOURCE` | `"Game Client"` (game-client mimicry; see §3.3) | `Config::source` |
| `DEFAULT_USER_AGENT` | `"CNL/4.2.0"` (game-client mimicry) | `Config::user_agent` |
| `TOKEN_PATH` | `"/auth/realms/zwift/protocol/openid-connect/token"` | — |

Types:

- `Config { auth_base, api_base, source, user_agent }` — `auth_base` /
  `api_base` are full URLs **with scheme** (e.g.
  `"https://secure.zwift.com"`), not bare hostnames. This is a
  deliberate departure from sauce's `host` + `scheme` split: full URLs
  let `wiremock::MockServer::uri()` plug in directly without test
  scaffolding, and force production callers to be explicit about HTTPS.
- `Tokens { access_token, refresh_token, expires_in,
  refresh_expires_in, token_type }` — `serde::Deserialize` directly
  from a Keycloak token response. Both `expires_in` fields are seconds.
- `Error` — `Http` (transport), `InvalidTokenResponse` (JSON parse),
  `AuthFailed` (login non-2xx), `RefreshFailed` (refresh non-2xx),
  `NotAuthenticated` (no tokens stored), `Status { status, body }`
  (catch-all for non-2xx on `fetch()`).
- `ZwiftAuth { Arc<Inner> }` — the public handle. Cloneable so
  multiple tasks (background refresher + future main/monitor pairs)
  share the same token store.

`ZwiftAuth` methods:

- `new(Config) -> Self` / `with_client(reqwest::Client, Config) -> Self`
  — the `with_client` form lets callers share a connection pool
  (anticipating two concurrent `ZwiftAuth` instances in STEP 09 for the
  main + monitor accounts).
- `async login(username, password) -> Result<()>` — POSTs the
  password-grant form, parses tokens, schedules the first preemptive
  refresh.
- `async refresh() -> Result<()>` — POSTs the refresh-token form,
  rotates tokens, reschedules.
- `async tokens() -> Option<Tokens>` — snapshot of the stored tokens.
- `async bearer() -> Result<String>` — current access token, or
  `NotAuthenticated`. Does *not* refresh on its own; the background
  scheduler handles half-life refresh and `fetch()` handles the 401
  fallback.
- `async fetch(urn) -> Result<reqwest::Response>` — GET against
  `{api_base}{urn}` with `Authorization: Bearer …`, `Source`,
  `User-Agent`. On 401 the response is dropped, `refresh()` runs
  inline, and the request is retried once with the new bearer.

### Background refresh scheduler

`Inner::schedule_refresh(self: Arc<Self>, delay: Duration)`:

1. Locks the `refresh_task: Mutex<Option<JoinHandle<()>>>` slot
   (`std::sync::Mutex` — critical section is just `take`/`replace`,
   never crosses an `.await`).
2. `abort()`s any previous handle. Safe even when called from inside
   the very task being aborted: `JoinHandle::abort()` only sets a
   cancellation flag, checked at the next await point, which the
   calling task no longer reaches before exiting.
3. Spawns a new task that does `tokio::time::sleep(delay)` then
   `Inner::do_refresh`. `do_refresh` reschedules on success and logs
   via `tracing::warn` on failure — there's no caller to surface
   failures to, and a 401 on the next `fetch()` will trigger a fresh
   inline refresh anyway.

### `auth-check` CLI subcommand

Added during STEP 07 as a no-network pre-flight diagnostic; kept
permanently rather than thrown away. See `src/cli.rs::print_auth_check`.

- Loads config + credentials the same way `start` will, constructs a
  `ZwiftAuth` (no `.login()` call), and prints the literal HTTP
  request `login()` would issue — form body (with the password slot
  rendered as `[redacted]`), Content-Type, fixed headers, the example
  authed call shape.
- Reads `cfg.source` / `cfg.user_agent` (not the constants) so any
  `Config` override flows through to the diagnostic.
- Useful as a gate before `start` — Zwift will lock the account on a
  few bad-password tries, so confirming the wiring without burning
  Keycloak attempts has real value.

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
| `refresh_failure_surfaces_error` | Refresh non-2xx → `Error::RefreshFailed` / `Error::Status` |
| `login_failure_401_surfaces_auth_error` | Login 401 → `Error::AuthFailed` / `Error::Status`, no tokens stored |
| `bearer_without_login_returns_not_authenticated` | `bearer()` before `login()` → `Error::NotAuthenticated` |
| `preemptive_refresh_fires_at_half_expires_in` | Background scheduler rotates the access token at `expires_in / 2` |

Plus root-crate parser tests in `tests/cli_args.rs`:

- `parses_auth_check_subcommand`
- `dispatch_auth_check_stub`

## Design decisions worth remembering

- **Full URL hosts in `Config`.** `auth_base` and `api_base` are full
  URLs with scheme, not `host` + `scheme` pairs. Trade-off discussed
  above; primarily about wiremock ergonomics.
- **No tokio re-export.** Callers that want to construct a
  `reqwest::Client` themselves use their own dependency on `reqwest`;
  `with_client` accepts any `reqwest::Client`. We don't pin a reqwest
  version through our public API.
- **`thiserror` for `Error`.** Pulled `thiserror = "1"` in (the
  workspace didn't have it before). Trivially replaceable with
  hand-rolled `Display` if the dep ever becomes inconvenient.
- **Background-task lifecycle.** The scheduled refresh task holds an
  `Arc<Inner>` to keep the auth state alive while it sleeps. When
  `ZwiftAuth` is dropped, the task survives until its sleep wakes (or
  until the runtime is shut down). Acceptable for the daemon — the
  single `ZwiftAuth` lives as long as the process. Worth revisiting
  if we ever construct/drop `ZwiftAuth` instances dynamically.
- **`Source: Game Client` default.** Honest identification (e.g.
  `"Ranchero"`) is one `Config::source = …` away, but the conservative
  default mimics a real Zwift client because Zwift's API is *suspected*
  to inspect this header; we won't know until STEP 09 hits real
  servers. See ARCHITECTURE-AND-RUST-SPEC.md §3.3.

## Known follow-ups

Tracked in `docs/plans/STEP-20-additional-considerations.md`:

- **§20.1** — the `preemptive_refresh_fires_at_half_expires_in` test
  uses a 2 s real-time wait instead of `tokio::time::pause()` +
  `advance()`. The virtual-time approach deadlocks under
  `current_thread` because reqwest IO needs a turn of the reactor that
  the test loop never lets through. Status quo until the cumulative
  real-time test budget gets uncomfortable.

Not yet implemented (deferred to later steps that need them):

- Typed wrappers for specific REST endpoints (`/api/profiles/me`,
  `/api/profiles/{id}`, `/relay/session/refresh`, etc.). Today
  `fetch()` returns a raw `reqwest::Response`; STEP 09+ will add
  typed methods as each endpoint is needed.
- Connection-pool sharing between two `ZwiftAuth` instances (the
  `with_client` constructor exists for this; no caller uses it yet).
- CLI/config exposure of `Config::source` / `Config::user_agent`
  overrides. The override hook is in code; no configuration surface
  yet.
