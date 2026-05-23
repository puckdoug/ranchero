# Step 08 — `zwift-relay` codec

**Status:** planned (2026-04-27).

## Goal

A pure, no-I/O Rust crate (`crates/zwift-relay`) that implements the
on-the-wire codec for the Zwift relay protocol: AES-128-GCM with a
**4-byte** auth tag, the `RelayIv` derivation, the variable-length
`Header`, and the TCP/UDP frame-wrapping rules. Bytes in, bytes out;
no sockets, no `tokio`, no `reqwest`.

This is the foundation under STEP 09 (`RelaySession`), STEP 10
(`UdpChannel`), and STEP 11 (`TcpChannel`). It also satisfies spec §7.11
compatibility tests #1 (AES-GCM byte-identical interop) and #2
(header round-trip across all 8 flag combinations).

## Scope

**In scope** (everything is a stateless function or a small POD type
on `Vec<u8>` / `&[u8]` / fixed-size arrays):

- `RelayIv` → 12-byte AES-GCM IV, with the leading two zero bytes
  written explicitly (spec §7.12 hazard).
- `HeaderFlags` bitmap + `Header` encode / decode (variable length,
  1-11 bytes).
- AES-128-GCM with `U4` tag size (`type Aes128Gcm4 = AesGcm<Aes128, U4>`,
  spec §7.3 note).
- Plaintext envelope construction:
  - TCP: `[u8 version=2][u8 hello?0:1][proto_bytes]`
  - UDP: `[u8 version=1][proto_bytes]` (verify — see "Open
    verification points" below)
- TCP frame wrapping: prepend `BE u16 frame_size` to
  `[header][ciphertext||tag4]`.
- TCP stream framing helper: split a byte stream into individual
  frames (the `_onTCPData` accumulator from `zwift.mjs:1259-1289`),
  returning `Ok(None)` when more bytes are needed.
- UDP frame wrapping: concatenate `[header][ciphertext||tag4]`
  (datagrams are self-delimited).
- Constants: `WORLD_TIME_EPOCH_MS`, port numbers, `TAG_LEN = 4`,
  `IV_LEN = 12`, version bytes (spec §7.4).

**Out of scope** (deferred to later steps):

| Concern | Where it is implemented |
|---|---|
| Owning `send_iv` / `recv_iv` mutable state | STEP 10 / 11 channels |
| TCP / UDP socket lifetimes, watchdog, reconnect | STEP 10 / 11 |
| Hello-packet handshake / SNTP-style time sync | STEP 10 (UDP) |
| Server pool selection | STEP 09 (`RelaySession`) |
| `LoginRequest` POST + `LoginResponse` parse | STEP 09 |
| World-time clock, athlete state, metrics | STEP 12+ |

## Crate layout

```
crates/zwift-relay/
├── Cargo.toml          — workspace member, AGPL-3.0-only
├── src/
│   ├── lib.rs          — re-exports + module-level docs
│   ├── consts.rs       — DeviceType, ChannelType, version bytes, ports, and so on
│   ├── iv.rs           — RelayIv + to_bytes()
│   ├── header.rs       — HeaderFlags, Header, encode/decode
│   ├── crypto.rs       — Aes128Gcm4 alias + encrypt/decrypt wrappers
│   └── frame.rs        — plaintext envelopes + TCP/UDP framing
└── tests/
    ├── iv.rs           — IV layout vectors (deterministic byte arrays)
    ├── header.rs       — round-trip + all-8-flag-combinations property test
    ├── crypto.rs       — AES-GCM-4 known-answer vectors
    ├── frame.rs        — TCP/UDP wire-format end-to-end vectors
    └── fixtures/
        ├── README.md   — how vectors were generated
        └── *.json      — vector inputs/outputs (key/iv/aad/plain/cipher)
```

The crate is split across modules for readability, but every public
item is re-exported from `lib.rs` so callers `use zwift_relay::{…}`
without navigating internal module paths (same convention as
`zwift-api`).

## Dependencies

```toml
[dependencies]
aes-gcm = { version = "0.10", default-features = false, features = ["aes", "alloc"] }
bitflags = "2"
thiserror = "1"

[dev-dependencies]
hex = "0.4"           # readable test vector literals
serde = { version = "1", features = ["derive"] }
serde_json = "1"
zwift-proto = { path = "../zwift-proto" }
```

Notes:

- **No `prost` dep on the main crate.** The codec works on `&[u8]`;
  it never decodes a proto. `zwift-proto` is `dev-dependencies` only,
  so end-to-end tests can wrap a real `ClientToServer::default()` to
  prove the envelope shape lines up.
- **No `tokio` dep, anywhere.** The codec is synchronous and stateless. The
  channel layer (STEP 10/11) introduces tokio.
- **`aes-gcm` with `default-features = false`.** Pulls in the
  `aes` and `alloc` features only; this keeps the crate `no_std`-ready
  even though it is not required at this step.

## Public API surface (proposed)

### Constants (`consts`)

```rust
pub const IV_LEN: usize  = 12;
pub const TAG_LEN: usize = 4;
pub const KEY_LEN: usize = 16;
pub const TCP_VERSION: u8 = 2;
pub const UDP_VERSION: u8 = 1;
```

Plus the spec §7.4 protocol constants (`WORLD_TIME_EPOCH_MS`, port
numbers, and similar): these duplicate spec text but live here so callers
do not have to pull a different crate to obtain a port number.

### Enums (`consts`)

```rust
#[repr(u16)]
pub enum DeviceType  { Relay = 1, Companion = 2 }

#[repr(u16)]
pub enum ChannelType { UdpClient = 1, UdpServer = 2, TcpClient = 3, TcpServer = 4 }
```

### `RelayIv` (`iv`)

```rust
pub struct RelayIv {
    pub device:  DeviceType,
    pub channel: ChannelType,
    pub conn_id: u16,
    pub seqno:   u32,
}

impl RelayIv {
    /// 12-byte GCM IV. **Bytes 0..2 are explicitly zeroed**; see
    /// spec §7.12 regarding the `Buffer.allocUnsafe` hazard in the JS
    /// reference.
    pub fn to_bytes(&self) -> [u8; IV_LEN];
}
```

### Header (`header`)

```rust
bitflags::bitflags! {
    pub struct HeaderFlags: u8 {
        const RELAY_ID = 0x4;
        const CONN_ID  = 0x2;
        const SEQNO    = 0x1;
    }
}

pub struct Header {
    pub flags:    HeaderFlags,
    pub relay_id: Option<u32>,
    pub conn_id:  Option<u16>,
    pub seqno:    Option<u32>,
}

pub struct ParsedHeader {
    pub header: Header,
    /// Number of bytes consumed by the header (== length of the
    /// AAD slice the caller will pass to `decrypt`).
    pub consumed: usize,
}

impl Header {
    pub fn encode(&self) -> Vec<u8>;
}

pub fn decode_header(bytes: &[u8]) -> Result<ParsedHeader, CodecError>;
```

Encode rule (mirrors `zwift.mjs:1112-1135`): write `flags` byte,
then the present fields in order `relay_id` (BE u32) → `conn_id`
(BE u16) → `seqno` (BE u32). Decode is the inverse, returning
`Header` plus `consumed` so the caller can slice the AAD off the
front of the packet.

### Crypto (`crypto`)

```rust
pub type Aes128Gcm4 = aes_gcm::AesGcm<aes_gcm::aes::Aes128, typenum::U4>;

pub fn encrypt(key: &[u8; KEY_LEN], iv: &[u8; IV_LEN], aad: &[u8], plaintext: &[u8])
    -> Vec<u8>;

pub fn decrypt(key: &[u8; KEY_LEN], iv: &[u8; IV_LEN], aad: &[u8], ciphertext_with_tag: &[u8])
    -> Result<Vec<u8>, CodecError>;
```

`encrypt` returns `ciphertext || tag4`. `decrypt` accepts the same
shape and validates the tag; tag-mismatch surfaces as
`CodecError::AuthTagMismatch` (the implementation must reject tampered packets;
this is covered by an explicit test).

### Plaintext envelopes + framing (`frame`)

```rust
pub fn tcp_plaintext(proto_bytes: &[u8], hello: bool) -> Vec<u8>;
pub fn udp_plaintext(proto_bytes: &[u8])              -> Vec<u8>;

pub fn parse_tcp_plaintext(buf: &[u8]) -> Result<TcpPlain<'_>, CodecError>;
pub fn parse_udp_plaintext(buf: &[u8]) -> Result<UdpPlain<'_>, CodecError>;

pub fn frame_tcp(header_bytes: &[u8], ciphertext_with_tag: &[u8]) -> Vec<u8>;

/// Stream demuxer for TCP. Returns `Ok(Some((payload, consumed)))` for
/// the next complete frame, `Ok(None)` if more bytes are needed.
/// Mirrors `_onTCPData`'s offset-walking loop at
/// `zwift.mjs:1259-1289`.
pub fn next_tcp_frame(buf: &[u8]) -> Result<Option<(&[u8], usize)>, CodecError>;
```

### Errors (`lib.rs` re-export)

```rust
#[derive(thiserror::Error, Debug)]
pub enum CodecError {
    #[error("input too short: need {needed} bytes, got {got}")]   TooShort { needed: usize, got: usize },
    #[error("unrecognized header flag bits: 0x{0:02x}")]          UnknownFlagBits(u8),
    #[error("AES-GCM auth tag mismatch (decrypt rejected)")]      AuthTagMismatch,
    #[error("frame size {0} exceeds buffer length")]              FrameSizeExceedsBuffer(u16),
    #[error("plaintext envelope: bad version byte {got}")]        BadVersion { got: u8 },
}
```

`relay_id` validation is *not* in the codec; see "Open verification
points" §3.

## Tests-first plan

Every test resides at `crates/zwift-relay/tests/*.rs` (following the project's
pattern of integration tests, not unit tests in `src/`). TDD order:
write each test, observe it fail, then implement until it passes.

### `iv.rs` — RelayIV vectors

| Test | Asserts |
|---|---|
| `iv_layout_zero_bytes_at_offsets_0_and_1` | `RelayIv { device: Relay, channel: UdpClient, conn_id: 0, seqno: 0 }.to_bytes()[0..2] == [0, 0]`. Catches the spec §7.12 hazard directly. |
| `iv_layout_known_vector` | A manually constructed `RelayIv` matches a precomputed 12-byte array, byte-for-byte. |
| `iv_byte_order_is_big_endian` | `device=2, channel=1, conn_id=0xABCD, seqno=0x12345678` → `[0,0, 0,2, 0,1, 0xAB,0xCD, 0x12,0x34,0x56,0x78]`. |

### `header.rs` — header codec

| Test | Asserts |
|---|---|
| `header_round_trip_all_flag_combinations` | For each of the 8 `HeaderFlags` combinations, build a `Header` with `Some(_)` for present fields, `encode()`, `decode_header()`, assert equality. **Spec §7.11 compat test #2.** |
| `header_steady_state_is_one_byte` | `Header { flags: empty, all None }` encodes to `[0x00]`, `consumed == 1`. |
| `header_field_order_relay_id_conn_id_seqno` | A header with all three flags encodes to `[0x07, relay_id_be4, conn_id_be2, seqno_be4]` with `consumed == 11`. |
| `header_decode_short_input_errors` | Truncated header bytes surface `CodecError::TooShort`. |
| `header_decode_unknown_flag_bits_errors` | A high bit (e.g. `0x08`) surfaces `CodecError::UnknownFlagBits(0x08)`. |

### `crypto.rs` — AES-GCM-4 known-answer vectors

| Test | Asserts |
|---|---|
| `aes_gcm4_encrypt_known_vector` | A canned `(key, iv, aad, plaintext)` produces a specific `ciphertext\|\|tag` byte string. **Spec §7.11 compat test #1.** |
| `aes_gcm4_decrypt_known_vector` | The inverse of the above. |
| `aes_gcm4_round_trip_random` | For 16 random inputs, `decrypt(encrypt(p)) == p`. |
| `aes_gcm4_decrypt_rejects_tampered_tag` | Flip a bit in the tag → `CodecError::AuthTagMismatch`. |
| `aes_gcm4_decrypt_rejects_tampered_aad` | Modify the AAD passed to `decrypt` → `AuthTagMismatch`. |
| `aes_gcm4_decrypt_rejects_tampered_ciphertext` | Flip a bit in the ciphertext body → `AuthTagMismatch`. |

**Reference vector strategy.** The compat-test #1 known answer is
generated once via a small Node script
(`crates/zwift-relay/tests/fixtures/gen_vectors.mjs`) that calls
`crypto.createCipheriv('aes-128-gcm', key, iv, { authTagLength: 4 })`
with hard-coded inputs and prints the outputs as hex. The script is
checked in for reproducibility but is not invoked from CI; the Rust
test reads the resulting byte arrays as constants. Treating the Node
output as the oracle is what the spec requires ("byte-identical
between JS and Rust") and allows the project to regenerate vectors
deterministically without provisioning a Zwift account.

### `frame.rs` — envelope + framing

| Test | Asserts |
|---|---|
| `tcp_plaintext_hello_byte` | `tcp_plaintext(b"abc", true)[0..2] == [2, 0]`; `tcp_plaintext(b"abc", false)[0..2] == [2, 1]`. |
| `udp_plaintext_version_byte` | `udp_plaintext(b"abc")[0] == 1`. (See "Open verification points".) |
| `tcp_frame_size_prefix_is_be_u16` | `frame_tcp(&[0; 5], &[0; 100])` starts with `[0x00, 0x69]` (5 + 100 = 105). |
| `tcp_next_frame_returns_none_on_short_buffer` | Single byte input → `Ok(None)`. |
| `tcp_next_frame_handles_back_to_back_frames` | Concatenated `[size1][p1][size2][p2]` yields two successive `Some` returns with correct `consumed` bumps. |
| `tcp_round_trip_with_real_proto` | Build a `ClientToServer::default()` via `zwift_proto`, encode → wrap → encrypt → frame → unframe → decrypt → unwrap → decode, assert equality. |
| `udp_round_trip_with_real_proto` | Same for UDP with no length prefix and the 1-byte plaintext envelope. |

## Open verification points

These are claims the implementor should confirm against a real
sauce4zwift run (or a captured packet) before declaring this step
complete. None block tests; the codec can be implemented and tested
against either choice. Record any decision in the as-built document.

1. **UDP plaintext shape.** Spec §4.4 / §7.4 says "UDP plaintext is
   the proto bytes only." The actual sauce code at
   `zwift.mjs:1437-1440` prepends `[u8 version=1]`. The plan above
   follows the *code*, because sauce works against real Zwift
   servers. If the `udp_round_trip_with_real_proto` test fails
   against captured wire data, that will indicate the spec is correct and the
   code is the anomaly (or the reverse). Update the spec to match
   whichever takes precedence.

2. **`forceSeq` on UDP sends.** The JS UDP path hard-codes
   `{forceSeq: true, ...options}` (`zwift.mjs:1439`) so every UDP
   send carries an explicit seqno field. This is a *channel*-layer
   policy, not a codec rule; the codec encodes whatever
   `Header` it is given. Noted here so STEP 10 does not omit it.

3. **`relay_id` validation locus.** The JS `decrypt()` validates
   that an inbound `relayId` matches the channel's expected value
   (`zwift.mjs:1077-1080`). The plan keeps validation *out* of the
   codec: `decode_header` returns the parsed value, and the
   channel layer compares against its expected `relay_id`. Two API
   shapes were considered:
   - `decode_header(bytes)` returns the parsed header; the *caller*
     compares (chosen above for stateless purity).
   - `decode_header(bytes, expected_relay_id: u32)` validates inline.

   If the second proves ergonomically cleaner for
   STEP 10/11, switch; both are straightforward.

4. **`HelloKind` enum vs. `bool hello`.** The plan uses `bool` in
   `tcp_plaintext` to keep the API minimal. If reviewers find the boolean
   parameter unclear, replace it with an enum.

## Design decisions worth pre-committing

- **Stateless functions, not a `Codec` struct.** The IV state machine
  belongs to the channel (it owns `send_iv`, mutates `recv_iv` based
  on inbound headers, increments seqno post-encrypt/decrypt). Pushing
  state into this crate would make it harder to share an AES key
  across two channels (the spec hints at exactly this: TCP and UDP
  share a session key but have independent connId counters).
- **No async, no `tokio`.** Tested with no runtime. The reactor
  enters at STEP 10 / 11.
- **Single error type (`CodecError`).** All entry points return
  `Result<_, CodecError>`. The variants above cover every distinct
  failure mode visible to a caller.
- **`Vec<u8>` outputs, not generic over `Buf` / `BufMut`.** Adopting
  `bytes::BytesMut` is a future optimization once the channel layer
  exists and the project can measure whether allocator pressure matters.
  Otherwise this would be premature abstraction.

## Wiring into the workspace

- `crates/zwift-relay/` is picked up by the existing `members =
  ["crates/*"]` glob in the root `Cargo.toml`; no edit is needed there
  until a consumer (STEP 09) starts depending on it.
- The root `ranchero` crate gains a `zwift-relay = { path = "..." }`
  dep only when STEP 09 needs it. STEP 08 itself does not require
  any CLI surface; the existing `auth-check` diagnostic does not
  extend to relay-level state until STEP 09.
- License header `// SPDX-License-Identifier: AGPL-3.0-only` at the
  top of every `.rs` file (matches `zwift-proto` and `zwift-api`).
