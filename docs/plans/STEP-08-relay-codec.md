# Step 08 — `zwift-relay` codec (stub)

## Goal

Pure, no-I/O module implementing the on-the-wire codec for the relay
protocol:

- `RelayIv` → 12-byte GCM IV (spec §7.4; zero bytes 0-1 explicitly).
- `Header` with `HeaderFlags` (RELAY_ID/CONN_ID/SEQNO) encode/decode.
- AES-128-GCM with **4-byte** tag (via `AesGcm<Aes128, U4>` type alias).
- TCP frame wrapping: `[BE u16 size][header][ciphertext||tag4]` over a
  `plaintext = [version=2][hello?0:1][proto bytes]`.
- UDP frame wrapping: identical minus the size prefix and the
  version/hello prefix.

## Tests-first outline

- Known-vector test: round-trip encrypt/decrypt of a fixed
  `(key, iv, aad, plaintext)` tuple, matching a reference byte dump
  captured by running the JS client with a known AES key.
- Fuzz `Header` encode/decode for all eight flag combinations —
  `encode(decode(x)) == x`.
- Reject tampered tags.
- Seqno increment semantics: sender increments post-encrypt, receiver
  updates its counter from header fields only when present.

This is exactly spec §7.11 compatibility tests 1 and 2.

To be fully elaborated when we start work on this step.
