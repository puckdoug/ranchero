# Step 06 — `zwift-proto` crate (stub)

## Goal

Generate Rust types for the Zwift protobuf schema with `prost-build`
during the workspace's build, against a **vendored** copy of the
`.proto` file checked into ranchero. This is the first step that
forces the workspace split (see `README.md` crate layout).

sauce4zwift is a porting reference, not a dependency: ranchero must
not have any build-time, test-time, or runtime path that resolves
through the sauce4zwift sibling checkout.

## One-time vendor

Copy `sauce4zwift/src/zwift.proto` once into
`crates/zwift-proto/proto/zwift.proto` and commit it. From this step
onward the proto file is maintained in ranchero's tree; the
sauce4zwift checkout (and its symlink) is not referenced by `build.rs`,
Cargo manifests, tests, or runtime asset paths.

## Sketch

- `crates/zwift-proto/build.rs` calls `prost_build::Config::new()`
  pointed at the vendored `proto/zwift.proto` (resolved relative to
  `CARGO_MANIFEST_DIR`).
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
- Vector tests: decode a captured `ServerToClient` byte dump (vendored
  under `crates/zwift-proto/tests/fixtures/`) and assert selected
  fields (athlete count, seqno, etc.) match.

To be fully elaborated when we start work on this step.
