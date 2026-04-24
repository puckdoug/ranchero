# Step 19 — Compatibility test battery (stub)

## Goal

Pull the whole pipeline together and verify spec §7.11:

1. **AES-GCM interop.** Fixed `(key, iv, aad, plaintext)` → byte-identical
   ciphertext+tag between JS and Rust (already exercised in STEP 08; here
   we pin it in a reproducible top-level test).
2. **Header codec round-trip.** Fuzz all 8 flag combinations (same note).
3. **Login.** Against a live-ish environment (or the captured replay),
   the Rust monitor must produce a `ServerToClient` on TCP and receive
   one UDP packet within 5 s of `establish()`.
4. **Metric parity.** Feed a recorded `ServerToClient` trace through
   both engines; compare published metrics per tick. ≤ 1e-6 drift on
   sums, exact match on counts/zones.
5. **WebSocket parity.** Point an unmodified sauce4zwift widget page at
   the Rust daemon; widgets render correctly (smoke + golden-snapshot).

## Tests-first outline

- Add a `compat/` fixtures tree:
  `compat/fixtures/server_to_client/{name}.bin` captured streams.
- `compat/expected/{name}.metrics.json` with the JS reference outputs.
- `tests/compat_metric_parity.rs` iterates every fixture.

To be fully elaborated when we start work on this step.
