# `tests/fixtures/` — reference vectors for `zwift-relay`

These vectors are the oracle for spec §7.11 compatibility test #1
("AES-GCM byte-identical interop between JS and Rust").

## `gen_vectors.mjs`

Run from this directory:

```
node gen_vectors.mjs
```

Outputs Rust `const` declarations to stdout. The script is
deterministic: the inputs are hard-coded so re-running produces
byte-identical output as long as the underlying Node `crypto` module
behaves the same. The output is checked into
`tests/crypto.rs` as `KEY` / `IV` / `AAD` / `PLAINTEXT` /
`CIPHER_TAG` constants.

The script is **not** invoked from CI. It exists so that:

- A reader can verify the vectors are reproducible from a
  documented input set, not invented bytes.
- Regenerating the vectors after a Node release upgrade is one
  command and a copy-paste, not an archaeology dig.

If `gen_vectors.mjs` ever produces different output on a host where
the Rust test passes, that's a discovery worth understanding before
shipping — Node and `RustCrypto::aes-gcm` are the two ends of the
"byte-identical between JS and Rust" claim the spec rests on.

## Why Node, not a NIST CAVP vector?

NIST CAVP publishes AES-GCM test vectors and would be a more
authoritative oracle in general. The reason ranchero uses
Node-derived vectors specifically is that the live sauce4zwift
client uses Node's `crypto.createCipheriv` directly
(`zwift.mjs:1092-1106`). Matching what *that* client emits — bit for
bit — is the property the spec actually requires of a port. A NIST
match would be necessary but not sufficient.
