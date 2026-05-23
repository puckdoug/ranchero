# Step 19 — Compatibility test battery

## Goal

Pull the whole pipeline together and verify spec §7.11:

1. **AES-GCM interop.** Fixed `(key, iv, aad, plaintext)` → byte-identical
   ciphertext+tag between JS and Rust (already exercised in STEP 08; here
   it is pinned in a reproducible top-level test).
2. **Header codec round-trip.** Fuzz all 8 flag combinations (same note).
3. **Login.** The Rust monitor must produce a `ServerToClient` on TCP and
   receive one UDP packet within 5 s of `establish()`. Verified offline by
   replaying a recorded login (no live session is used for testing).
4. **Metric parity.** Feed a recorded `ServerToClient` trace through the
   engine; compare published metrics. ≤ 1e-6 drift on sums, exact match on
   counts and zones (parity proven on synthetic fixtures whose expected
   values can be derived by hand; the real recorded ride is a regression
   guard).
5. **WebSocket parity.** Generate ranchero's own widget pages, point them
   at the Rust daemon, and confirm they render correctly (golden-snapshot,
   frozen in the repository; no live sauce4zwift checkout required).

---

## Current state assessment (checked 2026-05-23)

Before planning new work, here is what already exists in the tree and what
is genuinely outstanding. Each goal item is mapped to the work that remains.

| Goal item | Current state | What remains |
|---|---|---|
| 1 — AES-GCM interop | **Done at crate level.** `crates/zwift-relay/tests/crypto.rs` pins a known-answer vector (`aes_gcm4_encrypt_known_vector`, `aes_gcm4_decrypt_known_vector`) generated from Node.js via `tests/fixtures/gen_vectors.mjs`. | Add a discoverable top-level entry point. See 19.2. |
| 2 — Header round-trip | **Done at crate level.** `crates/zwift-relay/tests/header.rs::header_round_trip_all_flag_combinations` covers all 8 flag combinations of `{RELAY_ID, CONN_ID, SEQNO}`. | Add a discoverable top-level entry point. See 19.3. |
| 3 — Login within 5 s | **Input now in hand.** `tmp/output.cap` is a real capture that includes the full login (OAuth → relay login → first `ServerToClient` on TCP) and a 206 s ride. No live session will be used. | Sanitise the capture, then a replay-based login-confirmation test. See 19.0, 19.4. |
| 4 — Metric parity | **Input now in hand.** STEP 14 ships the stats-engine slice (`crates/zwift-stats/tests/stream_parity.rs` + `athlete_stream.json`); `tmp/output.cap` adds a real ride. | A `compat/` battery: synthetic fixtures for true parity plus the real ride as a regression guard. See 19.5. |
| 5 — WebSocket / widget parity | **Stubs only.** The `pages/` tree holds placeholder files (`index.html` 236 B, one-line `app.mjs`/`main.css`). | **Generate ranchero's own widget pages** (early implementation step), then a golden-snapshot render test. See 19.1, 19.6. |

Two cross-cutting gaps that STEP 18 flagged for this step:

- **Gap #1 — `state.latlng` deviation: STILL OPEN.** `format_state`
  (`src/web/format.rs:387-388`) emits separate `lat`/`lng` scalars where
  sauce4zwift emits a single `latlng: [lat, lng]` array. See 19.7.
- **Gap #2 — v2 WebSocket fanout: NOW RESOLVED.** STEP 18 item M1 is
  complete: `stats_fanout_task_v2` is live and wired in
  `src/web/subs/mod.rs`, and `emit_v2` / `apply_filter_group` are in
  place. A v2 subscription now receives a v2 payload. This step only needs
  to *confirm* the behaviour, not build it. See 19.8.

Supporting facts gathered for the plan:

- **Capture format.** `crates/zwift-relay/src/capture.rs` defines
  `CaptureWriter` / `CaptureReader` / `CaptureFollower`, `CaptureRecord`
  (`ts_unix_ns`, `direction`, `transport`, `hello`, `content_type`,
  `payload`), and `SessionManifest` (carries the AES key + IV seqnos).
  `.cap`/`.bin` files use an `RNCWCAP\0` magic, version 3, a 10-byte file
  header, and 17-byte record headers. `CaptureReader::next_item()` yields
  `CaptureItem::Frame` / `CaptureItem::Manifest`. The writer API is fully
  public, so a sanitised capture can be produced by a small generator.
- **Proto decode + routing.** `ServerToClient::decode(&bytes[..])` (prost)
  decodes a frame; `proto_to_stats::route_player_state(&proto, &state,
  now, wall_clock_ms)` (`src/web/proto_to_stats.rs`) upserts the athlete
  and ingests telemetry with unit conversions;
  `bridge_player_state_event` additionally emits `GameEvent::PlayerState`.
- **Formatters.** `format_athlete_data_v1` / `format_athlete_v2` /
  `format_bucket_stats_v1` etc. live in `src/web/format.rs`. The v1 fanout
  uses `format_athlete_data_v1`.
- **Parity harness.** `tests/support/mod.rs` already provides
  `assert_json_parity` (recursive key-set + tolerance comparison) and
  `build_athlete`. The `compat` tests should reuse `assert_json_parity`.
- **Build env.** Root crate dev-dependencies include `tempfile`, `tokio`
  (with `test-util`), `wiremock`, `tokio-tungstenite`, `rcgen`. A headless
  browser driver is available for the render snapshot. There is **no**
  `benches/` tree yet.

---

## The recorded capture: `tmp/output.cap`

A real session capture (241 KB, format version 3, 311 records, ~205.6 s).
Its record inventory:

| Records | Direction / transport | Meaning |
|--------:|-----------------------|---------|
| #0 | Out HTTP (url-encoded) | OAuth password-grant request — **plaintext credentials** |
| #1 | In HTTP (JSON, 2736 B) | OAuth token response (access + refresh tokens) |
| #3 | In HTTP (JSON, 2801 B) | REST profile response |
| #4 | Out HTTP (protobuf, 18 B) | Relay `LoginRequest` |
| #5 | In HTTP (protobuf, 10314 B) | Relay `LoginResponse` (server pools, TCP/UDP config) |
| #8, #9 | Manifest (UDP) | Session manifests with the **AES session keys** |
| #10 | Out TCP (hello) | TCP hello frame |
| #11 | In TCP (1182 B) | **First `ServerToClient`** |
| 205 | In TCP | `ServerToClient` telemetry stream (the ride) |
| 61 | UDP (42 out / 19 in) | UDP telemetry; first UDP data frame ≈ 121 s in |

Conclusions:

- **It captures the login fully** — OAuth, REST, relay login, session keys,
  TCP hello, and the first `ServerToClient`. Item 3 needs no live session.
- **It captures a full ride** — 205 `ServerToClient` frames for item 4.
- **It contains secrets and personal data.** Record #0 holds a plaintext
  password, #1 holds OAuth tokens, #8/#9 hold AES keys, and the telemetry
  carries real athlete identifiers. **This file must never be committed.**
  It stays in `tmp/` (outside git) and is only ever read by the one-time
  sanitiser in 19.0, which produces the committable fixtures.
- The first UDP *data* frame is ≈ 121 s in (the rider started later), so
  the "within 5 s" timing in item 3 is a live-only property; the offline
  test confirms a `ServerToClient` and a UDP packet are present and decode,
  not the 5 s window.

---

## Decisions

These shape the work below; each is converted into a checklist item.

- **D1 — Capture sanitisation (gates items 3 and 4).** `tmp/output.cap`
  cannot be committed. 19.0 builds a one-time sanitiser that reads it,
  drops every HTTP record (credentials, tokens, REST bodies), keeps the
  TCP/UDP relay telemetry, decrypts it once using the manifest key,
  remaps real athlete identifiers to synthetic ones, and re-emits a clean
  capture of **plaintext `ServerToClient` frames** with a zeroed manifest
  (no key shipped, because the frames are no longer encrypted). Cipher
  parity is covered separately by 19.2's known-answer vector, so dropping
  encryption from the fixture loses no coverage.
- **D2 — Generate web pages, do not copy them (item 5).** ranchero gets its
  own widget pages (19.1), built against the existing sauce-compatible
  HTTP + WebSocket API. No sauce4zwift page is copied in. Keep the pages
  framework-free (plain ES modules + CSS, matching the current stub) so no
  front-end build toolchain is introduced.
- **D3 — Two kinds of oracle (item 4).** True 1e-6 parity is proven on
  **synthetic** fixtures whose expected values can be derived by hand
  (constant power, a simple ramp), the same approach as STEP 14's
  `athlete_stream.json`. The **real ride** from `tmp/output.cap` is used as
  a **regression guard**: ranchero's own output over it is frozen as golden
  and asserted stable. The real ride is not an independent JavaScript
  oracle, because there is no JavaScript replay path and a 206 s variable
  ride cannot be hand-derived; this is stated honestly in `compat/README.md`
  rather than overclaimed. (If an independent JavaScript reference for the
  real ride is ever wanted, it would require a one-time offline derivation;
  that is out of scope here and would need your sign-off given the rule
  that no test path resolves through the sauce4zwift symlink.)
- **D4 — The independent-clone CPU measurement is dropped.** The STEP 14
  deferred item to measure per-push CPU and compare against JavaScript
  (the "10× slower" rule) is removed. A Rust port running slower than the
  JavaScript original is not a credible risk, the comparison cannot be
  measured offline anyway (no JavaScript replay path), and the duplication
  it questioned is negligible at 1 Hz. Recorded as dismissed below.

---

## Layout

A new top-level `compat/` tree plus one root integration test per concern,
and real widget pages under `pages/`:

```
compat/
  fixtures/
    server_to_client/
      constant_power.{source.json,bin}   # synthetic, hand-derivable
      ramp.{source.json,bin}             # synthetic, hand-derivable
      recorded_ride.bin                  # sanitised from tmp/output.cap (D1)
  expected/
    constant_power.metrics.json          # JS-reference parity oracle
    ramp.metrics.json                    # JS-reference parity oracle
    recorded_ride.golden.json            # ranchero regression golden (D3)
  README.md                              # provenance, oracle derivation, licence
pages/
  watching.html + watching.mjs           # live stats widget (19.1)
  nearby.html   + nearby.mjs             # nearby list
  groups.html   + groups.mjs             # groups view
  map.html      + map.mjs                # position widget (exercises latlng)
  shared/*.mjs                           # shared WebSocket client + helpers
tests/
  compat_aes_vector.rs        # 19.2
  compat_header_roundtrip.rs  # 19.3
  compat_login.rs             # 19.4 (replay of sanitised capture; no live)
  compat_metric_parity.rs     # 19.5 (synthetic parity + real-ride regression)
  compat_widget_parity.rs     # 19.6 (serves + renders the generated pages)
```

Provenance note for `compat/README.md`: any material derived from live
Zwift traffic or from sauce4zwift records its source and the AGPL-3.0-only
licence, consistent with the project's licensing.

---

## Workflow reminders for this step

- **Test-first.** For each item, the `-T` task writes a failing test and the
  `-I` task adds the smallest code to make it pass.
- **Slow-test marking.** Any compat test over roughly 100 ms (the real-ride
  replay, the multi-rider trace, the headless render) is marked
  `#[ignore = "slow: <reason>"]` so the inner `cargo test` loop stays fast.
- **No git operations.** Do not commit, move plan files into `done/`, or run
  `git mv`/`git add`/`git rm` on plan files. You own those.
- **The raw capture stays out of git.** `tmp/output.cap` is read only by the
  19.0 sanitiser; only its sanitised output is committed.

---

## 19.0 — Sanitise `tmp/output.cap` into committable fixtures

Prerequisite for 19.4 and the real-ride part of 19.5. A one-time generator,
not a test that runs every build.

Checklist:

- [ ] **19.0-I (sanitiser)** — Add a small generator (a `compat` binary or a
  `cargo xtask`-style helper) that:
  - reads `tmp/output.cap` with `CaptureReader`;
  - **drops every HTTP record** (records #0–#7 and all later HTTP records),
    so no credentials, tokens, or REST bodies survive;
  - reads the session manifest, decrypts the TCP/UDP relay frames once;
  - **remaps real athlete identifiers** to stable synthetic values;
  - re-emits `compat/fixtures/server_to_client/recorded_ride.bin` containing
    plaintext `ServerToClient` frames (`content_type = ProtobufLite`) and a
    **zeroed** manifest (no AES key, frames are plaintext).
- [ ] **19.0 (verify clean)** — Add a check (a fast committed test) that the
  emitted fixture contains no HTTP records, a zeroed manifest key, and no
  occurrence of the original athlete identifiers. This guards against a
  future re-run leaking data.
- [ ] **19.0 (document)** — In `compat/README.md`, record that
  `recorded_ride.bin` was derived from a live capture, list exactly what was
  stripped/remapped, and note the AGPL-3.0-only licence.
- [ ] **19.0 (close the open fixture gap)** — Once `recorded_ride.bin` (or an
  extracted single frame) exists, also satisfy the long-missing
  `crates/zwift-proto/tests/fixtures/server_to_client_basic.bin` and update
  `docs/planning/missing-proto-fixture.md` to closed.

## 19.1 — Generate ranchero's own web pages (early implementation)

Item 5 currently has only stub pages. Build ranchero's own widget pages
against the existing sauce-compatible HTTP + WebSocket API. Do this early so
19.6 has real pages to render and so 19.7 (the `latlng` fix) has a consumer.

Scope a small but genuine set (not the whole sauce widget catalogue):

- a **watching stats** widget — power, heart rate, speed, NP, TSS, W'bal —
  subscribing to `athlete/watching/v2`;
- a **nearby** list — subscribing to `nearby`;
- a **groups** view — subscribing to `groups`;
- a **map / position** widget — reads `state.latlng`, chosen deliberately to
  exercise gap #1 (19.7).

Keep it framework-free: plain ES modules served from `pages/`, a shared
WebSocket client module under `pages/shared/`, no build toolchain.

Checklist:

- [ ] **19.1-T** — A test that the daemon serves each new page with a 200 and
  the correct MIME type (extend the existing `tests/http_static_pages.rs` /
  `tests/http_mime_types.rs` patterns), and that each page's module loads
  without error.
- [ ] **19.1-I** — Write the page HTML + ES modules and the shared WebSocket
  client. Replace the placeholder `index.html`/`app.mjs`/`main.css` with a
  real entry page that links the widgets.
- [ ] **19.1 (no copied assets)** — Confirm nothing is copied from
  sauce4zwift; the pages are ranchero originals (D2).
- [ ] Confirm the static-serving routes in `src/web/http/mod.rs` serve the
  new files (and `pages/shared/` via the existing `/shared/*` route).

## 19.2 — AES-GCM interop, pinned at the top level

The crate test (`crates/zwift-relay/tests/crypto.rs`) already pins a
known-answer vector. Add a discoverable workspace-root entry point.

Checklist:

- [ ] **19.2-T** — Add `tests/compat_aes_vector.rs` that calls
  `zwift_relay::encrypt`/`decrypt` on the fixed `(key, iv, aad, plaintext)`
  and asserts byte-equality with the frozen ciphertext+tag (reuse the exact
  vector from `gen_vectors.mjs`; do not invent a new one).
- [ ] **19.2-I** — No production code expected.
- [ ] Confirm fast (no `#[ignore]`); cross-reference the canonical pin in
  `compat/README.md`.

## 19.3 — Header codec round-trip, pinned at the top level

The crate test (`crates/zwift-relay/tests/header.rs`) already covers all 8
flag combinations. Add a discoverable workspace-root entry point.

Checklist:

- [ ] **19.3-T** — Add `tests/compat_header_roundtrip.rs` that round-trips
  `decode_header(encode(...))` across all 8 combinations of
  `{RELAY_ID, CONN_ID, SEQNO}` through the public `zwift_relay::Header` /
  `decode_header` API, asserting `encode(decode(x)) == x` and the consumed-
  length invariant.
- [ ] **19.3-I** — No production code expected.
- [ ] Confirm fast (no `#[ignore]`); cross-reference in `compat/README.md`.

## 19.4 — Login confirmation by replaying the sanitised capture

Spec §7.11 item 3, offline. No live session (your instruction). Depends on
19.0's `recorded_ride.bin`.

What can and cannot be checked offline:

- **Can:** replaying the captured stream, the pipeline decodes a
  `ServerToClient` on TCP and processes at least one UDP packet — the
  artifacts that prove the login produced a working session.
- **Cannot:** the literal "within 5 s of `establish()`" wall-clock timing,
  which is a live-network property (and the recorded UDP data starts ≈ 121 s
  in regardless). The test asserts presence and successful decode, not the
  5 s window, and says so in a comment.

Checklist:

- [ ] **19.4-T** — Add `tests/compat_login.rs` that reads
  `recorded_ride.bin`, finds the first inbound TCP frame, decodes it as a
  `ServerToClient`, and asserts at least one UDP frame is present and
  decodes. Mark `#[ignore = "slow: replays the full recorded-ride capture"]`
  if it exceeds ~100 ms.
- [ ] **19.4-I** — Any test-only reader helper needed; no new production
  surface beyond the existing capture/decode APIs.
- [ ] Note in the test and in `compat/README.md` that the 5 s timing is a
  live-only property, intentionally not asserted offline.

## 19.5 — Metric parity

Feed recorded `ServerToClient` traces from raw bytes through proto decode,
stats ingest, and published-metric formatting. Two oracle kinds (D3):
synthetic fixtures for true parity, the real ride for regression.

Tolerance: ≤ 1e-6 on sums/averages, **exact** on counts, zones, peak times.

Checklist:

- [ ] **19.5-T (harness)** — Add `tests/compat_metric_parity.rs` that
  discovers every `compat/fixtures/server_to_client/*.bin`, pairs it with
  its oracle in `compat/expected/`, and **fails** when a fixture has no
  oracle (no silent skips).
- [ ] **19.5-I (capture generator)** — A helper that builds a `.bin` from a
  `*.source.json` script using `CaptureWriter`: one record per tick,
  `Inbound`/`Tcp`/`ProtobufLite`, payload `ServerToClient::encode_to_vec()`
  wrapping the tick's `PlayerState`(s). Commit both the `*.source.json` and
  the generated `.bin`.
- [ ] **19.5-I (pipeline)** — In the test, read frames via `CaptureReader`,
  decode with `ServerToClient::decode`, route each `PlayerState` through
  `proto_to_stats::route_player_state` into a `WebState` registry, then
  format with `format_athlete_data_v1` and the v2 path.
- [ ] **19.5-T (comparison)** — Reuse `assert_json_parity`; extend it (or add
  a sibling) so counts/zones/peak-times compare **exactly** while
  sums/averages use ≤ 1e-6. Prove each regime bites (perturb one sum and one
  count locally, watch each fail, revert).
- [ ] **19.5 (synthetic parity fixtures)** — `constant_power` (promote the
  STEP 14 `athlete_stream.json`; see 19.10) and a `ramp` fixture, each with a
  hand-derived `*.metrics.json` parity oracle including per-period peaks and
  NP-peaks.
- [ ] **19.5 (real-ride regression)** — Run `recorded_ride.bin` through the
  same pipeline, freeze ranchero's output as `recorded_ride.golden.json`, and
  assert it stays stable. Mark `#[ignore = "slow: real recorded ride"]`.
  Label it clearly as a regression golden, not a JavaScript parity oracle.
- [ ] Confirm `cargo test --test compat_metric_parity` is green.

## 19.6 — WebSocket / widget parity (render the generated pages)

Depends on 19.1's real pages. Two layers.

- **Payload contract (fast).** Start the web server, open a WebSocket,
  subscribe to the events the widgets use (`athlete/watching/v2`, `nearby`,
  `groups`), and assert the payload shapes against frozen golden JSON with
  `assert_json_parity`. Include a position assertion that exposes the 19.7
  `latlng` issue.
- **Rendered snapshot (slow).** Drive the generated pages headless against
  the running daemon, feed a known WebSocket payload, and compare the
  rendered output to golden snapshots frozen in the repository.

Checklist:

- [ ] **19.6-T (contract)** — Add `tests/compat_widget_parity.rs` with
  WebSocket subscriptions and golden payload-shape assertions, plus the
  position/`state` assertion tied to 19.7.
- [ ] **19.6-T (render)** — Headless render of each generated page against a
  known payload, compared to a committed golden snapshot. Mark
  `#[ignore = "slow: headless browser render"]`.
- [ ] **19.6-I** — Golden JSON + snapshot fixtures and any test-only server
  bootstrap. No production code beyond what 19.1/19.7 already add.
- [ ] **19.6 (record coverage)** — In `compat/README.md`, list which widgets
  are covered and which fields are still null/absent because of deferred
  gaps (G1 athlete profile, G4 event/route metadata), so the snapshots'
  limits are explicit. Note v2 widgets are no longer blocked (see 19.8).

## 19.7 — Resolve the `state.latlng` deviation (gap #1)

`format_state` (`src/web/format.rs:387-388`) emits separate `lat`/`lng`
scalars; sauce4zwift emits a single `latlng: [lat, lng]` array (the streams
formatter already emits `latlng` pairs, so the state formatter is the lone
divergence). The map widget from 19.1 reads `state.latlng`. The underlying
world-coordinate computation (`x`/`y`/`roadCompletion`/`progress`) stays in
STEP 20 §20.19/§20.20; only the `latlng` field shape is in scope here.

Decision to record: (a) repack `lat`/`lng` into `latlng: [lat, lng]` to match
sauce4zwift (recommended — it is the field widgets read), or (b) keep
`lat`/`lng` as a documented deliberate extension.

Checklist:

- [ ] **19.7 decision recorded** in `compat/README.md` and the STEP 18 parity
  ledger (`docs/planning/done/STEP-18-parity-ledger.md`, `_formatState` and
  gap G3 rows).
- [ ] **19.7-T (if a)** — Failing test asserting `format_state` output
  contains `latlng: [lat, lng]` (decide whether the scalar keys are dropped
  or kept alongside).
- [ ] **19.7-I (if a)** — Repack in `format_state`; update the ledger.
- [ ] Re-run `tests/format_slices.rs`, the 19.6 position assertion, and the
  map-widget snapshot to confirm no regression.

## 19.8 — Confirm v2 WebSocket fanout parity (gap #2 — already wired)

STEP 18 M1 is complete: `stats_fanout_task_v2` is live in
`src/web/subs/mod.rs`; `emit_v2` / `apply_filter_group` /
`create_query_strategies` / `create_filter_groups` are implemented and
unit-tested (`tests/subs_emit_v2.rs`, `tests/subs_ws_v2_payload.rs`,
`tests/subs_v2_query_dedup.rs`, `tests/query_reduction.rs`). This is a
**confirmation**, not new construction.

Checklist:

- [ ] **19.8-T** — Open a WebSocket, subscribe to `athlete/watching/v2` with a
  `{resources, stats}` query, push a `PlayerState`, and assert the frame has
  the v2 shape (`version: 2`, requested resources present, unrequested
  absent) — not the v1 shape.
- [ ] **19.8-I** — No production code expected; any gap found is a STEP 18 M1
  regression and is fixed, not deferred.
- [ ] Update this file and the ledger to record gap #2 confirmed closed
  end-to-end.

## 19.9 — Peak-snapshot memory footprint (STEP 14 deferred input)

Measurement-driven, not correctness-driven. Each periodized peak stores a
full clone of the rolling window at the peak moment (matches JS
`stats.mjs:185-189`); worst case ≈ 174 MB across 100 athletes × 5 signals ×
6 periods. The published metrics need only `{snap_value, snap_time}`.
Acceptance is "measured, decided, recorded", not "fixed".

Checklist:

- [ ] **19.9 (measure)** — Drive a multi-rider trace and measure actual
  peak-snapshot resident memory (a counting allocator, a heap snapshot, or
  instrumenting `PeakSnapshot`/`NpPeakSnapshot` construction). Mark
  `#[ignore = "slow: memory measurement under multi-rider trace"]`.
- [ ] **19.9 (decide)** — (a) keep the full clone (matches JS), (b) downgrade
  to `(snap_value, snap_time)`-only, or (c) keep the clone behind a feature
  flag for future analysis tooling.
- [ ] **19.9 (record)** — Decision + measured number in the as-built notes.
  If (b)/(c), the change is mechanically small in `zwift-stats::collector`.

## 19.10 — Fold the STEP 14 fixture into the compat tree

The concrete tie-in for 19.5's first synthetic fixture, deferred here by
STEP 14.

Checklist:

- [ ] Promote `crates/zwift-stats/tests/fixtures/athlete_stream.json` into
  `compat/fixtures/server_to_client/constant_power.{source.json,bin}` so the
  same trace runs from raw bytes, not just the stats-engine slice.
- [ ] Copy the per-period peak and NP-peak values into
  `compat/expected/constant_power.metrics.json`.
- [ ] Keep tolerance consistent with STEP 14 (≤ 1e-6 sums, exact peaks/zones).
- [ ] Leave `crates/zwift-stats/tests/stream_parity.rs` in place as the fast
  unit-level guard; the compat test is the broader orchestration guard.

---

## Dismissed: independent-clone fan-out CPU cost (was a STEP 14 deferred input)

STEP 14 deferred a measurement of the per-push CPU cost of copying
`_times`/`_values` on `RollingAverage::clone` (gap-fill running N+1 times per
push), with a "switch to `Arc<Vec<f64>>` if more than 10× the JavaScript
wall clock" rule. **Dropped, by your decision:** a Rust port being slower
than the JavaScript original is not a credible risk, the comparison cannot be
measured offline (no JavaScript replay path), and the duplication is
negligible at the 1 Hz tick rate. The independent-clone design stays as-is;
no measurement is performed. Recorded here so the STEP 14 deferral is closed
rather than silently forgotten.

---

## Acceptance criteria

STEP 19 is complete when:

- [ ] 19.0 produces a sanitised, secret-free `recorded_ride.bin`, the
  cleanliness check passes, and `missing-proto-fixture.md` is closed.
- [ ] 19.1 ships real ranchero widget pages (watching, nearby, groups, map),
  served correctly, with nothing copied from sauce4zwift.
- [ ] Items 1 and 2 have discoverable top-level tests (19.2, 19.3) and pass.
- [ ] 19.4 confirms, from the sanitised capture, that a `ServerToClient` on
  TCP and a UDP packet decode — no live session used.
- [ ] 19.5's battery runs raw bytes → decode → ingest → format with the
  ≤ 1e-6 / exact-match regimes proven to bite; synthetic parity oracles and
  the real-ride regression golden both pass.
- [ ] 19.6 has a payload-contract test and a rendered snapshot of the
  generated pages; covered/uncovered widgets are recorded.
- [ ] Gap #1 (`state.latlng`) is resolved with a recorded decision; gap #2
  (v2 fanout) is confirmed closed end-to-end.
- [ ] 19.9 is measured, decided, and recorded; the CPU-cost item is recorded
  as dismissed.
- [ ] `cargo test -- --include-ignored` is green across the workspace (fast
  `cargo test` stays fast; new slow tests carry `slow:`-prefixed reasons).
- [ ] `compat/README.md` records every fixture's provenance, the oracle
  derivation (parity vs regression), the AGPL licence note, and the
  sanitisation details.

## Out of scope (tracked elsewhere)

- World-coordinate computation `x`/`y`/`roadCompletion`/`progress` (gap G3) —
  STEP 20 §20.19 item 2 / §20.20. Only the `latlng` field shape is in scope
  here (19.7).
- Athlete-profile cache and FTP/TSS (gaps G1/G2), event/route metadata and
  game session (gap G4) — these leave certain fields null/absent; widgets
  depending on them are listed as covered-with-gaps in 19.6, not implemented
  here.
- An independent JavaScript parity oracle for the real recorded ride — would
  need a one-time offline derivation and your sign-off (D3).
