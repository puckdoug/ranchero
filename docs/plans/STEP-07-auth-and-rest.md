# Step 07 — `zwift-api`: OAuth2 + REST client (stub)

## Goal

Port `ZwiftAPI` from `sauce4zwift/src/zwift.mjs` (lines 340-500 roughly)
into a `zwift-api` crate: Keycloak password-grant login, pre-emptive
refresh at 50% of `expires_in`, and a `fetch()` helper that retries on
401 with a refresh.

## Sketch

- `ZwiftAuth` owns `reqwest::Client`, tokens behind `RwLock<Option<Tokens>>`.
- `client_id = "Zwift Game Client"` (literal space; URL-encodes to `Zwift+Game+Client`).
- Fixed headers on all authenticated calls: `Authorization: Bearer …`,
  `Source: Sauce for Zwift`, `User-Agent: CNL/4.2.0 (…)`.
- Background refresh task driven by `tokio::time::sleep_until`.
- REST endpoints (as we need them): `/api/profiles/me`,
  `/api/profiles/{id}`, `/relay/session/refresh`, etc.

## Tests-first outline

- Mock HTTP server (e.g. `wiremock`) to exercise:
  - Successful login → tokens parsed, refresh scheduled at `expires_in / 2`.
  - 401 on authed call → inline refresh triggered → retry → success.
  - Refresh failure → error surfaced; subsequent call re-runs full login.
  - Request shape (form body, headers) asserted exactly.
- No live calls to Zwift in CI.

To be fully elaborated when we start work on this step.
