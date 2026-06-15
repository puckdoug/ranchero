# Step 23 — 1 Hz nearby/groups processor (G2 + D2)

Source: `review.md` findings **G2** (with **D2** folded in). Order-of-work
item 3. This is STEP-20 plan Step 12, which was ticked complete but never
implemented — the library functions exist with no production caller.

## Goal

A periodic task computes each athlete's gap and group membership and emits
`nearby` and `groups` events (v1 and v2). The `/nearby/*` and `/groups/*`
HTTP routes and the `getNearbyData` / `getGroupsData` RPCs return real,
sorted data. The most-used overlay widgets work.

## Background the implementer needs

- `compute_groups` (`crates/zwift-stats/src/groups.rs:59`), `apply_gap`,
  `apply_gaps_for_registry`, and `apply_gap_chained`
  (`crates/zwift-stats/src/gap.rs`) exist and are tested; none is called
  outside tests.
- The only periodic web task is `gc_tick_loop` (`src/web/server.rs:209`) —
  the new task is a sibling of it.
- `gap` / `gap_distance` / `is_gap_est` / `group_id` are always `None` today,
  so `/nearby/v1` sorts on nothing and `/groups/*` returns empty.
- The `getNearbyData` / `getGroupsData` RPCs are explicit `[]` stubs
  (`src/web/rpc.rs:312-323`).
- `event_matches_athlete` already correctly rejects `nearby`/`groups` as
  non-athlete event names (`src/web/subs/mod.rs:481` test) — so the producer
  must emit array payloads, not per-athlete ones.
- Per-tick recording (`most_recent_state`, road history) is in place, so the
  inputs the gap/group math needs already exist.

## Tests first

- [ ] **23.1-T** A processor pass over a registry of several athletes sets
      `gap` / `gap_distance` / `is_gap_est` on each (direct road comparison
      where possible, chained estimate otherwise) and assigns `group_id`.
      Assert against a hand-built registry with known positions.
- [ ] **23.1-I** Add a processor function that calls
      `apply_gaps_for_registry` then `compute_groups` + `assign_group_ids`,
      writing results back onto the athlete records. Port sauce's
      `_computeNearby` chained estimation — `apply_gap_chained` exists but the
      walk across adjacent riders (ahead/behind split, sort by gap distance,
      infer each missing gap) is what makes estimates usable.
- [ ] **23.2-T** `nearby` v1 array is sorted by gap ascending and contains
      formatted athletes; `nearby` v2 is **also** sorted (this is D2).
- [ ] **23.2-I** Build the `nearby` array from the processed registry, sorted
      by gap, in both v1 and v2 shapes. Fix `nearby_v2_handler`
      (`src/web/http/mod.rs:266-278`) to apply the same sort as
      `nearby_v1_handler`.
- [ ] **23.3-T** `groups` array groups athletes by `group_id`, each group
      carrying its aggregate (min gap, composition); clustering threshold is
      2 s (0.8 s without draft), with Jaccard identity preservation across
      frames.
- [ ] **23.3-I** Build the `groups` payload from `compute_groups`; preserve
      group identity across ticks per sauce's `_computeGroups`.
- [ ] **23.4-T** The 1 Hz task emits `nearby` and `groups` (v1 and v2) to
      subscribers; a subscribed socket receives the arrays once per tick.
- [ ] **23.4-I** Spawn the task (sibling to `gc_tick_loop`) at 1 Hz; emit the
      four event payloads through the web event path.
- [ ] **23.5-T** `getNearbyData` / `getGroupsData` RPCs return the same
      arrays the routes serve.
- [ ] **23.5-I** Replace the `[]` stubs (`src/web/rpc.rs:312-323`) with reads
      of the processed registry.

## Acceptance criteria

- `gap` / `group_id` populated for athletes within range; `nearby` sorted in
  v1 and v2; `groups` non-empty when riders cluster.
- WebSocket `nearby` / `groups` subscriptions deliver sorted arrays, not
  per-athlete payloads.
- Fast suite green; the 1 Hz task does not regress `gc_tick_loop` cadence.

## Dependencies

- Step 21 (event forwarding) so the emitted `nearby`/`groups` events reach
  subscribers in production.
- Step 22 helps (names in the nearby list) but is not strictly required for
  the gap/group math.

## Deferred

- `eventPosition` / `eventParticipants` from `EventSubgroupPlacements`
  (proto `ev_subgroup_ps = 23`) ride along with the event chain — see
  Step 24 / STEP-20 item 20.22.
