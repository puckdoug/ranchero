# Step 26 — Segment leaderboards: fetch, store, evict, serve (G6)

Source: `review.md` finding **G6**. Order-of-work item 6. The fetchers, the
store, and the evictor all exist and are tested in isolation; none runs in
production.

## Goal

When the watched athlete approaches or completes a segment, its leaderboard
is fetched, cached in `segments.sqlite` with a TTL, served by the
`getSegmentResults` RPC, and the stale rows are evicted on a schedule.

## Decision in force

**QA2 (segment leaderboards stay in scope):** so the fetchers, the evictor,
and `segments.sqlite` are all retained and wired — not removed.

## Background the implementer needs

- `get_segment_results`, `get_live_segment_leaders`,
  `get_live_segment_leaderboard` (`crates/zwift-api/src/lib.rs:579-658`) have
  no production caller.
- `SegmentsDb::put`/`get` are tested but never called outside tests; the
  `getSegmentResults` RPC returns `[]`.
- `segments_evict_tick_loop` is fully implemented and exported
  (`src/daemon/stores.rs:23-42`) but **never spawned** — `run_daemon` spawns
  only the web server and `gc_tick_loop` (`src/web/server.rs:209`).
- `segments.sqlite` is a ranchero-original design (sauce uses a 2-second
  in-memory cache); keep the TTL store but note this is not a parity
  requirement.

## Tests first

Do the evictor spawn **first** — it is one line and prevents unbounded growth
once a writer exists.

- [ ] **26.1-T** `segments_evict_tick_loop` is spawned by the daemon: a
      harness over `run_daemon`'s wiring shows the evict loop running on its
      interval (`SEGMENTS_EVICT_TICK_INTERVAL_SECS`).
- [ ] **26.1-I** Spawn `segments_evict_tick_loop` in `run_daemon` beside
      `gc_tick_loop`.
- [ ] **26.2-T** A fetch-on-active-segment path: when the active-segment
      check fires for a segment id, the leaderboard fetcher is invoked once
      and the result is written through `SegmentsDb::put`. Stub the auth.
- [ ] **26.2-I** Wire `get_segment_results` (and the live-leaderboard
      fetchers as needed) behind the active-segment detection in the per-tick
      path; write results through `SegmentsDb`, off the async runtime (K2).
- [ ] **26.3-T** `getSegmentResults` RPC returns cached leaderboard rows for
      a known segment id, honouring TTL (an expired row is not returned).
- [ ] **26.3-I** Back the RPC with `SegmentsDb::get`.
- [ ] **26.4-T** Eviction removes expired rows: after the TTL passes, an
      evict pass drops the row and the RPC returns `[]`.
- [ ] **26.4-I** Covered by 26.1-I + the store's existing `evict_expired`;
      lock with the test.

## Acceptance criteria

- Leaderboards populate `segments.sqlite` on active-segment events and are
  served by the RPC.
- The evictor runs; the table does not grow without bound.
- Fast suite green; daemon-spawning tests `#[ignore]` with `slow:` reasons.

## Dependencies

- Active-segment detection (`active_segment_check`) is already wired in the
  per-tick path when a segment environment is attached — confirm that
  environment is present in production or attach it here.
- Step 22's auth accessor is reused for the fetch.

## Deferred

- Matching sauce's exact in-memory 2 s cache shape is not required; the TTL
  store is the chosen design.
