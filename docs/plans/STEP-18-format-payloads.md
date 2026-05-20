# Step 18 — v1/v2 payload formatters (stub)

## Goal

Port `_formatAthleteData` and `_formatAthleteDataV2` verbatim so
unmodified sauce4zwift browser widgets continue to work (spec §7.9, §7.12
`keepCase` hazard).

- Keep field names byte-identical to the JS formatters (camelCase /
  underscored where JS uses them).
- Implement `ADV2QueryReductionEmitter` — each subscription carries a
  query; memoize formatted payloads so N subscribers with identical
  queries cost one serialization.

## Tests-first outline

- Compare Rust-formatted JSON bytes against JS-formatted JSON bytes for
  a captured trace — zero-diff.
- Query reduction: two subscribers with identical queries cause one
  serialization call (use a counting in-memory implementation).

To be fully elaborated when work on this step begins.

## Deferred from STEP 17 (formatter-dependent routes and filtering)

STEP 17 built the registry, the subscription engine, and the v1/v2 athlete
endpoints, but left three things to this step because they cannot be done
without the byte-parity formatters above. They are recorded here so they are
not lost; pull each into this step's elaboration when work begins.

- **Deep resource-filter parity for v2 endpoints.** STEP 17's v2 filter is a
  whitelist of top-level keys only. Sauce4zwift's query supports nested paths
  such as `resource=lap.distance`. The nested filter belongs here, alongside
  the `_formatAthleteDataV2` port and `ADV2QueryReductionEmitter`, because the
  field shape it filters is defined by the formatter. (STEP-17 "Out of scope",
  item: "Resource-filter parity for every v2 endpoint field".)
- **`GET /api/athlete/streams/v1/:id`.** The stream data already lives on
  `AthleteData::streams` (STEP 14), but the wire format is produced by the
  STEP 18 stream formatter. Add the route here once that formatter exists.
  (STEP-17 "Out of scope", item: "The `/api/athlete/streams/v1/:id` route".)
- **`GET /api/athlete/laps`, `/api/athlete/segments`, `/api/athlete/events`.**
  Same reason as `/streams`: the data is present (STEP 15), but the JSON shape
  is the formatter's responsibility. Add these routes here. (STEP-17 "Out of
  scope", item: "The `/api/athlete/laps`, `/segments`, and `/events` routes".)
