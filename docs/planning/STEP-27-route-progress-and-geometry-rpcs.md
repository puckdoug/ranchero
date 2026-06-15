# Step 27 — Route progress + geometry RPC getters (G7 + D4)

Source: `review.md` findings **G7** and **D4**. Order-of-work item 7. The
`zwift-routes` crate is built and tested but is an orphan — nothing depends
on it. The geometry RPC getters are registered stubs.

## Goal

Route distance, route percentage, and the route `remaining*` fields are
computed and published. The geometry RPCs (`getWorldMetas`, `getCourseId`,
`getRoad`, `getRoute`, `getSegment`) return real data, and `getWebServerURL`
returns the bound address.

## Decision in force

**QA4 (route progress is a v1 goal):** schedule the `zwift-routes` wiring as
deliberate work.

## Background the implementer needs

- `crates/zwift-routes` has route lookup, lead-in distance, and remaining-info
  computations, each tested — but it is **not** a dependency of the root
  crate (`Cargo.toml` lists no `zwift-routes`) and nothing references it.
- `zwift-worlds` (the sibling crate) **is** wired into the per-tick path
  (`src/web/proto_to_stats.rs:67-79`) — follow that integration as the model.
- The route `remaining*` formatter fields are hardcoded absent with a
  "requires route/event metadata" comment.
- Geometry RPC stubs: `getWorldMetas` returns `[]`, `getCourseId`/`getRoad`/
  `getRoute`/`getSegment` return `null` (`src/web/rpc.rs:420-426`).
- `getWebServerURL` returns `null` — the bound address lives in the web
  handle but is not threaded into the registry.

## Tests first

- [ ] **27.1-T** Add `zwift-routes` as a root-crate dependency and call a
      route computation from a unit test exercising the per-tick path:
      given a `route_id` and distance-along, `routeDistance` and route
      percentage are computed.
- [ ] **27.1-I** Add the dependency to `Cargo.toml`; call the route
      computations from `route_player_state`, keyed by the rider's
      `route_id`, mirroring sauce's `_computeRouteDistance`.
- [ ] **27.2-T** Route `remaining*` fields populate for a rider on a known
      route (the route half of `_getEventOrRouteInfo`): `remaining`,
      `remainingMetric`, `remainingType` = `"route"`, `remainingEnd`.
- [ ] **27.2-I** Compute and publish the route `remaining*` set in the
      formatter; replace the hardcoded-absent values.
- [ ] **27.3-T** `getWorldMetas` returns the world-meta table;
      `getCourseId` / `getRoad` / `getRoute` / `getSegment` return real
      records for known ids and `null` for unknown.
- [ ] **27.3-I** Back the geometry getters from `zwift-worlds` /
      `zwift-routes` (`src/web/rpc.rs:420-426`).
- [ ] **27.4-T** `getWebServerURL` returns the bound `http(s)://host:port`.
- [ ] **27.4-I** Thread the bound address from the web handle into
      `WebState` and back the RPC.

## Acceptance criteria

- `routeDistance`, route percentage, and route `remaining*` carry real values
  for riders on a known route.
- The geometry RPCs and `getWebServerURL` return real data.
- Fast suite green.

## Dependencies

- `zwift-worlds` integration (Step 18 work) is the reference pattern and is
  already in place.

## Deferred

- `getPowerProfile`, `getEventSubgroupEntrants`, `getEventSubgroupResults`
  stubs depend on other data (the latter two on Step 24); leave them stubbed
  but annotate each with the finding it waits on, per D4's recommendation.
