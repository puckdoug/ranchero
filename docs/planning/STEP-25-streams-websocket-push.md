# Step 25 — `streams/*` WebSocket push (D3)

Source: `review.md` finding **D3**. Order-of-work item 5.

## Goal

Chart widgets that subscribe to `streams/watching`, `streams/self`, and
`streams/{id}` receive live stream slices over WebSocket, matching sauce.
Today only the one-shot HTTP route exists, so a subscribing chart widget
stays empty.

## Decision in force

**Q8 (2026-06-12): WebSocket push is critical for v1.** Fetch-only is not
acceptable.

## Background the implementer needs

- The HTTP route `/athlete/streams/v1/...` serves stream arrays today.
- The subscription layer has **no** producer for `streams/*` — there is no
  `"streams/"` match in `src/web/subs/mod.rs` (compare the existing
  `athlete/*`, `chat`, `rideon` matches at `subs/mod.rs:405-438`).
- sauce emits stream *slices* on a short interval, not one push per inbound
  `PlayerState` — the push carries the new samples since the last emit, not
  the whole accumulated array each time.

## Tests first

- [ ] **25.1-T** A subscriber on `stats` / `streams/watching` receives a
      slice payload after new samples are recorded for the watched athlete;
      the slice contains the expected stream keys
      (power/speed/hr/altitude/latlng/wbal/distance) and only the new
      samples, not the full history.
- [ ] **25.1-I** Add a `streams/*` producer to the subscription layer:
      recognise the `streams/watching|self|{id}` event names, and emit a
      slice of newly-appended stream samples on a short interval (match
      sauce's cadence; do not emit per state).
- [ ] **25.2-T** `streams/self` and `streams/{id}` resolve to the right
      athlete (self = watched athlete; `{id}` = that id); a subscriber gets
      slices for the correct rider.
- [ ] **25.2-I** Reuse the athlete-resolution the `athlete/*` producers use
      for `watching` / `self` / `{id}`.
- [ ] **25.3-T** No-new-samples case: when nothing new has been recorded, the
      interval tick emits nothing (no empty-slice spam).
- [ ] **25.3-I** Track a per-subscription high-water mark so each emit only
      sends samples past it.

## Acceptance criteria

- A subscribing chart widget renders a live-updating trace.
- Slices are incremental (new samples only), emitted on an interval, not per
  inbound state.
- The HTTP one-shot route still works unchanged.
- Fast suite green.

## Dependencies

- Step 21 (event delivery path) so the produced slices reach subscribers.
- Per-tick stream recording is already in place (Step 20 work).

## Deferred

- None specific. v2 query reduction for streams, if sauce applies it, can be
  matched here or noted as a follow-up.
