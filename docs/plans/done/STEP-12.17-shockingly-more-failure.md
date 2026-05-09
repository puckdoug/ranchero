# STEP-12.17 — Profile fetch returns 200 but body is not JSON

**Status:** complete (2026-05-09). Phase 1 implemented and all tests
pass (`cargo test --workspace`). Two adjacent diagnostic improvements
deferred to STEP-20 §20.16.

After STEP-12.16 closed (course gate, mid-session reconnect,
handshake budget), the daemon was driven against the live Zwift API
again with a real account. Login succeeded; the daemon exited inside
the eager `get_profile_me()` call that runs from `ZwiftAuth::login`.

## 1. Observed failure

```
2026-05-09T11:58:11.792810Z  INFO ranchero::daemon::runtime: ranchero started pid=93936
ranchero started (pid 93936)
2026-05-09T11:58:11.796721Z  INFO ranchero::relay: relay.capture.opened
2026-05-09T11:58:11.796813Z  INFO ranchero::relay: relay.auth.token.requested username="doug+sauce@mhost.com" grant_type="password"
2026-05-09T11:58:14.182513Z  INFO ranchero::relay: relay.auth.token.granted expires_in_s=21600 refresh_expires_in_s=691200
2026-05-09T11:58:14.874647Z  WARN ranchero::relay: relay.auth.profile.failed status=200 variant="BadSchema"
2026-05-09T11:58:14.884804Z  INFO ranchero::relay: relay.capture.writer.closed total_records=0 total_bytes=0
ranchero stopped
2026-05-09T11:58:14.885248Z ERROR ranchero::relay: relay.start.failed error=auth: authentication failed: unexpected response shape: expected value at line 1 column 1
2026-05-09T11:58:14.885441Z  INFO ranchero::daemon::runtime: ranchero stopped
error: I/O error: auth: authentication failed: unexpected response shape: expected value at line 1 column 1
```

The token grant succeeds. `GET /api/profiles/me` returns HTTP 200
but `serde_json::from_slice::<Profile>` fails with `expected value at
line 1 column 1`. That message means the body's first byte is not a
JSON value start (`{`, `[`, `"`, `-`, digit, `t`, `f`, `n`); typical
causes are an empty body, an HTML error page, or a binary protobuf
payload returned in lieu of JSON.

The error path at `crates/zwift-api/src/lib.rs:380-397` maps this to
`Error::AuthFailedBadSchema` with the serde error string and emits
`relay.auth.profile.failed status=200 variant="BadSchema"`, exactly
as observed.

## 2. Root cause: missing `Accept: application/json`

Sauce4zwift's `fetch` helper (`zwift.mjs:424-479`) only adds an
`Accept` header when `options.accept` is set, and `fetchJSON`
(`zwift.mjs:523-529`) always sets it:

```js
async fetchJSON(urn, options, headers) {
    const r = await this.fetch(urn, {accept: 'json', ...options}, headers);
    if (r.status === 204) {
        return;
    }
    return await r.json();
}
```

`getProfile('me')` (`zwift.mjs:541-553`) calls `fetchJSON`, so every
profile request goes out with `Accept: application/json`. Sauce's
login (`zwift.mjs:362`) calls `getProfile('me')` immediately after
the token grant — exactly the path ranchero is mimicking.

Ranchero's `get_profile_me` at `crates/zwift-api/src/lib.rs:360-435`
sends `Authorization`, `Source`, `Platform`, and `User-Agent`, but
**no `Accept` header**:

```rust
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
```

`reqwest` does not synthesise a default `Accept`. Without it, Zwift's
API gateway is free to pick any representation it likes. The body
returned for this account at this moment was not JSON — most likely
`application/x-protobuf-lite` (the same endpoint serves protobuf
when asked, e.g. via the `PlayerProfiles` proto) — and the JSON
decoder correctly rejected it at byte zero.

The token-endpoint call in `login()` already sets
`Accept: application/json` at line 307 (added in STEP-12.14 §N3),
which is why the token grant works. The fix was applied to one call
site and missed for the profile call right next to it.

## 3. Audit of every authenticated GET in `crates/zwift-api/src/lib.rs`

| Line | Call site                       | Expected body type | `Accept` sent today                | Sauce reference          |
| ---: | ------------------------------- | ------------------ | ---------------------------------- | ------------------------ |
|  307 | `login()` token POST            | JSON               | `application/json`                 | zwift.mjs:346 (matches)  |
|  372 | **`get_profile_me()` GET**      | **JSON**           | **none — defect**                  | zwift.mjs:541 → fetchJSON|
|  501 | protobuf endpoint               | protobuf           | `application/x-protobuf-lite`      | zwift.mjs:531 fetchPB    |
|  540 | protobuf endpoint               | protobuf           | `application/x-protobuf-lite`      | zwift.mjs:531 fetchPB    |
|  641 | generic `fetch()` PB path       | protobuf           | `application/x-protobuf-lite`      | zwift.mjs:531 fetchPB    |
|  693 | generic `fetch()` PB retry path | protobuf           | `application/x-protobuf-lite`      | zwift.mjs:531 fetchPB    |
|  735 | generic GET (HttpResponse)      | caller-defined     | none                               | sauce only adds when set |
|  780 | generic GET retry path          | caller-defined     | none                               | sauce only adds when set |
|  834 | `post_empty()` (logout / leave) | (no body)          | none                               | sauce only adds when set |
|  904 | another JSON fetch              | JSON               | `application/json`                 | zwift.mjs:523 fetchJSON  |

Every path sauce sends `Accept: application/x-protobuf-lite` for is
covered. Every JSON path sauce sends `Accept: application/json` for
is covered **except** `get_profile_me`. The generic GET helpers at
735/780 and `post_empty` correctly omit the header (sauce omits it
when the caller does not request a specific representation).

## 4. Why STEP-12.14 §N3 missed this

§N3's audit was framed around "the token POST sometimes returns HTML
without `Accept: application/json`". That is the origin server
behaviour the §N3 fix targeted, and the fix was applied at line 307.
The eagerly-following profile GET sits in the same `login()` flow
but was treated as a separate concern at the time and not audited.

The lesson for future header audits: search for *every* call site in
`zwift-api/src/lib.rs` that returns a typed JSON struct via
`serde_json::from_slice`, not only the path that originally
manifested the symptom. The current grep set —

```
grep -n 'serde_json::from_slice\|from_str' crates/zwift-api/src/lib.rs
```

— enumerates the JSON decode points that must each be paired with
an `Accept: application/json` request header.

## 5. Implementation plan

Single phase. The fix is a one-line header addition driven by a
TDD pair, followed by a regression test that reproduces the
observed failure mode (200 + non-JSON body) so a future regression
in header handling fails loudly rather than silently.

### Phase 1 — Fix `get_profile_me` to send `Accept: application/json` ✓

#### 1a — Failing tests

Add to `crates/zwift-api/tests/auth.rs`, modelled on the existing
`authed_fetch_sends_bearer_source_and_user_agent_headers` test
(`tests/auth.rs:175-205`):

- **`get_profile_me_sends_accept_application_json`** — wiremock
  `Mock::given(method("GET")).and(path("/api/profiles/me"))
  .and(header("accept", "application/json"))`. The mock returns the
  canonical `{"id": 1}` JSON body. `auth.login("alice", "hunter2")`
  must succeed. Currently RED: `get_profile_me` does not send the
  header, so the mock rejects the request and `login()` returns
  `Err`.

- **`get_profile_me_login_succeeds_when_server_honours_accept`** —
  wiremock matches `header("accept", "application/json")` and
  returns valid JSON; an unconditional fall-through mock for
  `GET /api/profiles/me` (no `accept` matcher) returns
  `application/x-protobuf-lite` with binary garbage. With the fix
  in place, the request hits the JSON mock (because it sent the
  expected Accept header), `login()` succeeds, and the protobuf
  fall-through is never matched. Currently RED: without the Accept
  header the request matches the fall-through, returns binary,
  and `Error::AuthFailedBadSchema` is raised. This test mirrors the
  observed live-server behaviour (200 + non-JSON body) and serves
  as the regression guard.

  Note on wiremock matching: `wiremock` 0.6 matches mounts in
  registration order on tie; mounting the strict (with-Accept)
  matcher first and the permissive matcher second produces the
  required behaviour. If the matcher order proves brittle, fall
  back to two registered mocks each with `expect(0)` /
  `expect(1)` and assert call counts at end-of-test.

Both tests run against `MockServer::start()` — no real Zwift host is
contacted.

#### 1b — Implementation

Single-line addition to `crates/zwift-api/src/lib.rs`, between the
existing `User-Agent` line and `.send()` at lines 373-374:

```rust
        .header("User-Agent", &self.inner.config.user_agent)
        .header("Accept", "application/json") // STEP-12.17 — zwift.mjs:523 fetchJSON
        .send()
        .await?;
```

Both Phase 1a tests pass. Run:

```
cargo test -p zwift-api --test auth
cargo test --workspace
```

The full workspace test pass is required because
`crates/zwift-relay/tests/session.rs` and
`tests/relay_runtime.rs` exercise `get_profile_me` indirectly via
`ZwiftAuth::login`; any regression in their wiremock setup (for
example, a mount that did not expect an Accept header) shows up as
a failed test, not a hidden behaviour change.

### Phase 2 — Optional cross-check: confirm no other JSON GET is reached at startup

Verification only; no code change. Run:

```
grep -n 'serde_json::from_slice\|serde_json::from_str' crates/zwift-api/src/lib.rs
```

Expect three sites: the token POST (line ~325), `get_profile_me`
(now fixed), and the line-904 JSON fetch path. Confirm by reading
each call site that:

- Token POST sends `Accept: application/json` (line 307).
- `get_profile_me` sends `Accept: application/json` (Phase 1b).
- Line-904 path sends `Accept: application/json` (already
  present per §3).

If any of those call sites lacks the header, raise it as a
follow-up; do not fix in this plan unless the call site is reached
during the smoke (none currently is, per the §3 audit).

## 6. Verification gate

After Phase 1 lands, the original smoke must succeed past the
profile fetch:

```
ranchero start --capture output.cap
sleep 5
ranchero status
```

Expected:

- `relay.auth.token.granted` followed by no
  `relay.auth.profile.failed` trace.
- `relay.session.login.ok` and the rest of the STEP-12.16
  lifecycle events.
- `ranchero status` reports "running".

If `relay.auth.profile.failed status=200 variant="BadSchema"`
recurs even with the `Accept` header in place, the cause is
something other than content negotiation — at that point the
diagnostic improvements deferred under STEP-20 §20.16 (logging
the response Content-Type, including a body prefix in the error
message) become the next investigation lever and should be
elevated out of the parking lot.

## 7. Deferred to STEP-20 §20.16

Two adjacent diagnostic improvements were noted during this
investigation. Neither is required to make the smoke pass; both
make the *next* failure of the same general class (200 + wrong
body type) self-diagnosing. They are recorded here for traceability
and parked in STEP-20 §20.16 with their own decision rule:

- **Log the response Content-Type on `BadSchema`.** The current
  `relay.auth.profile.failed` trace records only `status` and
  `variant="BadSchema"`. A one-line addition to record the
  `content-type` response header on the BadSchema branch in
  `crates/zwift-api/src/lib.rs:389-397` would distinguish
  "endpoint returned protobuf" from "endpoint returned HTML
  error page" from "endpoint returned malformed JSON" without
  changing the error type or the operator-facing semantics.

- **Include a body prefix in `Error::AuthFailedBadSchema`.** The
  variant message at `crates/zwift-api/src/lib.rs:73-74` is
  `"authentication failed: unexpected response shape: {0}"`,
  with the serde error appended. Including the first ~64 bytes
  of the body (truncated, lossy-decoded UTF-8) in the error
  string would surface the actionable signal — typically the
  first line of an HTML error page or the magic bytes of a
  protobuf payload — without requiring trace-level logs to
  diagnose. Schema change is local to `Error::AuthFailedBadSchema`
  and the BadSchema branch in `get_profile_me`.

These are tracked as STEP-12.20 item #69 and STEP-20 §20.16.
