# Step 06 — `zwift-proto` crate (stub)

## Goal

Generate Rust types for the Zwift protobuf schema with `prost-build`
during the workspace's build, pointed at
`sauce4zwift/src/zwift.proto`. This is the first step that forces the
workspace split (see `README.md` crate layout).

## Sketch

- `crates/zwift-proto/build.rs` calls `prost_build::Config::new()`,
  points at the proto file via the symlink.
- Expose specific messages via `pub use` so downstream crates don't need
  to know module paths: `LoginRequest`, `LoginResponse`, `ClientToServer`,
  `ServerToClient`, `PlayerState`, `WorldUpdate`, `WorldUpdatePayloadType`,
  `TCPServer`, `UDPServer`, `TCPConfig`, `UDPConfig`, `UDPConfigVOD`,
  `UDPServerVODPool`, `SegmentResult`, `RideOn`, `PlayerJoinedWorld`,
  `PlayerLeftWorld`, `Event`, `EventSubgroup`, `Segment`.
- **`keepCase` equivalent** — prost uses snake_case; add serde attributes
  where needed so v2 payload formatters (STEP 18) can emit the exact
  casing widgets expect (per spec §7.12 footgun).

## Tests-first outline

- Compile test: types round-trip via `prost::Message::encode` /
  `Message::decode`.
- Vector tests: decode a captured `ServerToClient` byte dump from the JS
  client and assert selected fields (athlete count, seqno, etc.) match.

To be fully elaborated when we start work on this step.
