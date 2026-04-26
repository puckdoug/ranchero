# Step 11.5 — Wire capture & replay (stub)

## Goal

Make ranchero self-sufficient for producing the captured `ServerToClient`
/ `ClientToServer` fixtures that STEPS 08, 18, and 19 depend on, instead
of inheriting them from a JS reference implementation. Also useful for
debugging and for exploring future protocol changes.

- `ranchero start --capture <path>` opens a writer; capture is off when
  the flag is absent (zero overhead in that path).
- Tap sits on each channel's receive path *after* AES-GCM tag verification
  and *before* prost decode, plus on each send path for `ClientToServer`.
  A single tap covers both UDP (STEP 10) and TCP (STEP 11) channels —
  this step lands after both exist.
- A companion `ranchero replay <path>` (or `xtask replay`) reads a
  capture and feeds records into the same decode/stats pipeline a live
  channel would. STEPS 18 and 19 consume the replay path; no JS
  dependency.

## Sketch

- **File format (working proposal — settle at elaboration time):**
  framed records, each
  `[u64 ts_ns][u8 direction][u8 transport][u8 flags][u32 len][len bytes plaintext]`,
  preceded by a small file header `[magic "RNCWCAP\0"][u16 version]`.
  Length-prefixed, self-describing, append-only, fast to write, easy to
  iterate. JSONL is rejected — too large and slow for a multi-Hz UDP
  stream.
- **What "plaintext" means:** the bytes that go into `prost::decode`
  (post-AES-GCM, post-2-byte version/hello prefix on TCP). This is the
  most useful level for STEPS 18/19 and avoids leaking the AES session
  key.
- **Optional `--capture-raw` mode (deferred):** store ciphertext + IV +
  AAD + the per-session AES key alongside, so codec-layer tests in
  STEP 08 can replay through the AES path. The key is short-lived but
  authenticates the relay session — gate this mode behind an explicit
  flag and a warning, default off, never the default for `--capture`.
- **Backpressure:** write to a bounded `tokio::sync::mpsc` from the
  channel hot path; a dedicated writer task does file I/O. If the
  channel fills (slow disk), drop records and increment a counter
  rather than backpressuring the network read — capture must never
  affect live behavior.
- **Rotation:** out of scope for v1. A single file per `start`
  invocation is enough for the test-fixture use case.

## Tests-first outline

- Round-trip: write a synthetic stream of `ServerToClient` and
  `ClientToServer` records, read it back, byte-equal each plaintext.
- Drop-on-saturation: feed records faster than the writer drains;
  assert dropped-count metric advances and no panic / no
  network-thread stall.
- Format stability: the `RNCWCAP` magic + version must round-trip; an
  unknown version on read is a clean error, not a panic.
- Capture-off zero-overhead: when `--capture` is absent, no tap code
  paths execute (verify by feature flag / branch absence in a unit
  test, or by a microbench that asserts equivalent throughput).

## Open questions for elaboration

- Exact format of the per-record header — should `flags` carry the
  channel id (UDP server pool index) so multi-UDP captures are
  unambiguous?
- Single file or per-channel? Single is simpler; per-channel makes
  filtering trivial.
- Should `ranchero replay` be its own subcommand (visible to users)
  or an `xtask` (developer-only)? Probably the former since the user
  asked about exploring future features.

## Plans this unblocks / updates

- STEP 08: replace "captured by running the JS client with a known
  AES key" with "captured via `ranchero start --capture-raw`" once
  this step lands (only step that needs the raw form).
- STEP 18: parity-test traces come from `--capture` files.
- STEP 19: `compat/fixtures/server_to_client/{name}.bin` are
  `--capture` files; the WebSocket-parity smoke test feeds the daemon
  via `ranchero replay` instead of requiring a live Zwift session.

To be fully elaborated when we start work on this step.
