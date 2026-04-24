# Step 18 — v1/v2 payload formatters (stub)

## Goal

Port `_formatAthleteData` and `_formatAthleteDataV2` verbatim so
unmodified sauce4zwift browser widgets continue to work (spec §7.9, §7.12
`keepCase` footgun).

- Keep field names byte-identical to the JS formatters (camelCase /
  underscored where JS uses them).
- Implement `ADV2QueryReductionEmitter` — each subscription carries a
  query; memoize formatted payloads so N subscribers with identical
  queries cost one serialization.

## Tests-first outline

- Compare Rust-formatted JSON bytes against JS-formatted JSON bytes for
  a captured trace — zero-diff.
- Query reduction: two subscribers with identical queries cause one
  serialization call (use a counting fake).

To be fully elaborated when we start work on this step.
