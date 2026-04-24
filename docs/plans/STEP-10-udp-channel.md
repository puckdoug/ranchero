# Step 10 — UDP channel + time sync (stub)

## Goal

Establish the secure UDPv4 channel per spec §4.6:

- Connected UDP socket to the chosen server's `securePort` (3024).
- Up to 25 hello packets with increasing delay (10, 20, 30… ms).
- For each reply, compute latency & offset; on ≥5 samples with
  reasonable stddev, take the median-by-latency and call
  `worldTimer.adjustOffset(-meanOffset)`.
- Emit `"latency"` events for observability.
- Watchdog: every `CHANNEL_TIMEOUT / 2`; reconnect after 30 s silence.

## Tests-first outline

- Fake transport trait so UDP I/O can be replayed deterministically.
- Time-sync math: feed synthetic `(localWorldTime, serverWorldTime,
  arrival)` triples; assert computed offset/latency match hand-computed
  values.
- Outlier rejection: seed 4 tight samples + 1 absurd one; median pick is
  correct.

To be fully elaborated when we start work on this step.
