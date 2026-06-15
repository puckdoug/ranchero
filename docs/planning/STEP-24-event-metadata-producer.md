# Step 24 — Event-metadata producer (G5)

Source: `review.md` finding **G5**. Order-of-work item 4. Completes the QA3
event chain: telemetry carries an event-subgroup id, the daemon fetches the
event, caches the subgroup→event mapping, and the formatters spread the event
fields.

## Goal

When the watched athlete is in an event, the published payload carries
real event context — the sub-group, event leader and sweeper, and the
event `remaining*` fields — instead of leaving them absent. The `getEvent`
RPC returns the fetched event.

## Decision in force

**QA3 (event support is a v1 goal):** the absent-event-fields state is only
acceptable in the brief window before the cache populates, never as a shipped
steady state.

## Background the implementer needs

- `WebState.event_subgroups` is read in the per-tick path
  (`src/web/proto_to_stats.rs:193`) and by `getCachedEvent(s)`
  (`src/web/rpc.rs:331-352`), but nothing writes to it.
- `get_event` (`crates/zwift-api/src/lib.rs:552-572`) exists and is never
  called; the `getEvent` RPC is a `null` stub.
- `apply_event_state` already runs in production but always sees a cache
  miss, so it returns `Idle`.
- The event-subgroup cache is in-memory by decision QD2 (no fourth SQLite
  DB) — verified correct in `review.md` §F; do **not** add persistence.

## Tests first

- [ ] **24.1-T** A fetch-on-miss coordinator: given an unknown
      `event_subgroup_id`, it calls `get_event` once and populates
      `event_subgroups`; a second sighting of the same id makes no further
      call. Use a stub auth returning a canned event.
- [ ] **24.1-I** Implement the coordinator (de-dupe by subgroup id, like the
      Step 22 profile coordinator). Inject the fetch for testability.
- [ ] **24.2-T** With the subgroup cached, `apply_event_state` transitions
      out of `Idle` for a state carrying that subgroup id, and the event
      slice / privacy flags are applied.
- [ ] **24.2-I** Wire the coordinator into the production path: when a
      `PlayerState` carries an unknown subgroup id, enqueue the fetch; on
      arrival, insert into `event_subgroups`.
- [ ] **24.3-T** Formatter spread: with event metadata present,
      `eventLeader` / `eventSweeper` / `remaining*` (event variant) appear in
      the payload. (`src/web/format.rs:418-452`.)
- [ ] **24.3-I** Confirm the formatter reads these from the cached subgroup;
      fill any gap so the fields populate.
- [ ] **24.4-T** `getEvent` RPC returns the fetched event for a known id,
      `null` for an unknown one.
- [ ] **24.4-I** Back the `getEvent` RPC with the same fetch-and-cache path.

## Acceptance criteria

- An athlete in an event produces non-absent event fields within a tick or
  two of the subgroup being seen.
- `getEvent` returns real data; `getCachedEvent(s)` reflect the populated
  cache.
- No new SQLite DB; the cache stays in-memory (QD2).
- Fast suite green.

## Dependencies

- Step 22's fetch-coordinator pattern is a useful template; the auth accessor
  added there is reused here.

## Deferred

- Persisting the subgroup cache across restarts is explicitly out of scope
  (QD2). The route half of `_getEventOrRouteInfo` is **Step 27**.
