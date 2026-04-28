# Step 19 — Compatibility test battery (stub)

## Goal

Pull the whole pipeline together and verify spec §7.11:

1. **AES-GCM interop.** Fixed `(key, iv, aad, plaintext)` → byte-identical
   ciphertext+tag between JS and Rust (already exercised in STEP 08; here
   it is pinned in a reproducible top-level test).
2. **Header codec round-trip.** Fuzz all 8 flag combinations (same note).
3. **Login.** Against a near-live environment (or the captured replay),
   the Rust monitor must produce a `ServerToClient` on TCP and receive
   one UDP packet within 5 s of `establish()`.
4. **Metric parity.** Feed a recorded `ServerToClient` trace through
   both engines; compare published metrics per tick. ≤ 1e-6 drift on
   sums, exact match on counts and zones.
5. **WebSocket parity.** Point ranchero's vendored widget pages (the
   `pages/` tree copied in at the time of the port) at the Rust
   daemon; widgets render correctly (manual verification plus
   golden-snapshot). The golden snapshots are captured once against
   the original JS server during the port and then frozen in
   ranchero's repository; the test must not require a live
   sauce4zwift checkout.

## Tests-first outline

- Add a `compat/` fixtures tree:
  `compat/fixtures/server_to_client/{name}.bin` captured streams.
- `compat/expected/{name}.metrics.json` with the JS reference outputs.
- `tests/compat_metric_parity.rs` iterates every fixture.

To be fully elaborated when work on this step begins.
