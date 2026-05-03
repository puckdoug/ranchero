# STEP-12.14 — Detailed review of the daemon UDP-establish path against sauce4zwift

**Status:** review (2026-05-03), pre-implementation. Nine rounds
(§§4, 4b, 4c, 4d, 4e, 4f, 4g, 4h, 4i) of side-by-side walks of the
daemon against `sauce4zwift/src/zwift.mjs` and
`sauce4zwift/src/zwift.proto`. Each round was an attempt to find
any divergence the previous round missed.

- Rounds 1-3 found C1-C12 (the original 12 critical/cosmetic
  divergences).
- Round 4 found N1-N9 (proto-schema and HTTP-detail divergences).
- Round 5 found N10-N12 (including the critical `ackSeqno` tag
  mismatch).
- Round 6 found R1 + R2 (refinements; no bugs).
- Round 7 found N13 (SNTP-corrected clock lost between hello-loop
  and heartbeat).
- Round 8 found N14 (supervisor re-login leaves stale-key
  channels).
- Round 9 found nothing new and confirmed the codec layer is
  clean.

The codec layer (frame, header, IV, AES-GCM-4, plaintext
envelopes) was verified against sauce field-by-field and matches.
All remaining divergences are in the daemon-level orchestration
(`start_all_inner`, `recv_loop`, `HeartbeatScheduler`, supervisor
event handler) and the HTTP-detail layer (headers, content-type,
accept).

After STEP-12.13 §3b's first attempt at fixing the UDP target pick,
this document is a side-by-side walk of the sauce4zwift reference
(`sauce4zwift/src/zwift.mjs`, the JS implementation that demonstrably
works against live Zwift) and the current daemon
(`src/daemon/relay.rs` + `crates/zwift-relay/src/{tcp,udp,session}.rs`).
The goal is to find every place we diverge from the reference's
working contract — both gaps (things sauce does that we don't) and
disagreements (things we do that sauce doesn't), with a judgement
call on whether each one matters for "actually getting UDP up against
real Zwift".

Findings are bucketed Critical / Material / Cosmetic. The Summary
checklist at the end pulls only the must-fix items into Na/Nb pairs.

## Tracking checklist

Each phase / batch is a TDD pair: `Na` writes failing tests against
the contract the work establishes; `Nb` makes those tests pass. See
§8 for full test plans and implementation sketches; §6 for legacy
checklist text reproduced under the same pair numbers.

**Critical-block fix order** (phases 1–8 must land in order — each
unblocks the next point in the live-trace failure chain):

- [x] **1a** — Tests for Phase 1 (UDP target port + ack matcher +
  connId counters): `pick_initial_udp_target` uses port 3024 even
  when `RelayAddress.port = 3022`; hello-ack matcher reads
  `stc.stc_f5` (tag 5) and matches; UDP recv trace reports
  `player_count` from `stc.states` (tag 8); TCP and UDP `connId`
  counters are independent.
- [x] **1b** — Implementation for Phase 1: hardcode 3024 in
  `pick_initial_udp_target`; read `stc.stc_f5` instead of
  `stc.seqno`; fix the `player_count` trace; split
  `CONN_ID_COUNTER` into TCP and UDP statics.
- [ ] **2a** — Tests for Phase 2 (HTTP impersonation):
  `Platform: OSX` on every authenticated request; full
  `User-Agent: CNL/3.44.0 (Darwin Kernel 23.2.0) zwift/1.0.122968 game/1.54.0 curl/8.4.0`;
  `Content-Type: application/x-protobuf-lite; version=2.0` on
  protobuf POSTs; `Accept: application/json` on token requests;
  `Accept: application/x-protobuf-lite` on protobuf requests.
- [ ] **2b** — Implementation for Phase 2: add `platform` to
  `Config`; thread `Platform` header into every send; replace
  `DEFAULT_USER_AGENT`; append `; version=2.0` to
  `PROTOBUF_CONTENT_TYPE`; set `Accept` headers in `login`,
  `do_refresh`, and `post`.
- [ ] **3a** — Tests for Phase 3 (UDP pool selection): daemon
  picks UDP target from the `lb_course=0` pool when both generic
  and per-course pools are present; errors with a typed variant
  if no generic pool is in the push.
- [ ] **3b** — Implementation for Phase 3: refactor
  `extract_udp_servers` → `extract_udp_pools` preserving the
  `(lb_realm, lb_course)` discriminator; daemon's wait-for-
  udp_config branch picks from the generic pool; add
  `RelayRuntimeError::NoGenericPool`.
- [ ] **4a** — Tests for Phase 4 (course gate via
  `getPlayerState`): `auth.get_player_state(id)` decodes the
  proto response; daemon calls it with `cfg.watched_athlete_id`
  (NOT the monitor's `auth.athlete_id()`); daemon suspends when
  the watched athlete has no `state.world` (course); daemon
  errors when `cfg.watched_athlete_id` is unset.
- [ ] **4b** — Implementation for Phase 4: add
  `ZwiftAuth::get_player_state` (HTTP GET
  `/relay/worlds/1/players/{id}` returning a parsed
  `PlayerState`); insert step 4.5 in `start_all_inner` to call
  it and gate UDP setup on `state.world` (tag 35); add
  `RelayRuntimeError::NoWatchedAthlete` and
  `WatchedAthleteNotInGame`.
- [ ] **5a** — Tests for Phase 5 (post-establish UDP send + TCP
  hello seqno): exactly one `send_player_state` call with
  `watching_rider_id`, `id`, `just_watching = true`, `world` is
  recorded between UDP convergence and the first heartbeat; TCP
  hello carries `seqno = Some(0)`, not `Some(1)`.
- [ ] **5b** — Implementation for Phase 5: insert
  `udp_channel.send_player_state(initial_state)` between steps
  9 and 10 of `start_all_inner`; change TCP hello literal's
  `seqno: Some(1)` to `seqno: Some(0)`.
- [ ] **6a** — Tests for Phase 6 (heartbeat content + shared
  WorldTimer): heartbeat carries `id`, `just_watching`,
  `watching_rider_id`, `world` (course), and a non-zero
  `world_time` reflecting the SNTP offset adjusted during UDP
  hello sync.
- [ ] **6b** — Implementation for Phase 6: clone `world_timer`
  before moving into `UdpChannel::establish`; pass the clone to
  `HeartbeatScheduler` along with `watched_id` and `course_id`;
  rewrite `next_payload` (now `next_state`) to populate the
  required PlayerState fields and read `world_time` from the
  shared timer; drop dead CTS-level fields per R2.
- [ ] **7a** — Tests for Phase 7 (UDP hello header consistency):
  every UDP hello iteration's encoded header carries
  `RELAY_ID | CONN_ID | SEQNO`, not just `SEQNO`.
- [ ] **7b** — Implementation for Phase 7: drop the
  `hello_idx == 1` special case in
  `udp.rs::build_send_header`; emit the full triple every
  iteration.
- [ ] **8a** — Tests for Phase 8 (reconnect-state tracking):
  `inner.last_world_update_ts` advances from inbound
  `WorldAttribute.timestamp`; TCP hello's `larg_wa_time`
  reads the running max; world updates with stale `ts` are
  dropped; `last_player_update` carries the world-update
  seqno running max.
- [ ] **8b** — Implementation for Phase 8: add
  `last_world_update_ts: AtomicI64` and `largest_wa_seqno:
  AtomicI64` to `RuntimeInner`; populate from `recv_loop`'s
  `Inbound` arm with dedup; thread current values into the
  TCP hello literal in step 8 of `start_all_inner`.

**Post-critical batches** (independent of each other; can land in
any order after Phase 8):

- [ ] **Aa** — Tests for Batch A (live pool routing &
  multi-channel UDP): mid-session `udp_config_vod_*` pushes
  update `inner.pool_router`; pool router swaps emit
  `GameEvent::PoolSwap`; UDP channel swap runs grace shutdown
  on the old channel; portal pools are accepted via a
  `'portal'` key analogue.
- [ ] **Ab** — Implementation for Batch A: wire
  `extract_udp_pools` into `recv_loop`'s `Inbound` arm;
  implement `recompute_udp_selection` to call
  `find_best_udp_server` and trigger swaps; extend
  `RelayRuntime` to hold multiple UDP channels with grace
  shutdown; patch the proto for `xBoundMin`/`yBoundMin`/
  `securePort` (depends on Eb if proto fork lands first).
- [ ] **Ba** — Tests for Batch B (connect retry & supervisor
  recovery): start failure triggers exponential backoff retry;
  TCP server is pinned across reconnects; supervisor re-login
  recreates channels with the new key; clean shutdown sends
  `logout` and `leave`.
- [ ] **Bb** — Implementation for Batch B: wrap
  `start_all_inner` in a retry loop with `1.2^attempt`
  backoff; persist last-good TCP IP in `RuntimeInner`; replace
  in-place re-login with `SessionEvent::SessionLost` so the
  outer retry loop handles it (sauce parity); add
  `auth.logout()` and `auth.leave()` and call them on
  shutdown.
- [ ] **Ca** — Tests for Batch C (state-refresh fallback &
  suspend / resume): `_refreshStates` polls
  `getPlayerState` on a self-tuning interval; daemon suspends
  after 15 s of no self-state; daemon resumes on incoming
  self-state; polled state is broadcast as a fake server
  packet.
- [ ] **Cb** — Implementation for Batch C: spawn
  `StateRefresher` task in `start_all_inner` with self-tuning
  delay (3 s minimum, 30 s on suspend, 5 min on errors); add
  `RuntimeInner::suspended: AtomicBool`; gate heartbeat ticks
  on `suspended == false`; resume from `_updateSelfState`-
  equivalent path in `recv_loop`.
- [ ] **Da** — Tests for Batch D (diagnostics & TCP-flag
  parity): `expungeReason` is logged when present; TCP
  non-hello sends emit no SEQNO flag in header; TCP hello
  omits SEQNO flag when `iv_seqno == 0`; `udp_config_vod_2`
  and flat `udp_config` fallback paths are inert.
- [ ] **Db** — Implementation for Batch D: add `expunge_reason`
  log in `recv_loop`; restructure `tcp.rs::send_packet` to
  emit `flags=0` for non-hello and conditional SEQNO for
  hello-with-iv_seqno=0; remove fallback paths from
  `extract_udp_pools` (or feature-gate for zwift-offline
  compat).
- [ ] **Ea** — Tests for Batch E (proto fork: drop required
  markers, add missing fields): TCP hello wire bytes omit tag
  1 (`server_realm`), tag 7 (`state`), tag 10
  (`last_update`), tag 12 (`last_player_update`); UDP hello
  carries exactly four wire fields (tags 1-4); `RelayAddress`
  round-trips with all 9 tags populated.
- [ ] **Eb** — Implementation for Batch E: fork the vendored
  proto under `crates/zwift-proto/src/zwift_patched.proto`
  changing `required` → `optional` on tags 1, 7, 10, 12 of
  `ClientToServer`; add tags 7, 8, 9 to `RelayAddress`;
  regenerate via `prost-build`; update every
  `ClientToServer { … }` literal to omit defaulted fields;
  add presence-check audits for fields we still expect set.

## 0. Findings summary (priority-ordered)

| ID | Severity | Summary | Bundled into checklist pair |
| --- | --- | --- | --- |
| **C5** | **Critical** | Hardcode UDP port 3024; `RelayAddress.port` is the **plaintext** port (3022) — encrypted hellos to the plaintext port surface as the live trace's `Connection refused` | 1 |
| **N10** | **Critical** | Hello-ack matcher reads `stc.seqno` (tag 4) instead of `stc.stc_f5` (tag 5 = sauce's `ackSeqno`) — SNTP convergence is fed coincidentally-matching nonsense; second blocker after C5 | 1 |
| **N13** | **Critical** | `WorldTimer` SNTP offset is silently lost between hello loop and heartbeat (two `WorldTimer::new()` calls = independent state); heartbeats send uncorrected `world_time` — server may drop the session | 6 |
| **C1** | **Critical** | `extract_udp_servers` flattens all pools and picks the first arbitrary entry; sauce specifically uses `_udpServerPools.get(0).servers[0]` (the `lb_course=0` generic load-balancer pool) | 3 |
| **C2** | **Critical** | Daemon never learns the watched athlete's `courseId`; sauce gates UDP setup on `getPlayerState(selfAthleteId)` returning a course | 4 |
| **C3** | **Critical** | No post-establish UDP `sendPlayerState({watchingAthleteId})` — without it, UDP comes up but server sends nothing back | 5 |
| **C4** | **Critical** | Heartbeat sends `state: PlayerState::default()`; sauce sends `{id, just_watching, watching_rider_id, courseId, x, y, z, eventSubgroupId}` — server drops session after a few empty heartbeats | 6 |
| **C6** | **Critical** | Missing `Platform: OSX` HTTP header on every authenticated request | 2 |
| **C7** | **Critical** | `User-Agent: CNL/4.2.0` is a stub; sauce sends the full Zwift game string `CNL/3.44.0 (Darwin Kernel 23.2.0) zwift/1.0.122968 game/1.54.0 curl/8.4.0` | 2 |
| **C8** | **Critical** | Protobuf `Content-Type` missing `; version=2.0` parameter | 2 |
| **N1** | Material | `ClientToServer` hello body sends extra `state`, `last_update`, `last_player_update` fields sauce omits — proto-required forces them; needs proto fork | (deferred) |
| **N3** | Material | Token endpoint missing `Accept: application/json` | 2 |
| **N4** | Material | Protobuf requests missing `Accept: application/x-protobuf-lite` | 2 |
| **N6** | Material | Inbound `worldUpdates` not deduplicated by `ts` (pre-req for L3 / M2) | 8 |
| **N7** | Material | Tag 10 / tag 12 (`last_update`/`last_player_update`) sent as 0; reconnect re-floods world updates from session start | 8 |
| **N9** | Material | No clean `/api/users/logout` / `/relay/worlds/1/leave` on shutdown — server-side session lingers up to 90 min | (L-block) |
| **N14** | Material | Supervisor re-login (post-refresh-failure) writes new capture manifest but does NOT recreate channels — old `aes_key` still in use; silent data-plane death | (blocked-by-L5) |
| **M1** | Material | UDP hello iter 2+ drops `relay_id`+`conn_id` from header; sauce keeps them on every hello — relevant on lossy networks | 7 |
| **M2** | Material | TCP hello sends `larg_wa_time = 0` (vs sauce's `_lastWorldUpdate`); reconnect path re-floods world updates | 8 |
| **L1** | Material | No `_refreshStates` polling fallback (`getPlayerState` on 3-30s self-tuning interval); data pipeline silent during UDP quiet periods | (L-block) |
| **L2** | Material | No suspend / resume on idle (15 s of no self-state → suspend; live data → resume) | (L-block) |
| **L3** | Material | `_lastWorldUpdate` not tracked from incoming `worldUpdates[*].ts` (pre-req for M2/N7) | 8 |
| **L5** | Material | No connect retry with exponential backoff (`1.2^backoffCount`); a single network blip kills the daemon | (L-block) |
| **L6** | Material | Single UDP channel; no overlap-and-grace switch (sauce: `_udpChannels[]` with 60s reusable / 1s otherwise); required once pool routing lands | (L-block) |
| **C9** | Cosmetic | Documentation breadcrumb: courseId lives in `PlayerState.world` (tag 35), not `f19` aux | inline in 4b/6b |
| **C10** | Cosmetic | Documentation breadcrumb: y/z fields swapped in zoffline naming (sauce's tag 26 `z` is our `y_altitude`) | inline in 6b |
| **C11** | Cosmetic | Vendored proto missing `xBoundMin`/`yBoundMin`/`securePort` on `RelayAddress` (tags 7-9); needed for full pool routing | (deferred) |
| **C12** | Cosmetic | `watching_rider_id` is int64 in zoffline, int32 in sauce (tag 28) — wire-tolerant | (deferred) |
| **N2** | Cosmetic | TCP/UDP share one `connId` counter; sauce uses two (per-class statics) | 1 |
| **N5** | Cosmetic | TCP hello uses `seqno: 1`; sauce starts at 0 (off-by-one) | 5 |
| **N8** | Cosmetic | `expungeReason` from server is silently ignored (sauce also doesn't act on it; defines field for diagnostics) | (L-block) |
| **N11** | Cosmetic | UDP recv tracing reports `player_count = stc.player_states.len()` (tag 28 = blocked list) instead of `stc.states.len()` (tag 8) | 1 |
| **N12** | Cosmetic | TCP hello carries `server_realm: 1`; sauce's TCP hello omits realm (same root as N1) | (deferred) |
| **M3** | Cosmetic | TCP non-hello sends include `SEQNO` flag; sauce uses flags=0 (server tolerates either) | (deferred) |
| **k1** | Cosmetic | TCP hello sets `SEQNO` flag with seqno=0; sauce omits when iv.seqno=0 (server tolerates) | (deferred) |
| **k2** | Cosmetic | We honour `udp_config_vod_2` and flat `udp_config` as fallbacks; sauce only acts on `udpConfigVOD.pools` | (deferred) |
| **k3** | Cosmetic | No `udpConfigVOD.portalPools` handling | (deferred) |
| **k4** | Cosmetic | `find_best_udp_server` exists but is never called (no live pool routing) | (12.13 plan §4) |
| **L4** | Cosmetic | TCP server is not pinned across reconnects (sauce: `_lastTCPServer`) | (L-block) |
| **L7** | Cosmetic | `auxCourseId` packed in `PlayerState.f19` bits 16-23 — alternative to C9; not actually needed since tag 35 is canonical | inline in 4b |
| **R1** | Refinement | C2 must call `get_player_state(cfg.watched_athlete_id)`, not the monitor's `auth.athlete_id()` | inline in 4b |
| **R2** | Refinement | `HeartbeatScheduler::next_payload` builds a CTS whose all-but-`state` fields are dead code | inline in 6b |

**Critical-block fix order** (what unblocks the first successful
live trace, ordered by suggested implementation sequence):

1. **C5 + N10 + N2** — UDP target port + ack matcher field + connId counters (one site, one Na/Nb pair).
2. **C6 + C7 + C8 + N3 + N4** — HTTP impersonation (one site, one Na/Nb pair).
3. **C1** — Pool selection (refactor `extract_udp_servers`).
4. **C2** — `get_player_state(cfg.watched_athlete_id)` + courseId gate.
5. **C3 + N5** — Post-establish PlayerState + TCP hello seqno=0.
6. **C4 + N13 + R2** — Heartbeat content + WorldTimer sharing.
7. **M1** — UDP hello iter 2+ keeps `relay_id`+`conn_id`.
8. **M2 + L3 + N6 + N7** — `_lastWorldUpdate` tracking + TCP hello `larg_wa_time` + worldUpdate dedup + tag 10/12 reconnect values.

L-block items (L1, L2, L4, L5, L6, N8, N9, N14) and proto-fork
items (N1, N12, C11) defer to a follow-up STEP. C9, C10, C11
(part), C12, M3, k1-k4, L7, R1, R2 are documentation breadcrumbs
or inline-comment work baked into the implementations above.

## 1. The reference flow at a glance

`zwift.mjs::GameMonitor._connect()` (line 1755):

```
login()                      ← HTTP POST /api/users/login (LoginRequest)
                               returns LoginResponse {relaySessionId,
                               session.tcpConfig.servers, expiration}
initPlayerState()            ← HTTP GET /relay/worlds/1/players/{selfId}
                               returns a PlayerState; courseId comes from
                               state.f19 bits 16-23 (decoded as
                               state.auxCourseId by processPlayerState)
setCourse(s.courseId)        ← updates this.courseId from the
                               player-state response
establishTCPChannel(session) ← TCP connect to filtered tcpServers[0],
                               no hello yet
activateSession(session)
   ├─ sendPacket({athleteId, worldTime: 0,
   │              largestWorldAttributeTimestamp: this._lastWorldUpdate},
   │             {hello: true})            ← TCP hello
   ├─ await udpServerPoolsUpdated event    ← wait for the udpConfigVOD
   │                                          push on the TCP stream
   ├─ this._session = session
   └─ if (this.courseId) setUDPChannel()   ← bring UDP up off
      else suspend()                          _udpServerPools.get(0)
                                              (the load balancer); see
                                              establishUDPChannel below
```

`establishUDPChannel(ch)` (line 2127), called by `setUDPChannel`:

```
ch.establish()               ← UDP socket.connect(3024, ip);
                               24-iter hello loop with SNTP-style sync
ch.sendPlayerState({         ← THE FIRST POST-ESTABLISH PACKET registers
   watchingAthleteId            the watching-athlete with the server.
})                              Without it, no inbound traffic flows.

…then 1 Hz broadcastPlayerState fires forever:

ch.sendPlayerState({         ← every tick re-asserts the session
   watchingAthleteId,            context (which is what the server
   _flags2: …roadId,             keys on for what to send back).
   portal,
   eventSubgroupId,
   ...watchingStateExtra
})
```

`setUDPChannel(ip)` (line 2103), called with no argument on initial
connect:

```js
if (!ip) {
    // Use a load balancer initially, We'll get swapped to a direct
    // server soon after..
    ip = this._udpServerPools.get(0).servers[0].ip;
}
```

`onInPacket(pb)` (line 2143), the TCP receive handler:

```js
if (pb.udpConfig) {
    // I believe this is the "load balancer" address, that can also
    // be found in the VOD list..
}
if (pb.udpConfigVOD) {
    for (const x of pb.udpConfigVOD.pools) {
        this._udpServerPools.set(x.courseId, x);
    }
    if (pb.udpConfigVOD.portalPools) {
        this._udpServerPools.set('portal', pb.udpConfigVOD.portalPools[0]);
    }
    this.emit('udpServerPoolsUpdated', this._udpServerPools);
}
```

Three load-bearing facts in the reference:

1. **UDP server pools are a `Map<courseId, pool>`**, populated from
   `pb.udpConfigVOD.pools`. Each pool entry carries its own
   `courseId`; the special key `0` is the **generic load-balancer
   pool** that every newly-connected client uses first.
2. **Initial UDP target = `_udpServerPools.get(0).servers[0].ip`** —
   *not* an arbitrary entry from the flat list, *not* the per-course
   pool, *not* `tcp_servers[0]`. The load balancer at courseId=0
   accepts everyone; per-course pools may reject athletes who aren't
   on that course.
3. **`pb.udpConfig` is ignored** — sauce's own comment notes it's
   redundant with the VOD list. The daemon should not key off it.

A separate fact, equally load-bearing for the *whole* UDP path to
even fire:

4. **The daemon must know the watched athlete's `courseId` before
   bringing UDP up.** Sauce reads it via
   `getPlayerState(selfAthleteId)` from `/relay/worlds/1/players/{id}`
   and stores it in `this.courseId`. If `courseId` is unset
   (athlete not currently in a game), `activateSession` calls
   `suspend()` rather than `setUDPChannel()` — UDP never comes up,
   no hellos are sent, no "Connection refused" race.

## 2. Findings — Critical (will break against live Zwift)

### C1 — `extract_udp_servers` flattens all pools and picks the wrong one

**Where:** `crates/zwift-relay/src/lib.rs::extract_udp_servers`, used
by `src/daemon/relay.rs::start_all_inner` step 8.5 (the wait-for-
udp_config branch).

**What we do:**

```rust
pub fn extract_udp_servers(stc) -> Option<Vec<RelayAddress>> {
    if let Some(vod) = &stc.udp_config_vod_1 {
        let addrs: Vec<_> = vod.relay_addresses_vod.iter()
            .flat_map(|p| p.relay_addresses.iter().cloned())
            .collect();
        if !addrs.is_empty() { return Some(addrs); }
    }
    // …falls back to udp_config_vod_2, then udp_config…
}
```

Then `pick_initial_udp_target` picks **the first entry** of the
flattened list, ignoring every pool's `lb_course` discriminator.

**What sauce does:**

```js
for (const x of pb.udpConfigVOD.pools) {
    this._udpServerPools.set(x.courseId, x);
}
…
ip = this._udpServerPools.get(0).servers[0].ip;
```

Pools indexed by `courseId`. Initial pick is **always from
`courseId=0` (generic load balancer)**.

**Why it matters:** if Zwift's first `udp_config_vod_1` push lists
the per-course pools first (e.g. courseId=42 ahead of the courseId=0
load-balancer pool), our `pick_initial_udp_target` picks the
courseId=42 server. That server is sized for athletes already on
course 42; an unknown athlete reaching it gets ICMP Port Unreachable,
which surfaces back to us as `os error 61 "Connection refused"` —
**exactly the failure mode in the live trace**.

This is the single most likely root cause for the live trace. Even if
3b's wait-for-push is doing its job, picking the wrong pool entry
puts us back in the same hole.

**Fix sketch:** preserve the `(lb_course, lb_realm)` discriminator
through `extract_udp_servers` (return the structured `RelayAddressesVod`
list, or a typed "pools" wrapper); add a "pick from lb_course=0 pool"
helper that the daemon uses for the initial connect.

### C2 — Daemon never learns the watched athlete's `courseId`

**Where:** `src/daemon/relay.rs::start_all_inner` (the whole flow);
`crates/zwift-api/src/lib.rs` (no helper for
`/relay/worlds/1/players/{id}` yet).

**What we do:** nothing. The runtime brings UDP up the moment it
sees a `udp_config*` push, regardless of whether the athlete is in
a game. The plumbing exists for a watched-athlete state
(`WatchedAthleteState`, `apply_pool_update`,
`observe_watched_player_state`), but nothing populates it from a
real source.

**What sauce does:** `initPlayerState()` runs **before**
`establishTCPChannel`:

```js
async initPlayerState() {
    if (this.selfAthleteId != null) {
        const s = await this.api.getPlayerState(this.selfAthleteId);
        this.setCourse(s ? s.courseId : null);
        if (s) {
            this.setWatching(s.watchingAthleteId);
            …
        } else {
            this.setWatching(this.selfAthleteId);
        }
    }
}
```

`getPlayerState(id)` is a `GET /relay/worlds/1/players/{id}` returning
a `PlayerState` protobuf. The result's `courseId` is what populates
`this.courseId`.

`activateSession` then guards `setUDPChannel()` on `this.courseId`:

```js
if (!this.suspended && this.courseId) {
    this.setUDPChannel();
} else {
    console.warn("User not in game: waiting for activity...");
    this.suspend();
}
```

**Why it matters:** even if C1 lands and we correctly pick the
load-balancer pool, the load balancer routes the athlete to a
direct server based on the courseId we're playing on. A monitor
account that isn't currently in a game has no courseId, sauce
suspends (no UDP), and there's no failure to chase. The daemon
needs the same gate. Without it, the daemon will fire UDP hellos
for a session that the server has nothing useful to do with — at
best a wasted load-balancer connection, at worst the rejected
hellos we already saw.

For the live trace specifically: the daemon was operating as the
monitor account (`5213306`). Until that athlete (or a watched
athlete) is in a live game, sauce wouldn't even *try* UDP.

**Fix sketch:** add `ZwiftAuth` (or `zwift-relay::session`) helper
`get_player_state(athlete_id) -> Option<PlayerState>`; call it from
`start_all_inner` after auth login and before TCP-establish; if it
returns `None` or `state.course_id` is 0/None, log
`relay.runtime.suspended_no_course` and skip the UDP setup branch.
A subsequent watched-athlete activity event (from the TCP stream)
re-arms UDP setup. Suspend-and-resume mirrors sauce's `suspend()` /
`resume()`.

## 3. Findings — Material (likely to matter under live conditions)

### M1 — Subsequent UDP hello headers drop `relay_id` and `conn_id`

**Where:** `crates/zwift-relay/src/udp.rs::build_send_header`.

**What we do:**

```rust
fn build_send_header(hello_idx: u32, relay_id, conn_id, iv_seqno) -> Header {
    if hello_idx == 1 {
        Header { flags: RELAY_ID|CONN_ID|SEQNO, … }
    } else {
        // Steady-state: SEQNO only — peer caches the rest.
        Header { flags: SEQNO, … }
    }
}
```

Only the first hello carries `relay_id` and `conn_id`. Subsequent
hello iterations carry `SEQNO` only.

**What sauce does:** sauce sends `{hello: true}` for **every** hello
iteration in the loop (`zwift.mjs:1378-1395`). Its `encodeHeader` for
`{hello: true}`:

```js
if (options.hello) {
    flags |= headerFlags.relayId;       // every hello
    …
}
if (options.hello && iv.connId !== undefined) {
    flags |= headerFlags.connId;        // every hello
    …
}
if ((options.hello && iv.seqno) || options.forceSeq) {
    flags |= headerFlags.seqno;         // forceSeq is true for UDP
    …
}
```

So sauce hello iter 2..25 carries **`relay_id + conn_id + seqno`**,
not just seqno.

**Why it matters:** the first hello might be dropped (real-world UDP
loss). Sauce's design assumes the server may need the `(relay_id,
conn_id)` to identify the session on iter 2+ if iter 1 didn't reach
it. Our design assumes iter 1 reached the server and the server
cached the IV state — a different assumption that's only valid in
near-perfect networks.

**Fix sketch:** drop the `hello_idx == 1` special-case; emit
`RELAY_ID | CONN_ID | SEQNO` for every hello iteration in the loop.

### M2 — TCP hello payload differs from sauce

**Where:** `src/daemon/relay.rs::start_all_inner` step 8 (TCP hello
send).

**What we do:**

```rust
tcp_sender.send_packet(
    zwift_proto::ClientToServer {
        server_realm: 1,
        player_id: athlete_id,
        world_time: Some(0),
        seqno: Some(1),
        state: zwift_proto::PlayerState::default(),
        ..Default::default()
    },
    true,
).await?;
```

We include `state: PlayerState::default()` and explicit `seqno: 1`.
We do **not** include `larg_wa_time` (sauce's
`largestWorldAttributeTimestamp`).

**What sauce does** (`zwift.mjs:1895`):

```js
session.tcpChannel.sendPacket({
    athleteId: this.athleteId,
    worldTime: 0,
    largestWorldAttributeTimestamp: this._lastWorldUpdate,
}, {hello: true})
```

No `realm` / `server_realm` (sauce sets it server-side or relies on
defaults), no `state`, but **does** include
`largestWorldAttributeTimestamp`. The seqno is auto-assigned by
`makeDataPBAndBuffer` (starts at 0, not 1).

**Why it matters:** the `state: PlayerState::default()` field is
`required` in proto2; prost will encode it as a zero-length submessage
on tag 7. Sauce's protobuf-js call (`fromObject({…})`) omits the
field entirely, which on the wire is *also* an empty tag 7 in proto2
(required-but-absent is technically a wire-format violation, but
Zwift evidently tolerates it). Either form is probably accepted.

The missing `larg_wa_time` is more interesting: Zwift's
`TcpClient::sayHello` (the proto comment on tag 13) reads this on
the server side. On a fresh first connect, sending 0 (or omitting
it) is fine. After a reconnect mid-session, sending 0 may cause the
server to re-flood every world attribute since session start. Not
relevant to the "UDP refuses to come up" trace, but worth fixing
before any production reconnect work.

**Fix sketch:** add a `last_world_update_ts` field to the runtime
state (initially 0, advanced from incoming `worldUpdates[*].ts`),
include it as `larg_wa_time` in the TCP hello.

### M3 — TCP non-hello headers always carry SEQNO; sauce doesn't

**Where:** `crates/zwift-relay/src/tcp.rs::send_packet` non-hello
branch.

**What we do:** always set `flags: SEQNO` on TCP non-hello sends.

**What sauce does:** TCP non-hello uses `encodeHeader({})` — no
hello, no forceSeq. All three flag conditions are false. Header is
just the 1-byte flags=0; the encrypted body follows immediately, IV
seqno auto-incremented client-side and (presumably) server-side.

**Why it matters:** server may get confused by an unsolicited SEQNO
in the header on steady-state TCP. Or it may not — Zwift's recv side
explicitly handles "if the header carries seqno, use it; otherwise
use the cached recv_iv_seqno+1". Probably tolerated; flag for review
in any future TCP steady-state work.

### M4 — Pool-router population from mid-session pushes

**Where:** `src/daemon/relay.rs::recv_loop` `Inbound` arm.

**What we do:** nothing. `RuntimeInner.pool_router` stays empty
forever.

**What sauce does:** every incoming TCP `ServerToClient` runs
through `onInPacket`, which calls into the udpConfigVOD branch
(line 2154-2167) for any non-empty push, updating the pool map and
emitting `udpServerPoolsUpdated`.

**Why it matters:** without mid-session pool updates, the daemon
can't switch UDP servers in response to (a) the watched athlete
moving to a different course or (b) Zwift reshaping the pool for
load-balancing reasons. STEP-12.13 §3b deferred this; STEP-12.14
should also defer it pending C1+C2 landing first. Restating here so
it doesn't get lost.

## 4. Findings — Cosmetic (worth noting, won't break anything)

### k1 — TCP hello sets `SEQNO` flag with seqno=0; sauce omits it

Sauce's `encodeHeader` skips the SEQNO flag when `iv.seqno === 0` on
the first hello (the `(options.hello && iv.seqno)` branch). Our TCP
hello always emits SEQNO. Server tolerates either; cosmetic.

### k2 — `udp_config` (flat) and `udp_config_vod_2` ignored by sauce; we honour them

Sauce's `onInPacket` only acts on `pb.udpConfigVOD`. The flat
`pb.udpConfig` is observed but explicitly skipped (the comment notes
it as "the load balancer address, that can also be found in the VOD
list" — i.e. redundant). We treat all three as equivalent fallbacks
in `extract_udp_servers`. Probably fine — the redundancy means the
flat field, if present, points at the same set of IPs as the VOD.
But matching sauce more strictly avoids surprises.

### k3 — No `udpConfigVOD.portalPools` handling

Sauce special-cases the portal pool with key `'portal'`. We ignore
it. Portal travel is a niche path; deferring is fine, but document
the gap.

### k4 — `find_best_udp_server` exists but is never called

The whole watched-athlete bounding-box selection
(`UdpPoolRouter::pool_for`, `find_best_udp_server`,
`recompute_udp_selection`) is unwired. Same lifecycle bucket as M4.

## 4b. Round-2 findings (the bits the first pass missed)

The first pass through this review stopped at `extract_udp_servers`
and the courseId gate and called it "enough". A second, more
methodical walk of `zwift.mjs` from constructor through
`broadcastPlayerState` and `_refreshStates` surfaced more material
gaps. The two below are critical (will block UDP from carrying any
real data even after C1+C2+M1+M2 land); the rest are listed L1–L7
as long-tail correctness deltas.

### C3 — Daemon never sends a post-establish "I'm watching" packet

**Where:** `src/daemon/relay.rs::start_all_inner` step 10
(heartbeat scheduler spawn). The UDP channel comes up via
`UdpChannel::establish(...).await?` and that's it — the next UDP
packet is the 1 Hz heartbeat one second later, with an empty
`PlayerState` (see C4).

**What sauce does** (`zwift.mjs::establishUDPChannel`, line 2127):

```js
async establishUDPChannel(ch) {
    try {
        await ch.establish();
        await ch.sendPlayerState({watchingAthleteId: this.watchingAthleteId});
    } catch(e) { … }
}
```

The very first packet after the UDP hello loop converges is a
`sendPlayerState` carrying the `watchingAthleteId`. This is what
**registers the relay session with the server** for inbound traffic.
Without it, the server has the connection up (UDP hellos got acked
in the bidirectional sync) but no idea what data the client wants.
Inbound `ServerToClient` flow effectively never starts.

**Why it matters:** even with C1 (right pool), C2 (right courseId),
M1 (relayId/connId on hellos), and M2 (larg_wa_time), a daemon that
gets to `relay.udp.established` will then sit silent forever. No
inbound `relay.udp.message.recv`, no player states, no world
updates. The trace will *look* successful but be useless.

This is also what the user's live trace would have done if 3b had
kept the daemon alive past UDP-establish: the daemon would have
called `relay.heartbeat.started`, the heartbeat would fire, and an
empty PlayerState would go out — but no inbound data ever arrives.

**Fix sketch:** after `UdpChannel::establish` returns Ok, call
`udp_channel.send_player_state(initial_state)` where `initial_state`
includes at minimum `id = athlete_id`, `just_watching = true`,
`watching_rider_id = watching_athlete_id` (defaulting to
`self_athlete_id` if no separate watching ID was configured).

### C4 — Heartbeat payload is empty `PlayerState::default()`

**Where:** `src/daemon/relay.rs::HeartbeatScheduler::next_payload`,
line 435. Every 1 Hz tick builds:

```rust
zwift_proto::ClientToServer {
    server_realm: 1,
    player_id: self.athlete_id,
    world_time: Some(self.world_timer.now()),
    seqno: Some(next_seqno),
    state: zwift_proto::PlayerState::default(),  // every field None
    last_update: 0,
    last_player_update: 0,
    ..Default::default()
}
```

**What sauce sends** (`broadcastPlayerState`, line 1942):

```js
await ch.sendPlayerState({
    watchingAthleteId: this.watchingAthleteId,
    _flags2: portal ? encodePlayerStateFlags2({roadId: lws.roadId}) : undefined,
    portal,
    eventSubgroupId: lws?.eventSubgroupId || 0,
    ...this.watchingStateExtra
});
```

…which in turn (`UDPChannel.sendPlayerState`, line 1450) builds:

```js
const state = {
    athleteId: this.athleteId,
    worldTime,
    justWatching: true,
    x: 0, y: 0, z: 0,
    courseId: this.courseId,
    ...extraState,  // includes watchingAthleteId
};
```

So sauce's heartbeat carries `{athleteId, justWatching: true, x: 0,
y: 0, z: 0, courseId, watchingAthleteId, eventSubgroupId,
...extras}`. Our heartbeat carries an entirely empty PlayerState.

**Why it matters:** Zwift uses these per-tick fields (especially
`watchingAthleteId` and `justWatching`) as the renewing session
context. An empty PlayerState heartbeat probably reads to the server
as "active rider with no position / no observation target". If C3
above is fixed (initial post-establish send) the server may
*briefly* deliver data, then drop us when the heartbeat-derived
session context goes empty.

**Fix sketch:** the `HeartbeatScheduler` needs `watching_athlete_id`
and `course_id` plumbed in from `RuntimeInner.watched_state`. Build
the payload with `id, just_watching: true, watching_rider_id,
x: 0, y: 0, z: 0` — and update the `course_id`-in-`f19` packing
once C2 makes course_id known. Same source proto types, just
populated.

### L1 — No `_refreshStates` polling loop

**What sauce does** (`zwift.mjs:1998-2057`): every
`_stateRefreshDelay` ms (3 s minimum, expanding to 30 s on
suspend, 5 min on errors), polls `/relay/worlds/1/players/{selfId}`
and `{watchingId}`, synthesizes "fake server packets" so downstream
consumers always have fresh data even when the live UDP stream is
quiet.

**Why it matters:** Zwift's UDP stream is *not* a guaranteed 1 Hz
delivery. If the watched athlete is parked or the network drops
packets, the data pipeline goes silent. Sauce's HTTP fallback fills
the gap.

**Daemon impact:** the `--debug` log will appear to "stall" for
long stretches between TCP/UDP messages even when the connection
is healthy. Misleading rather than wrong.

**Fix scope:** new background task in `start_all_inner` that polls
`get_player_state(self_athlete_id)` (and `watching_athlete_id`
separately if different) on the same self-tuning cadence.
Multi-step; sized for its own STEP, not 12.14.

### L2 — No suspend / resume

**What sauce does:**
- `_refreshStates` calls `suspend()` if `age > 15000` (15 s of
  no fresh self-state).
- `_updateSelfState` calls `resume()` on incoming live data.
- `suspend()` calls `schedShutdown(30000)` on every UDP channel
  (gives the server 30 s to flush remaining traffic, then closes).

**Why it matters:** prevents the daemon from drowning in retries
or holding stale UDP sockets after the watched athlete logs off
or parks. Also prevents Zwift's rate-limiter from kicking in.

**Daemon impact:** the daemon currently never suspends; it'll keep
sending empty heartbeats indefinitely. For the immediate trace
debug this isn't visible, but in any longer-running test it
matters.

### L3 — `_lastWorldUpdate` is never tracked

Already noted as M2's data source. Sauce updates `_lastWorldUpdate`
on every incoming `worldUpdates[*].ts`; we never read those
timestamps. Fixing M2 *requires* this tracking — the M2 fix without
L3 would just send a constant `0`.

### L4 — TCP server is never pinned across reconnects

**What sauce does** (`establishTCPChannel`, line 1817-1827):

```js
let ip;
if (this._lastTCPServer) {
    const lastServer = servers.find(x => x.ip === this._lastTCPServer);
    if (lastServer) ip = lastServer.ip;
}
if (!ip) ip = servers[0].ip;
this._lastTCPServer = ip;
```

The comment notes: *"After countless hours of testing and
experiments I've concluded that I really need to stick to the same
TCP server no matter what."*

**Daemon impact:** on reconnect we pick `tcp_servers[0]` afresh,
which may not be the same as the previous run. Probably manifests
as session-state confusion on the server side after a flap.

### L5 — No connect retry / exponential backoff

`_schedConnectRetry` (sauce line 1876): `delay = max(1000, 1000 * 1.2^backoffCount)`.

Our daemon errors out and exits. A network blip during start kills
the daemon process; the operator has to restart manually.

### L6 — Single UDP channel; no overlap-and-grace switch

Sauce maintains `_udpChannels: []` and switches by:
1. Make a new channel.
2. `schedShutdown(60000)` on the old one (60 s grace if the new
   one is reusable, 1 s otherwise).
3. Insert new at index 0.
4. Send via the first active channel.

This lets in-flight data from the old server drain naturally during
the swap. We have a single channel; on swap (whenever we wire pool
routing) we'd have to drop everything from the old server.

### L7 — `course_id` is packed in `PlayerState.f19` bits 16-23, not exposed as a field

`zoffline`'s proto has tag 19 = `f19: u32` with a comment noting
*"byte\[2\]: fallback course/getMapRevisionId"*. Sauce decodes this
as `auxCourseId = bits >>> 16 & 0xff` (`zwift.mjs:228`).

**Implication for C2 and C4:** when we read the watched athlete's
`PlayerState` to learn their `courseId`, we can't read a `course_id`
field because there isn't one. We have to decode `f19 >> 16 & 0xff`.
And when we send our own heartbeat with `courseId`, we have to pack
it back into `f19` via `encodePlayerStateFlags1`. Easy to overlook
with auto-complete pulling up `state.course_id` and finding nothing.

## 4c. Round-3 findings (the bits Round-2 also missed)

Round 2 stopped at the GameMonitor flow. Round 3 walked the
constants, the `ZwiftAPI` HTTP layer, and a side-by-side of
sauce's `zwift.proto` vs the generated zoffline proto we use.
What that surfaced is uncomfortably long, and includes what is
quite possibly the actual root cause of the live trace's
`Connection refused`.

### C5 — UDP port 3022 (plaintext) vs 3024 (secure). Probable root cause of the live failure.

**Where:** `src/daemon/relay.rs::pick_initial_udp_target` reads
`a.port` from the `RelayAddress` and uses it as the connect port,
falling back to `UDP_PORT_SECURE` (3024) only when `port` is
absent or zero.

**What sauce does** (`zwift.mjs::UDPChannel.establish` line 1338):

```js
this.sock.connect(3024, this.ip, …)
```

**Hardcoded 3024.** Sauce ignores whatever port the proto carries.

**Why it matters:** sauce's own `zwift.proto` declares **two**
ports on a UDP server entry:

```proto
message UDPServer {
    int32 realm = 1;
    int32 courseId = 2;
    string ip = 3;
    int32 port = 4;          // default 3022  ← plaintext
    float xBound = 5;
    float yBound = 6;
    float xBoundMin = 7;
    float yBoundMin = 8;
    int32 securePort = 9;    // default 3024  ← encrypted (sauce uses this)
}
```

Zwift populates **both**. `port` (tag 4) is the plaintext UDP port
(3022 by default); `securePort` (tag 9) is the encrypted port (3024
by default). Sauce ignores both proto fields and hardcodes 3024.

Our zoffline-derived proto declares `RelayAddress` only up through
tag 6 (no `xBoundMin`, `yBoundMin`, or `securePort`). We read tag 4
(`port`) — the **plaintext** port — and connect there with our
**encrypted** AES-128-GCM-4 hellos. The plaintext server has no
session for us, the kernel emits ICMP Port Unreachable, and the OS
surfaces it as **`os error 61: Connection refused`** — exactly the
failure mode in the live trace.

This is independently probable as the proximate cause, before C1
or C2 even matter: even if C1 picked the right pool and C2 gated
on the right courseId, sending encrypted hellos to the plaintext
port would still surface `Connection refused`.

**Fix sketch:** drop the `a.port` read entirely. Always use
`zwift_relay::UDP_PORT_SECURE` (3024) for the UDP target's port.
Mirrors sauce's hardcoded 3024.

### C6 — Missing `Platform: OSX` header on every HTTP request

**Where:** `crates/zwift-api/src/lib.rs::ZwiftAuth::post`,
`ZwiftAuth::fetch`, `ZwiftAuth::get_profile_me`, `ZwiftAuth::login`.
None of them set a `Platform` header.

**What sauce sends** on every authenticated request
(`zwift.mjs::ZwiftAPI.fetch` line 456-460):

```js
const defHeaders = {
    'Platform': 'OSX',
    'Source': 'Game Client',
    'User-Agent': 'CNL/3.44.0 (Darwin Kernel 23.2.0) zwift/1.0.122968 game/1.54.0 curl/8.4.0'
};
```

**Why it matters:** Zwift's API is suspected to inspect headers and
filter / degrade non-game-client traffic. We don't *know* what
Zwift does with a missing `Platform` header — it may silently
omit fields from the response that an OSX game client would
receive (e.g. `udp_config_vod_1` could be empty or absent for a
"web client" identity).

**Fix sketch:** add `platform: String` to `Config` (default
`"OSX"`), set the header on every authenticated send.

### C7 — Weak `User-Agent` (`CNL/4.2.0` vs sauce's full game-client string)

**Where:** `crates/zwift-api/src/lib.rs::DEFAULT_USER_AGENT`.

```rust
pub const DEFAULT_USER_AGENT: &str = "CNL/4.2.0";
```

**What sauce sends:**

```
'CNL/3.44.0 (Darwin Kernel 23.2.0) zwift/1.0.122968 game/1.54.0 curl/8.4.0'
```

**Why it matters:** same as C6 — Zwift may pattern-match on the
User-Agent and silently degrade responses for clients that don't
look like the official game. The real game's UA mentions Darwin
kernel, the Zwift app version, the game module version, and the
curl version it was built against. Our terse `CNL/4.2.0` plausibly
trips a "this is not a game client" filter.

**Fix sketch:** update `DEFAULT_USER_AGENT` to the full sauce
string. The exact values are public (sauce ships them); they
don't need to be impersonated more carefully than they already
are.

### C8 — Protobuf `Content-Type` missing `; version=2.0`

**Where:** `crates/zwift-relay/src/consts.rs`:

```rust
pub const PROTOBUF_CONTENT_TYPE: &str = "application/x-protobuf-lite";
```

**What sauce sends** (`zwift.mjs::ZwiftAPI.fetch` line 445):

```js
if (options.pb) {
    options.body = options.pb.finish();
    headers['Content-Type'] = 'application/x-protobuf-lite; version=2.0';
}
```

The `; version=2.0` parameter is part of the Content-Type, not
just decorative. Zwift's API gateway may use it for codec selection
and reject / degrade requests without it.

**Fix sketch:** add `; version=2.0`. One-character change in the
constant.

### C9 — Course ID lives in `PlayerState.world` (tag 35), not in `f19` aux bits

**Where:** mostly future work — neither C2 nor C4 is implemented
yet, but the obvious-looking field name is wrong.

**What sauce reads** for `state.courseId`: the `PlayerState.courseId`
proto field at tag 35 (sauce's proto):

```proto
int32 courseId = 35;
```

The same wire field is `world: Option<i32>` in the zoffline-
generated Rust:

```rust
#[prost(int32, optional, tag = "35")]
pub world: ::core::option::Option<i32>,
```

There is also an `auxCourseId` packed in `PlayerState.f19` bits
16-23, which sauce decodes via `decodePlayerStateFlags1Into`. But
the canonical `courseId` is the standalone tag-35 int32, **not**
the packed aux value.

**Why it matters:** when C2 lands and we call `get_player_state(id)`,
the field to read for the course is `state.world` (despite the
misleading name). When C4 lands and we put `courseId` into the
heartbeat, the field to set is `state.world = Some(course_id)`.

A reviewer expecting `state.course_id` will find nothing on
PlayerState (it doesn't exist as a top-level field). The name
`world` in the zoffline schema is a documentation pitfall.

**Fix sketch:** breadcrumb in the C2/C4 implementation pointing at
`PlayerState.world` as "tag 35, sauce's `courseId`". Optionally
re-export it under a clearer alias in `zwift-proto` (e.g. a `pub fn
course_id(&self) -> Option<i32>` extension).

### C10 — `y` and `z` are swapped between zoffline and sauce naming

**Where:** PlayerState proto field naming.

**Sauce proto:** `float x = 25; float z = 26; float y = 27;`
**Zoffline proto:** `pub x: ... tag 25; pub y_altitude: ... tag 26;
pub z: ... tag 27;`

The wire tags are stable; the names are swapped. To send sauce's
`{x, y, z}`, our daemon writes `{x, y_altitude (= sauce's z), z (=
sauce's y)}`. For the heartbeat-with-zeroes case it doesn't
matter (all three are 0). For any future code that touches
positions it absolutely does.

**Fix sketch:** comment in the heartbeat-build site warning about
the swap. Also worth a `pub fn x_y_z(&self) -> (Option<f32>,
Option<f32>, Option<f32>)` extension in `zwift-proto` that returns
the canonical `(x, y, z)` per sauce's naming.

### C11 — Missing proto fields on `RelayAddress`: `xBoundMin`, `yBoundMin`, `securePort`

**Where:** zoffline-generated `RelayAddress` only declares tags
1-6. Sauce's `UDPServer` (same wire structure) declares tags 1-9.

The missing tags:
- `xBoundMin` (tag 7, float) — needed for `find_best_udp_server`'s
  bounding-box check.
- `yBoundMin` (tag 8, float) — same.
- `securePort` (tag 9, int32, default 3024) — addressed by C5
  (we hardcode 3024 anyway).

**Why it matters:** Zwift's protobuf wire format includes these
tags; prost will silently ignore unknown fields, so we lose them
entirely. For C5 (port) the hardcode-3024 fix sidesteps the
missing field. For the bounding-box selection this is a
prerequisite for the full pool-routing work (which is already
deferred to a future STEP).

**Fix sketch:** patch the vendored proto to add the missing fields,
or add a thin Rust extension. Defer until pool routing actually
lands; for now the hardcode-3024 in C5 is what matters.

### C12 — `watching_rider_id` is int64 in zoffline, int32 in sauce (tag 28)

**Where:** `PlayerState.watching_rider_id: Option<i64>` (tag 28)
in our generated proto; `int32 watchingAthleteId = 28;` in sauce's.

Probably tolerated (protobuf varints are width-agnostic up to the
declared bound). Worth knowing about if a wide athlete-ID ever
shows up and the upper bits get truncated; not blocking.

## 4d. Round-4 findings (proto-schema diffs and HTTP-detail diffs)

A side-by-side `awk` of `sauce4zwift/src/zwift.proto` vs the
zoffline-generated Rust surfaced more wire-format and HTTP-detail
divergences. None individually as catastrophic as C5 (port 3022),
but the `ClientToServer` body shape (N1) is potentially a
silent-rejection trigger that's worth pinning before declaring the
trace fixed.

### N1 — `ClientToServer` hello body sends extra fields sauce omits

**Sauce's UDP hello body** (`zwift.mjs::UDPChannel.establish` line 1388):

```js
this.sendPacket({
    athleteId: this.athleteId,
    realm: 1,
    worldTime: 0,
}, {hello: true});
```

`makeDataPBAndBuffer` adds `seqno: this._sendSeqno++` (starts at
0). So the wire-encoded fields are:

- `realm` (tag 1) = 1
- `athleteId` (tag 2)
- `worldTime` (tag 3) = 0
- `seqno` (tag 4) = 0 / 1 / 2 / …

Four fields. Nothing else.

**Our UDP hello body** (`crates/zwift-relay/src/udp.rs::build_hello`):

```rust
zwift_proto::ClientToServer {
    server_realm: 1,
    player_id: athlete_id,
    world_time: Some(0),
    seqno: Some(app_seqno),
    state: zwift_proto::PlayerState::default(),  // ← extra
    last_update: 0,                              // ← extra
    last_player_update: 0,                       // ← extra
    ..Default::default()
}
```

We send three additional fields. Two of them (`last_update` tag 10,
`last_player_update` tag 12) are **`required` in the zoffline-derived
proto2 schema** so the prost-generated code forces us to populate
them. Sauce's proto schema treats tag 10 as deprecated and tag 12
as `largestWorldAttributeSequenceNumber` (a different concept) —
sauce never sends either.

`state: PlayerState::default()` (tag 7) encodes as a zero-length
sub-message. Required-by-our-schema, absent in sauce's wire bytes.

**Why it matters:** the server may pattern-match on the wire format
of an "expected" hello packet. Three extra varints + a zero-length
sub-message could trip a "this is not a real game client" filter
in the same way C6/C7/C8 do for HTTP. Or it could be tolerated.
Without an actual decode of what sauce sends and what we send to
compare side-by-side, we can't *prove* the server cares — but
sending things sauce doesn't send is a known divergence.

**Fix sketch:** the cleanest fix requires forking the vendored
proto to drop the `required` markers on tags 10 and 12, then
omitting them from `build_hello` (and from every other `ClientToServer`
construction). A larger surface change. Defer until after C5/C6/C7/C8
are tested live and we know whether a header/port fix is enough.

### N2 — TCP and UDP share a single `connId` counter; sauce uses two

**Where:** `src/daemon/relay.rs::next_conn_id` increments a
single `static CONN_ID_COUNTER: AtomicU32`.

**What sauce does** (`zwift.mjs::NetChannel.getConnInc` line 1036):

```js
static getConnInc() {
    return this._connInc++ % 0xffff; // Defined by subclasses so tcp and udp each have their own counter
}
…
class TCPChannel extends NetChannel {
    static _connInc = 0;
…
class UDPChannel extends NetChannel {
    static _connInc = 0;
```

Each subclass has its own static counter. A fresh process has
TCP `connId=0` and UDP `connId=0` — same value, different
counters.

Our daemon: TCP `connId=N`, UDP `connId=N+1` (or +2 because of
serialization).

**Why it matters:** the IV input includes `connId`. If the server
maps a session by `(relayId, connId, channelType)` triple, the
TCP and UDP halves of the same daemon session look "more
unrelated" than sauce's do. Probably tolerated, but a divergence.

**Fix sketch:** split into two atomics
(`TCP_CONN_ID_COUNTER`, `UDP_CONN_ID_COUNTER`); each starts at
0. One-line change.

### N3 — Token endpoint: missing `Accept: application/json`

**Where:** `crates/zwift-api/src/lib.rs::ZwiftAuth::login` POST to
the Keycloak token endpoint sets only `Content-Type:
application/x-www-form-urlencoded`.

**What sauce does** (line 340-352, via `accept: 'json'` option that
fetch maps to `'Accept': 'application/json'`):

```js
const r = await this.fetch('/auth/realms/zwift/protocol/openid-connect/token', {
    …
    accept: 'json',
    body: new URLSearchParams({…})
});
```

**Why it matters:** Keycloak defaults to JSON for token responses,
so the Accept header isn't strictly required. But sauce sends it
and we don't. Possible filter trigger.

**Fix sketch:** add `.header("Accept", "application/json")` to
`login` and `do_refresh`.

### N4 — Protobuf requests: missing `Accept: application/x-protobuf-lite`

**Where:** `ZwiftAuth::post` (used for relay-session login + refresh).

**What sauce does** (from `accept: 'protobuf'` in `fetchPB`):

```js
headers['Accept'] = 'application/x-protobuf-lite';
```

We send no Accept header on protobuf requests. Same risk class as
C6/C7/C8 — Zwift may filter on missing/unexpected Accept.

**Fix sketch:** when `content_type` starts with
`application/x-protobuf-lite`, set `Accept` to the same.

### N5 — TCP hello uses `seqno: 1`, sauce starts at 0

**Where:** `src/daemon/relay.rs::start_all_inner` step 8:

```rust
tcp_sender.send_packet(
    zwift_proto::ClientToServer {
        …
        seqno: Some(1),  // ← hardcoded 1
        …
    },
    true,
).await?;
```

**What sauce does:** sauce's `_sendSeqno = 0` then `_sendSeqno++`
in `makeDataPBAndBuffer`. First packet sent has `seqno = 0`,
second has `seqno = 1`, etc.

**Why it matters:** off-by-one in the app-level seqno. Probably
tolerated by the server (it's just a sequence number for the
client's bookkeeping), but it's wrong.

**Fix sketch:** `seqno: Some(0)` for the first TCP send, then any
follow-up TCP sends auto-increment from 1.

### N6 — Inbound `worldUpdates` are not deduplicated by `ts`

**Where:** `src/daemon/relay.rs::recv_loop` `Inbound` arm consumes
`stc.updates` only as broadcast input for `GameEvent::PlayerState`;
nothing reads `stc.updates[*]` (the WorldAttribute list).

**What sauce does** (line 2172-2206):

```js
if (pb.worldUpdates.length) {
    for (let i = 0; i < pb.worldUpdates.length; i++) {
        …
        if (wu.ts <= this._lastWorldUpdate) {
            dropList.push(i);
            debugger;
            continue;
        }
        this._lastWorldUpdate = wu.ts;
        …
    }
    if (dropList.length) {
        for (let i = dropList.length - 1; i >= 0; i--) {
            pb.worldUpdates.splice(i, 1);
        }
    }
}
```

Sauce drops world updates whose `ts` is not strictly newer than
the last one seen, and updates `_lastWorldUpdate` to the highest
seen.

**Why it matters:** prerequisite for L3 (`_lastWorldUpdate`
tracking) which feeds M2 (`larg_wa_time` in TCP hello). Without
this we'll send 0 forever and re-process duplicate world updates.
Not a connect blocker; matters for stats correctness and for
reconnect.

**Fix sketch:** in the recv-loop's `Inbound` arm, walk
`stc.updates` and update `inner.last_world_update_ts` to
`max(current, wa.timestamp.unwrap_or(0))`. The dedup itself is
deferred to the per-athlete data-model STEP.

### N7 — Tag-10 / tag-12 schema interpretation diverges

`ClientToServer` tags 10 and 12 are **required** in the
zoffline-derived proto and named `last_update`, `last_player_update`.
Sauce's proto:

- Tag 10: `// deprecated int64 worldAttributesLastUpdated = 10;`
- Tag 12: `int64 largestWorldAttributeSequenceNumber = 12;`

We send `0` for both on every packet (forced by `required`). For
fresh connect this matches a "client has seen nothing" state and
is fine. For reconnect, sending 0 tells the server to re-flood
every world attribute from session start, which is wrong. Same
bucket as M2 (reconnect-only).

**Fix sketch:** track `largest_wa_seqno` alongside `_lastWorldUpdate`
and populate `last_player_update` (tag 12) from it. The deprecated
tag 10 stays at 0.

### N8 — `expungeReason` on incoming `ServerToClient` is ignored

**Where:** `src/daemon/relay.rs::recv_loop` doesn't read
`stc.expunge_reason`.

**What sauce does:** `pb.expungeReason` is checked at sauce's
proto level via the `ExpungeReason` enum; sauce doesn't act on it
in `onInPacket` either, but the proto field is at least named.

Our proto has `expunge_reason: Option<i32>` (tag 26) with
comment "tag464 UdpClient::receivedExpungeReason". The enum
values include common reasons like `MULTIPLE_LOGINS`,
`SERVER_FULL`, `KICKED`, etc.

**Why it matters:** the server can tell us *why* it's about to
disconnect us. Ignoring this means we can't distinguish "you
have multiple logins" (try again) from "you're banned" (stop
trying). Not a connect blocker; matters for retry/backoff
behaviour and operator diagnostics.

**Fix sketch:** in the recv-loop's `Inbound` arm, log
`relay.tcp.expunge_reason` (info or warn) when
`stc.expunge_reason.is_some()`. Defer any reaction logic to a
follow-up.

### N9 — No clean `logout` / `leave` on shutdown

**What sauce does:**

```js
async leave() {
    return await this.api.fetchJSON('/relay/worlds/1/leave', {method: 'POST', json: {}});
}
async logout() {
    const resp = await this.api.fetch('/api/users/logout', {method: 'POST'});
    …
}
```

Both are exposed as methods on `GameMonitor`'s `api` instance.
Sauce calls them (or expects callers to call them) on a clean
shutdown so the server-side session isn't left lingering.

Our daemon's shutdown flow calls neither.

**Why it matters:** the relay session (90-minute lifetime)
remains valid on the server side after a `ranchero stop`. Within
that window, restarting the daemon may collide with the lingering
session (Zwift may still think there's an active "game" with
stale state). Also a possible `multipleLogins` trigger (N8).

**Fix sketch:** on shutdown, before closing the TCP channel,
call `auth.post("/relay/worlds/1/leave", "application/json", b"{}".to_vec())`
and `auth.post("/api/users/logout", "application/json", vec![])`.
Best-effort; ignore failures (the daemon is exiting anyway).

## 4e. Round-5 findings (the hello-ack matcher reads the wrong field)

Round 4 was about HTTP detail and proto field-name divergences.
Round 5 traces what those divergences mean for actual code paths.
One of them is a critical bug that would prevent UDP sync from
ever converging even after every other C-block fix lands.

### N10 — UDP hello-ack matcher reads tag 4 instead of tag 5

**The most critical finding since C5.** Independently sufficient
to keep the daemon broken after C1+C2+C5 are all fixed.

**Where:** `crates/zwift-relay/src/udp.rs` line 311:

```rust
if let Some(ack) = stc.seqno {
    let ack_u32 = ack as u32;
    if let Some(sent_at) = send_times.remove(&ack_u32) {
        let local_now = clock.now();
        let latency = (local_now - sent_at) / 2;
        // …compute SNTP offset…
    }
}
```

We match `stc.seqno` (proto tag 4) against the map of pending
hello seqnos.

**What sauce does** (`zwift.mjs::UDPChannel.establish` line 1351):

```js
const sent = syncStamps.get(packet.ackSeqno);
```

Sauce reads `packet.ackSeqno` (proto tag 5).

**The proto schemas:**

- Sauce's `ServerToClient`: `int32 ackSeqno = 5; // UDP ack to our previously sent seqno`
- Zoffline's `ServerToClient`: `pub stc_f5: Option<i32> tag = 5` with comment
  *"low-priority world time sync algo (not investigated yet, maybe deprecated)"*

The field at tag 5 is the ACK seqno — the server echoes back
the seqno of the hello it's acknowledging. Sauce reads tag 5 to
match acks to outgoing hellos. We read tag 4 (the server's own
outgoing seqno), which has no relationship to our outgoing hello
seqno.

**Why it matters:** the hello-ack matcher decides whether to add
a sample to the SNTP buffer. We currently add a sample whenever
`stc.seqno` (the server's own seqno) happens to match a value
in our `send_times` map — which is purely coincidental, like
matching email subjects to telephone numbers. Statistically:

- The server's `seqno` increments per server-side message.
- Our `app_seqno` map keys are the values 0, 1, 2, … (one per
  hello sent).
- Match probability: nonzero only by accident.

When the matcher coincidentally hits a match, the latency /
offset values fed into `compute_offset` are nonsense (timestamps
of unrelated events), so even if convergence triggers, the
adjusted clock is wrong.

In practice the daemon would hit `MAX_HELLOS=25` exhausted
without convergence, and `establish` would return
`Error::SyncTimeout`. The trace would show 25
`relay.udp.hello.sent` events and zero `relay.udp.hello.ack`
events even though the server is actively sending acks back —
because we're ignoring the field that carries them.

**This is the second blocker after C5.** Fix is one line.

**Fix sketch:**

```rust
if let Some(ack) = stc.stc_f5 {  // was stc.seqno
    let ack_u32 = ack as u32;
    …
}
```

Or, better, fork the proto / rename the field via a thin
extension so the daemon code reads `stc.ack_seqno()` and the
intent is self-documenting. The C9/C10/C11 schema-renaming
breadcrumbs apply here too.

### N11 — UDP recv tracing logs the wrong field for `player_count`

**Where:** `crates/zwift-relay/src/udp.rs` line 620:

```rust
tracing::debug!(
    target: "ranchero::relay",
    world_time = stc.world_time.unwrap_or(0),
    player_count = stc.player_states.len(),
    payload_size = plaintext.len(),
    "relay.udp.message.recv",
);
```

`stc.player_states` is zoffline's name for **tag 28**, which is
sauce's `blockPlayerStates` — a *filtered* list ("block this
player" / shadow-bans / power-up filtering on the server side).
The actual player-states list is at **tag 8**, which zoffline
correctly names `stc.states` (and which the daemon uses
elsewhere — see `src/daemon/relay.rs:1938` and `:1954`).

**Why it matters:** the `relay.udp.message.recv` event reports
`player_count = N` where N is the number of **blocked** players,
not the number of regular players in the message. An operator
reading the trace would think there are zero players when
there's a full server-side player list. Tracing-only bug — no
behavioural impact — but exactly the kind of mistake that makes
a `--debug` trace misleading.

**Fix sketch:** `player_count = stc.states.len()`.

### N12 — TCP hello carries `server_realm: 1`; sauce's TCP hello omits realm

**Where:** `src/daemon/relay.rs::start_all_inner` step 8 TCP
hello payload sets `server_realm: 1`.

**What sauce sends** (`zwift.mjs::activateSession` line 1895):

```js
session.tcpChannel.sendPacket({
    athleteId: this.athleteId,
    worldTime: 0,
    largestWorldAttributeTimestamp: this._lastWorldUpdate,
}, {hello: true});
```

No `realm` field. The TCP session is bound to a single relay
session on connect — sauce's design assumes the server knows the
realm from the bearer token and the relay-session login.

Contrast sauce's UDP hello (which DOES set `realm: 1`):

```js
this.sendPacket({
    athleteId: this.athleteId,
    realm: 1,
    worldTime: 0,
}, {hello: true});
```

UDP hello includes realm because UDP is stateless per-packet.

**Why it matters:** same risk class as N1. The server may or may
not care about an unexpected `realm` on a TCP hello. Probably
tolerated; flagged for completeness.

The root cause is the same as N1: zoffline marks `server_realm`
as `required`, so prost forces us to set it. Without forking the
proto, we can't omit it.

**Fix sketch:** part of N1's "fork the proto, drop required
markers" work. Don't fix in isolation.

## 4f. Round-6 findings (refinements; no new critical bugs)

Round 6 walked the same flow with an eye for things rounds 1-5
glossed over. Mostly confirmations and refinements; the new
material is one C2 refinement and one observation about dead code.

### R1 — C2 must call `get_player_state(cfg.watched_athlete_id)`, not the monitor's athlete ID

**Refinement to C2.** Sauce's `initPlayerState` (line 1700) calls
`getPlayerState(this.selfAthleteId)`, where:

- `this.athleteId` = `this.api.profile.id` — the OAuth account's
  ID (sauce's monitor account, our `auth.athlete_id()`).
- `this.selfAthleteId` = `options.selfAthleteId` — **the athlete
  the monitor account is watching** (configured externally;
  conceptually our `cfg.watched_athlete_id`).

So the courseId we need lives on the *watched* athlete's
PlayerState, not the monitor account's. The monitor account is
typically NOT in a game (it's a passive observer); fetching the
monitor's PlayerState would return 404 or an empty state.

**Why it matters:** if C2's `get_player_state` is wired to the
wrong athlete, it'll always return None for a real monitor
deployment and the daemon will suspend forever — even when
there's a perfectly valid watched athlete to follow.

**Fix sketch:** C2's `get_player_state` call must use
`cfg.watched_athlete_id`. If `watched_athlete_id` is None, the
daemon should refuse to start with a clear error
(`RelayRuntimeError::NoWatchedAthlete`), not silently suspend.

This refinement only affects the C2 implementation note; no
extra Na/Nb pair needed.

### R2 — `HeartbeatScheduler::next_payload` builds a `ClientToServer` whose all-but-`state` fields are dead code

**Where:** `src/daemon/relay.rs::HeartbeatScheduler::next_payload`
returns a `ClientToServer { state, server_realm, player_id,
world_time, seqno, ... }`. The caller (UDP heartbeat sink at
relay.rs:285) extracts only `payload.state` and passes it to
`UdpChannel::send_player_state(state)`. The other fields (and the
HeartbeatScheduler's own atomic `seqno` counter) are silently
discarded; the actual outgoing CTS is built inside
`UdpChannel::send_player_state` with the channel's own
`SendState.app_seqno`.

**Why it matters:** confusing to read, but harmless. The
HeartbeatScheduler's `seqno: AtomicU32` is a vestigial counter
that never affects any wire bytes.

**Fix sketch:** during C4, simplify `HeartbeatScheduler` to build
just a `PlayerState` (not the wrapping CTS). One small refactor.

### Confirmed-correct items (no fix needed)

The following were re-checked in round 6 and verified consistent
with sauce:

- `RelayIv` byte layout (`device, channel, conn_id, seqno` BE
  encoded into bytes 2-11 of a 12-byte IV). ✓
- `HeaderFlags`: `RELAY_ID = 4, CONN_ID = 2, SEQNO = 1`. ✓
- `TCP_VERSION = 2, UDP_VERSION = 1`. ✓
- TCP plaintext envelope: `[2, hello?0:1, ...proto]`. ✓
- UDP plaintext envelope: `[1, ...proto]`. ✓
- TCP wire frame: `[BE u16 size][header][cipher+tag]`. ✓
- AES-128-GCM with 4-byte truncated tag. ✓
- `WorldTimer::now() = unix_now_ms - ZWIFT_EPOCH_MS + offset`. ✓
- `ZWIFT_EPOCH_MS = 1_414_016_074_400`. ✓
- `MIN_SYNC_SAMPLES = 5` (strict >5 = at least 6 samples). ✓
- `MIN_REFRESH_INTERVAL = 3 s`. ✓
- `SESSION_REFRESH_FRACTION = 0.90`. ✓
- TCP `next_tcp_frame` partial-frame handling (length prefix
  reassembly). ✓
- TCP recv-side: `payload_owned` is the de-prefixed body; IV
  state advances per-frame. ✓
- Connected UDP socket (`socket.connect(addr)` puts the UDP
  socket in connected mode; sauce's `sock.connect(3024, ip)`
  does the same). ✓
- TCP/UDP plaintext envelopes are asymmetric on the recv side
  (sauce's `_onUDPData` and `_onTCPData` both decode
  `protos.ServerToClient.decode(plain)` directly without
  stripping a version prefix; our recv path does the same). ✓
- TCP/UDP hello loops use `transport.connect(addr)` rather than
  passing the addr per-send. ✓
- Token endpoint path: `/auth/realms/zwift/protocol/openid-connect/token`. ✓
- Relay-session login path: `/api/users/login`. ✓
- Relay-session refresh path: `/relay/session/refresh`. ✓
- `LoginRequest.key` is the AES key (16 random bytes). ✓
- `LoginResponse.relay_session_id` → `RelaySession.relay_id`. ✓
- TCP-server filter: `lb_realm == 0 && lb_course == 0`. ✓
- TCP port hardcoded to 3025 (`TCP_PORT_SECURE`). ✓
- OAuth token form fields: `client_id`, `grant_type`, `username`,
  `password`. ✓ (`client_id = "Zwift Game Client"`, with the
  literal space.)
- Token refresh half-life: `expires_in / 2`. ✓

The codec layer is clean. The gaps are entirely in the orchestration
(start_all_inner, recv_loop, heartbeat) and in the HTTP-detail layer
(headers, content-type, accept).

## 4g. Round-7 findings (substantive — found another live-critical bug)

Round 6 declared "diminishing returns" prematurely. Round 7 found
N13 — a clock-correction loss that probably matters for whether
heartbeats are accepted by the server.

### N13 — SNTP-corrected `WorldTimer` offset is silently lost between hello loop and heartbeat scheduler

**Where:** `src/daemon/relay.rs::start_all_inner` lines 1276 and 1296:

```rust
// Step 9: UDP establish
let world_timer = zwift_relay::WorldTimer::new();   // ← instance A, offset=0
let (udp_channel, _) = zwift_relay::UdpChannel::establish(
    udp_transport, &session, world_timer, udp_config
).await.map_err(...)?;
// ↑ instance A is moved into establish(). Inside the hello loop,
// `clock.adjust_offset(-mean_offset_ms)` mutates instance A's
// shared Arc<Mutex<State>>. After establish() returns, the local
// `clock` binding is dropped — the LAST surviving Arc.

// Step 10: heartbeat scheduler
let heartbeat_world_timer = zwift_relay::WorldTimer::new();  // ← instance B, fresh offset=0
let scheduler = HeartbeatScheduler::new(sink, heartbeat_world_timer, athlete_id);
```

**What sauce does** (`zwift.mjs:125`):

```js
export const worldTimer = new WorldTimer();
```

A **single global** `worldTimer` shared by:
- The UDP hello loop (`UDPChannel.establish`, calls
  `worldTimer.adjustOffset(-meanOffset)` after convergence).
- `broadcastPlayerState` 1 Hz heartbeats (`worldTimer.now()`).
- `_refreshStates` (`worldTimer.serverNow()`).
- All `processPlayerStateMessage` calls.

After SNTP convergence, every read goes through the corrected
clock.

**Why it matters:** Zwift's UDP server cross-checks `world_time`
on incoming `ClientToServer` packets against its own clock. The
hello-loop sync exists *specifically* so the client's reported
`world_time` matches what the server expects. Sauce's worldTime
offset can be tens or hundreds of milliseconds (it's measured by
the SNTP-style algorithm to be exactly the round-trip-corrected
divergence between local clock and server clock).

When the heartbeat scheduler reads from `instance B` (offset=0),
its `world_time` value is the local clock's "ms since Zwift
epoch", **uncorrected** for the offset measured during hello sync.
For an in-sync local clock the divergence is small (a few ms);
for a clock that's 100ms or more off (common on virtualised
hosts, container networks, or after a system clock adjustment),
the heartbeat's `world_time` is off by that much.

If Zwift's server has a tolerance window (which is how SNTP-
style protocols typically work), heartbeats outside the window
get rejected as "stale" or "future". This may surface as silent
session drop, "multiple logins" errors (server thinks two sessions
are alive), or simply no inbound traffic in response.

**Why it wasn't caught earlier:** the codec layer is correct —
`WorldTimer::clone()` *would* share state via the `Arc<Mutex<…>>`
internal. The bug is in the daemon-level *plumbing*: we call
`WorldTimer::new()` twice and forget to clone-share. In a
synchronously-coupled design (sauce's global) this can't happen.

There's a *related* bug downstream of N13 — `HeartbeatScheduler::next_payload`
sets `world_time: Some(self.world_timer.now())` on the wrapping
CTS, but the caller (`UdpHeartbeatSink::send`) extracts only
`payload.state` and discards the CTS-level `world_time`.
`UdpChannel::send_player_state` then reads `state.world_time`
(which is `None` because `PlayerState::default()`) and forwards
it. Net effect: the heartbeat actually sends `world_time: None`,
not the (uncorrected) computed value. So even an uncorrected
read is currently dead code (R2). C4 must populate
`state.world_time = Some(corrected_clock.now())`.

**Fix sketch:** in `start_all_inner`, clone the `world_timer`
before moving it into `UdpChannel::establish`:

```rust
let world_timer = zwift_relay::WorldTimer::new();
let world_timer_for_heartbeat = world_timer.clone();  // ← shared Arc
let (udp_channel, _) = zwift_relay::UdpChannel::establish(
    udp_transport, &session, world_timer, udp_config
).await?;
…
let scheduler = HeartbeatScheduler::new(
    sink, world_timer_for_heartbeat, athlete_id,
);
```

And in C4's `next_payload` rewrite, populate `state.world_time =
Some(world_timer.now())` so the corrected value actually reaches
the wire.

**Bundle:** with C4 (heartbeat-content fix) — same site, same
test surface.

## 4h. Round-8 findings (supervisor re-login leaves stale channels)

### N14 — Supervisor re-login writes a fresh manifest but does not recreate TCP/UDP channels

**Where:** `src/daemon/relay.rs::start_all_inner` step 11 — the
spawned `supervisor_event_abort` task handles
`SessionEvent::LoggedIn(new_session)`:

```rust
Ok(zwift_relay::SessionEvent::LoggedIn(new_session)) => {
    tracing::info!(target: "ranchero::relay", "relay.session.logged_in");
    if let Some(writer) = writer_for_supervisor.as_ref() {
        writer.record_session_manifest(
            manifest_from_session(&new_session, session_conn_id),
        );
    }
}
```

We write a fresh manifest with the new session's AES key. We do
NOT recreate the TCP or UDP channels. The channels keep
encrypting and decrypting with the **OLD** session's `aes_key`
(captured at `TcpChannel::establish` time — see
`crates/zwift-relay/src/tcp.rs:185 aes_key = session.aes_key`).

**What sauce does:** sauce doesn't have a "re-login while keeping
channels" path at all. Its `refreshSession` only updates
`session.expires` (the relay session ID and AES key never change
on refresh — confirmed by sauce's
`message RelaySessionRefreshResponse { uint32 relaySessionId = 1;
uint32 expiration = 2; }`). If refresh actually fails, sauce
calls `_schedConnectRetry()` which goes through the full
`disconnect` + `connect` cycle, building brand-new TCP and UDP
channels with the new session's key.

Our supervisor's `refresh_loop`, by contrast, falls back to a
full `login()` (new aes_key, new relay_id) and emits
`SessionEvent::LoggedIn(new_session)` *while keeping the existing
channels alive*. After that event fires:

1. The daemon writes a manifest with the new key (so the capture
   file is decryptable past the rotation).
2. The TCP channel keeps decrypting inbound bytes with the
   **old** key → every inbound packet returns
   `Error::AuthTagMismatch` → recv loop emits `RecvError` events.
3. The UDP channel keeps encrypting outbound heartbeats with the
   **old** key → server can't decrypt them → server drops the
   session.
4. The daemon appears to "recover" from the operator's
   perspective (the trace shows
   `relay.supervisor.relogin_ok`) but the data plane is dead.

**Why it matters:** for first-trace this doesn't fire (no
refresh attempt happens during start). For sustained operation,
any refresh failure (transient network blip, server-side timeout,
rate-limit) silently breaks the daemon.

**Why it's been hidden:** `SessionEvent::LoggedIn` semantically
implies "session re-established", but the implementation is just
a manifest log + broadcast. There's no plumbing back into
`start_all_inner`'s channel construction.

**Fix sketches** (pick one):
- **A — Sauce's approach (preferred for parity):** drop our
  in-place re-login from the supervisor. On refresh failure,
  emit `SessionEvent::RefreshFailed` and surface a typed
  `RelayRuntimeError::SessionLost` from the daemon's recv loop.
  Daemon then exits cleanly; an outer supervisor (currently
  absent — would be L5's connect-retry work) restarts via the
  full `start_all_inner` path.
- **B — In-place channel recreate:** keep our in-place
  re-login, but on `SessionEvent::LoggedIn(new_session)` the
  supervisor handler must:
  1. Shut down the existing TCP and UDP channels.
  2. Re-run steps 5-10 of `start_all_inner` (TCP connect +
     hello, wait for udp_config, UDP connect + hello loop,
     spawn new heartbeat).
  3. Replace the channel handles in `RelayRuntime`.

  This is invasive (changes the runtime's mutable state through
  Arc/Mutex layers) and probably worse than approach A.

**Bundling:** belongs with L5 (connect retry / reconnect). Mark
as "blocked-by-L5" since fixing it without proper reconnect
machinery makes things worse, not better.

## 4i. Round-9 — no new findings

Round 9 walked the rest of the spaces I could identify:

- The `TcpChannel.send_packet` app-seqno auto-increment policy —
  we documented in code that we don't auto-increment (caller-owns
  semantics) per a deliberate STEP-11 decision; sauce auto-
  increments via `_sendSeqno++`. For the daemon's only TCP send
  (the hello), this manifests as N5's seqno=1 hardcode. No new
  finding beyond N5.
- The AES-GCM-4 composition in `crates/zwift-relay/src/crypto.rs`
  — verified line-by-line against NIST SP 800-38D §7 and against
  the Node-derived known-answer test in `tests/crypto.rs`.
  Correct.
- The TCP recv-side IV state initialization
  (`recv_iv_conn_id = conn_id_init; recv_iv_seqno = 0;`) — matches
  sauce's `recvIV = new RelayIV({channelType: 'tcpServer', connId: this.connId})`
  with default `seqno = 0`.
- AES key randomness — `rand::thread_rng().fill_bytes()` is
  cryptographically secure (ChaCha20, OS-seeded). Matches sauce's
  `Crypto.randomBytes(16)`.
- Default relay/auth/api hosts, login/refresh paths,
  protobuf content-type prefix, OAuth grant fields, IV layout,
  header flag values, plaintext envelope structure (TCP and UDP),
  TCP frame wrapping, and recv-side decode order — all verified
  in earlier rounds and re-checked here.
- `SESSION_REFRESH_FRACTION = 0.90`, `MIN_REFRESH_INTERVAL = 3 s`,
  `CHANNEL_TIMEOUT = 30 s`, `MAX_HELLOS = 25`,
  `MIN_SYNC_SAMPLES = 5`, `ZWIFT_EPOCH_MS = 1_414_016_074_400` —
  all match sauce's values.
- `RelaySessionRefreshResponse` proto: refresh ONLY updates
  `expiration`, never the relay session ID — confirms our
  supervisor's "in-place re-login on refresh failure" is the
  buggy path (N14), not the refresh path itself.

The final tally now has C1–C8 and N10/N13 as live-trace
blockers; N14 as the highest-impact sustained-operation gap; and
the M-block + L-block items to round out a fully sauce-equivalent
client.

## 5. What was implemented vs what is needed for the daemon to run

| Phase | Item | Status | Live impact |
| --- | --- | --- | --- |
| 12.13 §3b | Wait for `udp_config*` push | Implemented | Necessary, not sufficient |
| 12.14 **C1** | Pick from `lb_course=0` pool | **Not implemented** | **Blocks UDP from connecting at all** |
| 12.14 **C2** | Read athlete `courseId` before UDP via `getPlayerState` | **Not implemented** | **Blocks UDP for non-active athletes** |
| 12.14 **C3** | Send post-establish player state with `watching_rider_id` | **Not implemented** | **UDP comes up but server sends nothing back** |
| 12.14 **C4** | Heartbeat carries `id`/`just_watching`/`watching_rider_id`/`courseId` | **Not implemented** | **Server drops the session after a few empty heartbeats** |
| 12.14 **C5** | Hardcode UDP port 3024; ignore `RelayAddress.port` | **Not implemented** | **Probable root cause of `Connection refused` in live trace** |
| 12.14 **C6** | Send `Platform: OSX` HTTP header | **Not implemented** | Likely degrades / filters Zwift API responses |
| 12.14 **C7** | Use full game-client `User-Agent` string | **Not implemented** | Same as C6 |
| 12.14 **C8** | Add `; version=2.0` to protobuf `Content-Type` | **Not implemented** | Same as C6 |
| 12.14 C9 | Read course from `PlayerState.world` (tag 35) | **Not implemented** | Documentation breadcrumb for C2 / C4 |
| 12.14 C10 | y/z naming swap between zoffline & sauce | **Not implemented** | Documentation; matters once positions are populated |
| 12.14 C11 | Add `xBoundMin`/`yBoundMin`/`securePort` to `RelayAddress` proto | **Not implemented** | Pre-req for full pool routing; C5 sidesteps `securePort` |
| 12.14 C12 | `watching_rider_id` int32 vs int64 (tag 28) | **Not implemented** | Tolerated; cosmetic |
| 12.14 N1 | `ClientToServer` hello sends extra `state`/`last_update`/`last_player_update` | **Not implemented** | Possible "not a real client" filter trigger |
| 12.14 N2 | TCP/UDP separate `connId` counters | **Not implemented** | Tolerated; cosmetic |
| 12.14 N3 | `Accept: application/json` on token request | **Not implemented** | Same risk class as C6 |
| 12.14 N4 | `Accept: application/x-protobuf-lite` on protobuf requests | **Not implemented** | Same risk class as C6 |
| 12.14 N5 | TCP hello `seqno: 1` should be `0` | **Not implemented** | Cosmetic; off-by-one |
| 12.14 N6 | Inbound `worldUpdates` not deduplicated (pre-req for L3) | **Not implemented** | Stats path; reconnect |
| 12.14 N7 | Tag 10/12 `last_update`/`last_player_update` value (depends on N6) | **Not implemented** | Reconnect re-flood |
| 12.14 N8 | `expungeReason` from server is silently ignored | **Not implemented** | Diagnostics; retry logic |
| 12.14 N9 | No `logout` / `leave` on shutdown | **Not implemented** | Server-side session lingers |
| 12.14 **N10** | Hello-ack matcher reads tag 4 (`stc.seqno`) instead of tag 5 (`stc_f5` = sauce's `ackSeqno`) | **Not implemented** | **SECOND BLOCKER after C5** — sync never converges |
| 12.14 N11 | UDP recv tracing reports `player_count = stc.player_states.len()` (tag 28 = blocked list) instead of `stc.states.len()` (tag 8) | **Not implemented** | Misleading `--debug` trace; no behavioural impact |
| 12.14 N12 | TCP hello carries `server_realm: 1`; sauce's TCP hello omits realm | **Not implemented** | Same root cause as N1 (proto-required) |
| 12.14 **N13** | SNTP-corrected `WorldTimer` offset is silently lost between hello loop and heartbeat | **Not implemented** | **Heartbeats send uncorrected `world_time` — server may drop session as stale** |
| 12.14 **N14** | Supervisor re-login writes new manifest but does not recreate channels (stale `aes_key`) | **Not implemented** | Sustained-op only — silent data-plane death after any refresh failure |
| 12.14 M1 | UDP hello iter 2+ keeps `relay_id`+`conn_id` | **Not implemented** | Lossy networks only |
| 12.14 M2 | TCP hello carries `larg_wa_time` (depends on L3) | **Not implemented** | Reconnect path only |
| 12.14 L1 | `_refreshStates` polling fallback | **Not implemented** | Data pipeline goes silent during quiet periods |
| 12.14 L2 | Suspend / resume on idle | **Not implemented** | Long-running flows; rate-limit risk |
| 12.14 L3 | `_lastWorldUpdate` timestamp tracking | **Not implemented** | Pre-req for M2 |
| 12.14 L4 | TCP server pinning across reconnects | **Not implemented** | Reconnect-stability path |
| 12.14 L5 | Connect retry with exponential backoff | **Not implemented** | Network blip kills the daemon |
| 12.14 L6 | Multi-UDP-channel grace-shutdown swap | **Not implemented** | Required once pool routing lands |
| 12.13 plan §6 | Mid-session pool updates wired into `recv_loop` | **Not implemented** | Required for course changes |
| 12.13 plan §4 | Per-watched-athlete UDP routing | **Not implemented** | Required for direct-server steady state |

**Critical path to the first working trace** (8 must-fix items):

- **C5** is the single most-likely root cause of the live
  `Connection refused`: connecting to plaintext port 3022 with
  encrypted hellos. Fix is one line.
- **C6 + C7 + C8** are HTTP-header impersonation. Zwift may be
  giving us a degraded `udp_config_vod_1` (or none) because we
  don't look like a game client.
- **C1 + C2** make the connect logic correct once C5 fixes the
  port.
- **C3 + C4** keep traffic flowing once the connect succeeds.

The C9–C12 items are documentation pitfalls / proto schema gaps
that matter for *implementing* C2/C4/pool-routing correctly. They
don't add new behaviour — they prevent the next implementer from
falling into the same naming / field-tag traps.

The L-rows are sustained-operation correctness work. None block
the first successful trace, but every one will bite once the
daemon is left running.

## 6. Summary checklist

In dependency order — each `Na` is failing tests, each `Nb` is the
implementation that makes them pass.

The **C-block (1–8)** is the minimum for a `--debug` trace to show
real inbound data flowing on UDP. Land all eight before declaring
success against live Zwift; any one missing leaves the trace stuck
at the obvious failure point downstream of it.

**Suggested ordering**: do C5 first (one-line fix; almost
certainly the live-trace blocker). Then C6/C7/C8 together (HTTP
header impersonation; small). Then C1, C2, C3, C4 together (the
flow correctness work).

- [ ] **1a** — Tests for **C5 + N10 + N2** (one combined pair —
  the UDP-connect critical path):
  - **C5**: a synthetic `udp_config` push with
    `RelayAddress.port = 3022` results in the daemon connecting to
    port **3024**, not 3022.
  - **N10**: an inbound hello reply carrying `stc_f5 = N` (the
    sauce-`ackSeqno` field, tag 5) advances the SNTP buffer for
    the matching outgoing hello; an inbound reply carrying only
    `seqno = N` (tag 4) does NOT match.
  - **N2**: the TCP and UDP `connId` counters are independent
    (each starts at 0 from a fresh process).
- [ ] **1b** — Implementation:
  - **C5**: in `pick_initial_udp_target`, drop the `a.port` read
    entirely; always use `zwift_relay::UDP_PORT_SECURE`. Mirrors
    sauce's hardcoded 3024 in `UDPChannel.establish`.
  - **N10**: in `udp.rs::establish` hello-loop, change `stc.seqno`
    to `stc.stc_f5` (or expose a typed `ack_seqno()` accessor).
    Also fix `udp.rs:620`'s **N11** — `player_count =
    stc.states.len()` instead of `stc.player_states.len()`.
  - **N2**: split `CONN_ID_COUNTER` into `TCP_CONN_ID_COUNTER`
    and `UDP_CONN_ID_COUNTER`; `next_tcp_conn_id()` /
    `next_udp_conn_id()` callers in `start_all_inner`.

- [ ] **2a** — Tests for **C6/C7/C8/N3/N4** (one combined pair —
  HTTP-header impersonation):
  - `auth.login` and `auth.do_refresh` send `Accept: application/json`
    on the token POST.
  - `auth.post` (used for relay-session login + refresh) sends
    `Accept: application/x-protobuf-lite`.
  - All authenticated requests carry `Platform: OSX`,
    the full game-client `User-Agent`, and (for protobuf bodies)
    `Content-Type: application/x-protobuf-lite; version=2.0`.
- [ ] **2b** — Implementation:
  - Add `platform: String` to `Config` (default `"OSX"`); set the
    header on every send including the token endpoint.
  - Replace `DEFAULT_USER_AGENT = "CNL/4.2.0"` with the full
    sauce string `"CNL/3.44.0 (Darwin Kernel 23.2.0)
    zwift/1.0.122968 game/1.54.0 curl/8.4.0"`.
  - Append `; version=2.0` to `PROTOBUF_CONTENT_TYPE`.
  - Set `Accept: application/json` on `login` + `do_refresh`.
  - Set `Accept: application/x-protobuf-lite` on `post` whenever
    `content_type` starts with that prefix.

- [ ] **3a** — Tests for **C1**: `extract_udp_servers` (or its
  replacement) preserves the `lb_course` discriminator; the daemon
  picks UDP target from the `lb_course=0` pool when both a generic
  pool and a per-course pool are present in the same push.
- [ ] **3b** — Implementation for C1: refactor
  `extract_udp_servers` to return per-course pools (rather than a
  flat list); update `start_all_inner` step 8.5 to look up the
  generic pool and pick its first server. Reject the push as
  insufficient (typed error) if no `lb_course=0` pool is present
  after a reasonable wait.

- [ ] **4a** — Tests for **C2**: when the watched athlete is not
  in a game (`get_player_state` returns `None` or `state.world ==
  None / 0`), the daemon does NOT call `udp_factory.connect()`;
  instead it logs `relay.runtime.suspended_no_course` and waits.
- [ ] **4b** — Implementation for C2: add a
  `get_player_state(athlete_id)` helper to `zwift-api` (HTTP GET
  `/relay/worlds/1/players/{id}` returning a parsed `PlayerState`);
  read course from `state.world` (tag 35 — see C9 breadcrumb,
  *not* the `f19` aux field); call from `start_all_inner` after
  auth login **with `cfg.watched_athlete_id`** (per R1 — sauce
  fetches the watched athlete's state, NOT the monitor account's);
  gate the UDP setup branch on the resulting course. If
  `cfg.watched_athlete_id` is None, refuse to start with a clear
  `RelayRuntimeError::NoWatchedAthlete`.

- [ ] **5a** — Tests for **C3**: after `UdpChannel::establish`
  returns Ok, the daemon issues exactly one `send_player_state`
  call carrying `id = athlete_id`, `just_watching = true`, and
  `watching_rider_id = watching_athlete_id`.
- [ ] **5b** — Implementation for C3: in `start_all_inner` step 10
  (immediately after the UDP channel is established, before the
  heartbeat scheduler), call
  `udp_channel.send_player_state(initial_state)` with the
  watching-athlete ID. Mirrors `establishUDPChannel` (line 2127).

- [ ] **6a** — Tests for **C4 + N13 + R2**: the heartbeat
  scheduler's outgoing PlayerState carries `id`, `just_watching =
  true`, `watching_rider_id`, the `world` (course) field, and a
  **non-zero `world_time`** that reflects the SNTP offset adjusted
  during UDP hello sync (i.e. the `WorldTimer` instance shared
  with `UdpChannel::establish`'s clock).
- [ ] **6b** — Implementation:
  - **N13**: in `start_all_inner` step 9, capture
    `world_timer.clone()` before moving the original into
    `UdpChannel::establish`; pass the clone to `HeartbeatScheduler`
    in step 10. Both refer to the same `Arc<Mutex<State>>` so the
    hello-loop's `adjust_offset` is visible to subsequent
    heartbeat reads.
  - **C4**: thread `watching_athlete_id` and `course_id` from
    `RuntimeInner.watched_state` into the `HeartbeatScheduler`;
    build the per-tick payload with `state.world_time =
    Some(world_timer.now())`, `state.id`, `state.just_watching =
    Some(true)`, `state.watching_rider_id`, and `state.world =
    Some(course_id)` (per C9 — sauce's `courseId` is at proto
    tag 35).
  - **R2**: while in this site, simplify `HeartbeatScheduler::next_payload`
    to build a `PlayerState` directly (not the wrapping CTS); the
    CTS-level fields it currently sets are all dead code.

The **M-block (7–8)** is correctness work that doesn't block the
first trace but should land in the same step:

- [ ] **7a** — Tests for M1: every UDP hello iteration emits a
  header carrying `RELAY_ID | CONN_ID | SEQNO`, not just SEQNO.
- [ ] **7b** — Implementation for M1: drop the `hello_idx == 1`
  special case in `build_send_header`; always emit the full triple
  for hellos.

- [ ] **8a** — Tests for M2 + L3: incoming `worldUpdates[*].ts`
  values advance a `last_world_update_ts: AtomicI64`; the next TCP
  hello reads that value into `larg_wa_time`.
- [ ] **8b** — Implementation for M2 + L3: add the atomic to
  `RuntimeInner` (read `wa.timestamp`, tag 14, on each inbound
  WorldAttribute); populate from the recv-loop's `Inbound` arm;
  thread the current value into the TCP hello in `start_all_inner`
  step 8.

The C9, C10, C11, C12 documentation breadcrumbs are baked into
items 4b, 6b, and 1b above as inline comments / type-extension
helpers — no separate Na/Nb pair needed.

The N1–N9 round-4 findings split as follows:
- **N1** (extra hello-body fields) — risky enough to fix in this
  STEP if C5+C6/7/8 don't unblock the trace. The fix requires
  forking the proto to drop `required` on tags 10/12, so it's a
  larger change. Defer to a follow-on Na/Nb pair only if needed.
- **N2** (separate connId counters) — one-line change; bundle
  with C5's `pick_initial_udp_target` fix in pair 1b.
- **N3, N4** (Accept headers) — bundled into pair 2 above.
- **N5** (TCP hello seqno=0) — one-line change; bundle with
  pair 5b (C3 implementation) since it's the same site.
- **N6, N7** (worldUpdate dedup + tag 10/12 reconnect values) —
  same scope bucket as M2 + L3. Bundle into pair 8.
- **N8, N9** (expungeReason logging, clean logout/leave) —
  defer to L-block follow-up STEP.

The **L-block** (suspend/resume, retry, multi-channel,
`_refreshStates`, TCP pinning, etc.) is a separate sustained-
operation cleanup that does not need to land before the first
successful live trace. List below in §7 for follow-ups, not in this
checklist.

## 7. Deferred to follow-ups (sustained-operation cleanup)

These are the L-rows from §5. Each blocks a different long-running
behavior, none blocks the first `relay.udp.message.recv`. Open as
separate STEPs after the C-block lands and the live trace shows
real data:

- **L1** — `_refreshStates` polling fallback (`get_player_state`
  on a self-tuning interval). Synthesizes "fake server packets"
  to keep the data pipeline fresh during UDP quiet periods.
- **L2** — Suspend / resume on idle. Auto-suspend after 15 s of
  no fresh self-state; auto-resume on incoming live data.
- **L4** — TCP server pinning across reconnects (`_lastTCPServer`).
- **L5** — Connect retry with exponential backoff
  (`1.2^backoffCount`).
- **L6** — Multi-UDP-channel with grace-shutdown swap (60 s
  reusable, 1 s otherwise).
- 12.13 plan §6 — Mid-session pool updates wired into `recv_loop`.
- 12.13 plan §4 — Per-watched-athlete pool selection /
  `find_best_udp_server` integration.
- 12.14 k3 — Portal-pool handling.
- 12.14 M3 / k1 — TCP non-hello flag=0 cleanup and TCP hello
  SEQNO=0 omission. Server tolerates both; cosmetic.
- Sauce's `_processIncomingPlayerState` / `_updateWatchingState`
  flag-bit decoding and downstream stats. Belongs in the
  per-athlete data-model STEP (13+), not here.

Each deferral is a deliberate choice: STEP-12.14 ships the minimum
for the first successful trace, with an explicit list of what to
look at next once that's working.

## 8. Implementation plan

The §6 summary checklist already lists the Na/Nb pairs for the
critical and material work (phases 1–8 in the critical-block fix
order from §0). This section adds the post-critical work as
batches A–E, following the same TDD pattern: each batch starts
with red-state tests, then green-state implementation that makes
those tests pass.

The §6 phases are reproduced here as a single ordered list so a
reader can see the full implementation arc in one place.

### Phase 1 — UDP target port + ack matcher + connId counters

Bundles **C5 + N10 + N2 + N11**. Single edit site
(`crates/zwift-relay/src/udp.rs` + `src/daemon/relay.rs`),
high-payoff one-liners. Probable root cause of the live trace's
`Connection refused`. See §6 pair 1 for the test list and
implementation sketches.

### Phase 2 — HTTP impersonation

Bundles **C6 + C7 + C8 + N3 + N4**. All in
`crates/zwift-api/src/lib.rs`. See §6 pair 2.

### Phase 3 — UDP pool selection

Bundles **C1**. Refactor `extract_udp_servers` →
`extract_udp_pools` to preserve the `lb_course` discriminator;
pick the initial UDP target from the `lb_course=0` (generic
load-balancer) pool. See §6 pair 3.

### Phase 4 — Course gate via `getPlayerState`

Bundles **C2 + R1 + C9**. Add an `auth.get_player_state(id)`
helper to `zwift-api`; call it from `start_all_inner` with
`cfg.watched_athlete_id` (per R1, NOT the monitor's
`auth.athlete_id()`); read course from `state.world` (tag 35,
per C9). Suspend instead of connecting UDP if the watched
athlete isn't in a game. See §6 pair 4.

### Phase 5 — Post-establish UDP send + TCP hello seqno

Bundles **C3 + N5**. After UDP convergence, send one
`PlayerState` with `watching_rider_id`, `id`, `just_watching`,
and `world` to register the session (sauce's
`establishUDPChannel` line 2127). Also fix TCP hello `seqno = 0`
(was 1). See §6 pair 5.

### Phase 6 — Heartbeat content + shared WorldTimer

Bundles **C4 + N13 + R2 + C10**. Heartbeat must populate
session context (`id`, `just_watching`, `watching_rider_id`,
`world`) and read `world_time` from the SHARED
SNTP-corrected `WorldTimer` (clone before moving into
`UdpChannel::establish`). Drop `HeartbeatScheduler::next_payload`'s
dead CTS-level fields (R2). See §6 pair 6.

### Phase 7 — UDP hello header consistency

Bundles **M1**. Drop the `hello_idx == 1` special case in
`build_send_header`; emit `RELAY_ID | CONN_ID | SEQNO` for every
hello iteration. See §6 pair 7.

### Phase 8 — Reconnect-state tracking

Bundles **M2 + L3 + N6 + N7**. Track `last_world_update_ts` and
`largest_wa_seqno` from inbound `WorldAttribute.timestamp`;
populate `larg_wa_time` (tag 13) and `last_player_update`
(tag 12) in TCP hello / steady-state CTS; deduplicate inbound
worldUpdates by `ts`. See §6 pair 8.

---

Phases 1–8 above must land **in order** — each unblocks the next
point in the live-trace failure chain. The batches below are
**independent** and can land in any order after Phase 8. Each is
a TDD pair (`Xa` writes failing tests, `Xb` makes them pass).

### Batch A — Live pool routing & multi-channel UDP

Covers **12.13 plan §4 + §6, L6, k3, k4, C11 partial**. Sauce
keeps `_udpServerPools` populated from every inbound
`udp_config_vod_*` push, runs `findBestUDPServer` whenever the
watched athlete moves, and switches UDP channels with a 60 s
grace period.

#### Aa — Tests (red state)

- `recv_loop_inbound_updates_pool_router_from_udp_config_push`
  — inject a TCP `Inbound(stc)` with a fresh `udp_config_vod_1`;
  assert `inner.pool_router` lock now contains the new pool
  keyed by `(realm, course)`.
- `pool_router_swap_emits_pool_swap_game_event` — change the
  watched athlete's `(realm, course, x, y)` such that
  `find_best_udp_server` picks a different server; assert
  `GameEvent::PoolSwap { from, to }` is broadcast on
  `game_events_tx`.
- `udp_channel_swap_runs_grace_shutdown_on_old_channel` —
  trigger a swap; assert the old UDP channel's
  `shutdown_and_wait` is called after a 60 s sleep (or
  whatever production grace setting) but the new channel takes
  over immediately.
- `portal_pool_handled_via_portal_key` — push a
  `udp_config_vod_1` with `portalPools` (or the zoffline-
  equivalent field once C11 partial adds it); assert the pool
  router accepts a `'portal'`-style key analogue.

#### Ab — Implementation (green state)

- `recv_loop` `Inbound` arm: call `extract_udp_pools(&stc)`; for
  each pool, build a `UdpServerPool` and call
  `inner.pool_router.lock().apply_pool_update(pool)`. Then call
  `inner.recompute_udp_selection()`.
- Implement `RelayRuntime::recompute_udp_selection` (currently a
  test stub):
  - Read `inner.watched_state` for `(realm, course, x, y)`.
  - Look up the `(realm, course)` pool, falling back to `(0, 0)`.
  - Call `find_best_udp_server(pool, x, y)`.
  - If the chosen server differs from `inner.current_udp_server`,
    broadcast `GameEvent::PoolSwap { from, to }` and trigger a
    UDP channel swap (see L6 below).
- L6: extend `RelayRuntime` to hold `Vec<Arc<UdpChannel<_>>>`
  (or a similar collection). On swap: spawn the new channel,
  schedule the old channel for 60 s grace shutdown via
  `tokio::spawn(async { sleep(60s); old.shutdown_and_wait().await; })`,
  promote the new channel as primary.
- Patch the vendored proto (or add a thin Rust extension) to
  expose `xBoundMin (tag 7)`, `yBoundMin (tag 8)`, and
  `securePort (tag 9)` on `RelayAddress` so
  `find_best_udp_server`'s bounding-box check has real data.

### Batch B — Connect retry & supervisor recovery

Covers **L5, N14, L4, N9**. A network blip currently kills the
daemon; supervisor in-place re-login leaves stale-key channels;
no clean logout/leave on shutdown.

#### Ba — Tests (red state)

- `start_failure_triggers_exponential_backoff_retry` — a
  start that fails (e.g. unroutable TCP) is retried up to N
  times with `1.2^attempt` backoff; trace shows
  `relay.runtime.connect_retry attempt=1 backoff_ms=…`.
- `tcp_server_pinned_across_reconnects` — first run picks
  `tcp_servers[0]`; second run (after a controlled disconnect)
  picks the **same** IP, even if the server list order has
  shuffled.
- `supervisor_relogin_recreates_channels_with_new_key` — inject
  a `SessionEvent::LoggedIn(new_session)` whose `aes_key`
  differs from the original. Assert: old TCP and UDP channels
  are shut down; new ones come up using `new_session.aes_key`;
  the recv loop is now decrypting against the new key.
- `clean_shutdown_sends_logout_and_leave` — daemon shutdown
  triggers `POST /api/users/logout` and `POST /relay/worlds/1/leave`.
  Both are best-effort; failures don't block exit.

#### Bb — Implementation (green state)

- L5: wrap `start_all_inner` in a retry loop in
  `start_with_writer` (or a higher-level
  `connect_with_retry` helper). Use exponential backoff
  `1000 ms × 1.2^attempt`, capped at 5 min. Surface a typed
  exhaustion error after N attempts (sauce doesn't cap; pick a
  sensible value like 50).
- L4: store the chosen TCP server IP in `RuntimeInner` (or a
  small state file). On reconnect, prefer the stored IP if
  present in the new server list, otherwise fall back to
  `servers[0]`.
- N14: pick **approach A (sauce parity)** — drop in-place
  re-login from the supervisor; emit
  `SessionEvent::SessionLost` instead. The outer retry loop
  (L5) handles the full reconnect cleanly.
  
  *(Alternative if approach B is preferred: recreate channels
  in place on `LoggedIn`. This requires holding `RuntimeInner`
  state — channels, abort handles — behind shared mutability
  and is significantly more invasive. Approach A is preferred.)*
- N9: add `auth.logout()` (`POST /api/users/logout`) and
  `auth.leave()` (`POST /relay/worlds/1/leave`) methods. On
  daemon shutdown, call both best-effort before exiting.

### Batch C — State-refresh fallback & suspend / resume

Covers **L1, L2**. Sauce's `_refreshStates` polls
`getPlayerState` on a self-tuning 3-30 s interval; the daemon
auto-suspends after 15 s of no self-state and auto-resumes on
incoming live data.

#### Ca — Tests (red state)

- `state_refresh_polls_get_player_state_on_self_tuning_interval`
  — drive the daemon; assert `get_player_state(watched_id)` is
  called every ~3 s initially. After 15 s of no incoming
  self-state, the polling interval expands toward 30 s.
- `daemon_suspends_after_15s_of_no_self_state` — silence the
  inbound stream for 15 s; assert `relay.runtime.suspended_idle`
  info fires; the heartbeat scheduler stops (or marks itself
  suspended).
- `daemon_resumes_on_incoming_self_state_when_suspended` —
  inject a fresh inbound state for `watched_athlete_id`; assert
  `relay.runtime.resumed` info fires; heartbeat re-arms.
- `state_refresh_synthesizes_fake_server_packet_from_polled_state`
  — the polled state must be broadcast as
  `GameEvent::PlayerState` (or an equivalent fake-packet event)
  so downstream consumers see it the same as if it had arrived
  on the wire.

#### Cb — Implementation (green state)

- New `StateRefresher` task spawned at `start_all_inner` step
  12 (alongside the recv_loop). Holds
  `_state_refresh_delay: Duration` (initially 3 s, expanding to
  30 s, capped at 5 min on errors). Uses `tokio::time::sleep`
  between polls.
- On each tick, call `auth.get_player_state(watched_id)`;
  branches:
  - `age < delay × 0.95`: stream is fresh, no action; reduce
    delay toward minimum.
  - `age > 15 s`: suspend; expand delay toward 30 s.
  - Error: expand delay (factor 1.15, cap 5 min).
- Suspend / resume: add `RuntimeInner::suspended: AtomicBool`.
  Suspend: set true; signal heartbeat scheduler to skip ticks.
  Resume: set false; heartbeat resumes.
- Inbound self-state in `recv_loop`'s `Inbound` arm: update
  `inner.last_self_state_updated_ms`. If suspended, call
  `resume()`.

### Batch D — Diagnostics & TCP-flag parity

Covers **N8, M3, k1, k2**. Operator-visibility improvements and
TCP flag-pattern parity with sauce. None of these change wire
behaviour against the server in any way the server actually
notices; they tighten our flag-emission to match what sauce
sends and add operator visibility.

#### Da — Tests (red state)

- `expunge_reason_is_logged_when_present` — inject a TCP
  `Inbound(stc)` with `expunge_reason = Some(MULTIPLE_LOGINS)`;
  assert `relay.tcp.expunge_reason variant=MultipleLogins` info
  fires.
- `tcp_non_hello_send_emits_no_seqno_flag_in_header` — drive a
  hypothetical TCP non-hello send (currently we don't have one;
  add a test path for it); the encoded header byte must equal
  `0x00` (no flags set).
- `tcp_hello_omits_seqno_flag_when_iv_seqno_is_zero` — first
  TCP hello has `iv_seqno = 0`; the encoded header must NOT
  include the SEQNO flag (just `RELAY_ID | CONN_ID`).
- `udp_config_v2_and_flat_fallback_paths_are_inert` — confirm
  the daemon ignores `udp_config_vod_2` and the flat
  `udp_config` (k2 parity). Negative test: inject a packet with
  only those fields populated; assert no pool-router update.

#### Db — Implementation (green state)

- N8: in `recv_loop`'s `Inbound` arm, if
  `stc.expunge_reason.is_some()`, emit
  `tracing::info!(target: "ranchero::relay", variant = ?stc.expunge_reason, "relay.tcp.expunge_reason");`.
- M3: in `tcp.rs::send_packet` non-hello branch, set
  `flags: HeaderFlags::empty(), seqno: None` so the encoded
  header is just one byte (`0x00`).
- k1: in `tcp.rs::send_packet` hello branch, conditionally
  include `SEQNO` only when `iv_seqno > 0` (mirroring sauce's
  `(options.hello && iv.seqno) || options.forceSeq`).
- k2: in `extract_udp_pools` (Phase 3 helper), return `None`
  if only `udp_config_vod_2` or flat `udp_config` are populated
  (sauce ignores both — only `udp_config_vod_1` triggers pool
  updates). Either drop the fallback paths entirely or keep
  them behind a feature flag for compat with self-hosted
  zwift-offline.

### Batch E — Proto fork: drop required markers, add missing fields

Covers **N1, N12, C11 full**. Our zoffline-derived proto marks
`ClientToServer.state` (tag 7), `last_update` (tag 10),
`last_player_update` (tag 12), and `server_realm` (tag 1) as
`required`. Sauce's proto schema treats them as optional /
absent. To match sauce's wire bytes exactly we need to fork the
proto so the prost-generated code lets us omit them.

#### Ea — Tests (red state)

- `tcp_hello_wire_bytes_omit_state_last_update_last_player_update`
  — capture the framed wire of a TCP hello; decrypt; assert the
  proto bytes do NOT include tag 7, 10, or 12.
- `tcp_hello_wire_bytes_omit_realm` — same, but assert tag 1
  (`server_realm`) is also absent (sauce's TCP hello carries
  no realm).
- `udp_hello_wire_bytes_match_sauce_minimal_form` — the UDP
  hello body contains exactly four wire fields: tag 1
  (`realm = 1`), tag 2 (`athleteId`), tag 3 (`worldTime = 0`),
  tag 4 (`seqno`). Nothing else.
- `relay_address_proto_carries_x_y_bound_min_and_secure_port`
  — round-trip a `RelayAddress` with all 9 tags populated;
  assert each field round-trips correctly via the patched proto.

#### Eb — Implementation (green state)

- Fork the vendored proto under
  `crates/zwift-proto/src/zwift_patched.proto` (or patch in
  place with a clear annotation); change `required` to
  `optional` on the relevant tags. Regenerate via
  `prost-build`.
- Update every `ClientToServer { … }` literal in the daemon
  and `zwift-relay::session` to omit the now-optional fields
  unless they have a non-default value.
- Add `xBoundMin`, `yBoundMin`, `securePort` to `RelayAddress`
  proto; regenerate; wire into `find_best_udp_server` (Batch A
  also depends on this).
- Audit: any prost decoder that previously returned an error
  on missing-required-field will now return `Ok` with default
  values. Verify this doesn't silently accept truly-malformed
  responses by adding manual presence checks in
  `extract_udp_pools` and the `LoginResponse` decode path for
  the fields we still expect to be set.

### Test infrastructure notes

- Phase 5 / Phase 6 tests need a way to extract the decoded
  `PlayerState` from captured outbound bytes. Reuse the
  `parse_outbound` helper already in
  `crates/zwift-relay/tests/udp.rs` (or factor it into a
  shared test helper module).
- Phase 4 tests need a stub `ZwiftAuth` whose
  `get_player_state` returns programmable results. The current
  `StubAuth` only exposes `login` and `athlete_id`; extend it
  with a `get_player_state` mock or use `wiremock` against the
  real `ZwiftAuth`.
- Batch C's suspend / resume tests need a way to silence the
  inbound stream and observe the heartbeat scheduler's state.
  Add a `HeartbeatScheduler::is_suspended()` test accessor.
- Batch B's reconnect tests need either a wiremock that fails
  N times then succeeds, or a higher-level retry-driver that
  can be tested in isolation from the daemon.

### Verification gate

After each phase / batch lands:

1. `cargo test --workspace` — green.
2. `ranchero start --debug --capture output.cap` against live
   Zwift — observe the trace progresses **past the previous
   failure point**:
   - Phase 1 unblocks the original `Connection refused`.
   - Phase 2 unblocks any HTTP-impersonation degradation.
   - Phases 3-4 unblock pool selection + course-gate suspension.
   - Phases 5-6 unblock the first inbound burst and steady
     traffic.
   - Phases 7-8 land hello / reconnect parity.
3. After Phase 8: live trace must show the full event sequence
   per STEP-12.12 §Acceptance, with at least one
   `relay.udp.message.recv` event carrying real `player_count`
   and `world_time` data.

The first 8 phases must land in order (each unblocks the next
trace failure). Batches A–E are independent; none of them
blocks the first successful trace.
