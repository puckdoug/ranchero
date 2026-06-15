# Step 22 — Athlete profile fetch driver + W′/zones configuration (G3 + G4)

Source: `review.md` findings **G3** and **G4**. Order-of-work item 2. G4
depends on G3 (the FTP that configures W′ and zones comes from the fetched
profile), so they are one step.

## Goal

Every athlete seen on course gets its profile (name, weight, FTP) fetched
from Zwift, cached, and written through to `athletes.sqlite`; the W′-balance
and power-zone accumulators are configured from that FTP. After this step the
published `athlete`, `tss`, `wBal`, and `timeInPowerZones` fields hold real
values instead of `null`/empty.

## Decision in force

**Q6 (2026-06-12): match sauce.** Fetch the profile of every athlete seen,
batched into multi-id requests, refreshed when stale. Not "watched only".

## Background the implementer needs

- `get_profiles` (batch, `crates/zwift-api/src/lib.rs:487-537`) and
  `get_profile` (`lib.rs:472-485`) exist and are tested; nothing in
  production calls them.
- `ProfileCache` with SQLite write-through exists (`src/web/state.rs:20-72`);
  `ProfileCache::insert_live` and `AthletesDb::touch` have no production
  caller.
- `WBalAccumulator::configure_from_profile(ftp, cp, w_prime)` already
  implements sauce's fallback rules (`crates/zwift-stats/src/wbal.rs:38-47`);
  an unconfigured accumulator returns `None` (`wbal.rs:56-57`).
  `ZonesAccumulator::configure(ftp, zones)` exists
  (`crates/zwift-stats/src/zones.rs:139`); zones come from
  `coggan_zones(ftp)`.
- The relay owns the only authenticated `ZwiftAuth` (held as
  `shutdown_auth`). The profile driver needs that handle; expose an accessor
  on `RelayRuntime` (preferred — avoids a second login) rather than
  constructing a new auth in `run_daemon`.

## Tests first

- [ ] **22.1-T** `crates/zwift-api` (or `tests/`) — a fetch-coordinator unit:
      given a set of athlete ids, it issues at most one batched `get_profiles`
      call per flush window and de-duplicates ids already in flight or
      recently fetched. Assert one batch request for N ids, and that a second
      request for an already-fresh id makes no call.
- [ ] **22.1-I** Implement the coordinator (a small struct holding
      seen/in-flight/last-fetched maps and a staleness TTL). Pure logic, no
      network — inject the fetch as a closure/trait so it is testable.
- [ ] **22.2-T** `RelayRuntime` exposes an auth/profile-fetch accessor;
      assert it returns a usable handle after `start_with_*`.
- [ ] **22.2-I** Add the accessor (return the existing `Arc<ZwiftAuth>` or a
      thin profile-fetch wrapper over it). No second login.
- [ ] **22.3-T** Integration: drive athlete ids through the production path
      (the bridge already sees every `PlayerState`); assert the coordinator
      is invoked and `ProfileCache::insert_live` is called with the fetched
      record, and that `AthletesDb` then returns it. Use a stub auth that
      returns canned profiles.
- [ ] **22.3-I** Add a profile-driver task in `run_daemon` (sibling to the
      bridge): observe athlete ids from the proto stream, feed the
      coordinator, write results through `ProfileCache::insert_live`, and call
      `AthletesDb::touch` for last-seen. Keep SQLite work off the async
      runtime (see K2 — use `spawn_blocking` or a dedicated writer).
- [ ] **22.4-T** Formatter reads identity from the cache: with a profile
      cached, `format_athlete_data_v1` emits a non-null `athlete` block with
      name/weight/FTP. (`src/web/format.rs`.)
- [ ] **22.4-I** Point the formatter's `athlete`/FTP lookup at the
      `ProfileCache` instead of the registry-only fallback
      (`src/web/rpc.rs:291-307` shows the current fallback shape).
- [ ] **22.5-T** W′ and zones configured from FTP: feed a profile with a
      known FTP, then power samples; assert `wBal` is non-null and
      `timeInPowerZones` is populated, and that `tss` is computed.
- [ ] **22.5-I** When a profile arrives in the cache, call
      `WBalAccumulator::configure_from_profile` and
      `ZonesAccumulator::configure(ftp, coggan_zones(ftp))` on that athlete's
      record; reconfigure on FTP change.

## Acceptance criteria

- On a busy course, profiles are fetched in batches, not one-per-rider-per-
  tick; de-duplication holds.
- `athlete`, `tss`, `wBal`, `timeInPowerZones` carry real values in v1/v2
  payloads.
- `athletes.sqlite` is populated and survives a restart (cache read-through
  on next boot).
- Fast suite green; daemon-spawning tests `#[ignore]` with `slow:` reasons.

## Dependencies

- Step 21 not strictly required, but the athlete-id observation rides on the
  same proto stream the bridge consumes.
- Reads K2 (blocking SQLite) — apply the off-runtime write pattern here.

## Deferred

- A request budget / rate-limit guard against Zwift can be tuned later; the
  caution about bulk-fetch tolerance is noted in Q6. Implement batching now;
  revisit throttling if real-server testing shows pushback.
