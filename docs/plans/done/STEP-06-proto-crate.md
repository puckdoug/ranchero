# Step 06 — `zwift-proto` crate (stub)

## Goal

Generate Rust types for the Zwift protobuf schema with `prost-build`
during the workspace's build, against a **vendored** copy of the
upstream protobuf tree from
[`zoffline/zwift-offline`](https://github.com/zoffline/zwift-offline)
(AGPL-3.0). This is the first step that forces the workspace split
(see `README.md` crate layout).

sauce4zwift's `src/zwift.proto` is a heavily modified single-file
proto3 fork (renamed messages, camelCase, dropped proto2 presence
semantics). It is **not** the source. We pull from upstream directly
to stay close to Zwift's actual wire format and to avoid
derivative-of-a-derivative drift.

## Licensing

Upstream is AGPL-3.0. ranchero is therefore AGPL-3.0 (GPL-3.0
inherited from sauce4zwift, upgraded by combining with AGPL upstream
— compatible, one-way). The `zwift-proto` crate carries an SPDX
`AGPL-3.0-only` header and the project-level LICENSE matches.

## One-time vendor

- Source: `https://github.com/zoffline/zwift-offline/tree/master/protobuf`
- Vendor location: `crates/zwift-proto/proto/`
- Layout: mirror upstream's filenames; do **not** merge into a single
  file. proto2 multi-file with `import` between files is the upstream
  convention and `prost-build::compile_protos` handles it natively.

Files to vendor for the live-data core (subset of upstream's 19):

| Upstream file | Why we need it |
|---|---|
| `login.proto` | `LoginRequest`, `LoginResponse`, `RelaySessionRefreshResponse` |
| `per-session-info.proto` | `PerSessionInfo`, `TcpConfig`, `TcpAddress` |
| `udp-node-msgs.proto` | `ClientToServer`, `ServerToClient`, `PlayerState`, `WorldAttribute`, `WA_TYPE`, `RideOn`, `PlayerLeftWorld`, `UdpConfig`, `RelayAddress`, `UdpConfigVOD`, `RelayAddressesVOD` |
| `tcp-node-msgs.proto` | TCP-side message variants |
| `events.proto` | `Event`, `EventSubgroup` |
| `segment-result.proto` | `SegmentResult` |
| `profile.proto` | `PlayerProfile`, `Sport`, enums imported by other files |

Plus any additional file pulled in transitively by `import` statements
in the above.

After vendoring, the file is maintained in ranchero's tree. Updates
are made by copying fresh from upstream and re-running codegen — no
sauce4zwift checkout, no upstream `git submodule`, no
build/test/runtime path that resolves outside ranchero.

## Sketch

- `crates/zwift-proto/build.rs` calls
  `prost_build::Config::new().compile_protos(&files, &[proto_root])`
  where `files` is the explicit list above (paths relative to
  `CARGO_MANIFEST_DIR`) and `proto_root` is `proto/` so `import`
  resolution works.
- Use `syntax = "proto2"` as found in upstream — do not convert.
- Expose the messages downstream crates need via `pub use` so callers
  don't have to navigate generated module paths. Working list (subject
  to refinement during the elaboration pass): `LoginRequest`,
  `LoginResponse`, `ClientToServer`, `ServerToClient`, `PlayerState`,
  `WorldAttribute`, `WA_TYPE`, `TcpConfig`, `TcpAddress`, `UdpConfig`,
  `RelayAddress`, `UdpConfigVOD`, `RelayAddressesVOD`, `SegmentResult`,
  `RideOn`, `PlayerLeftWorld`, `Event`, `EventSubgroup`, `Segment`.
  (Note: `PlayerJoinedWorld` from sauce4zwift's list does not appear
  by that name upstream — verify the corresponding upstream payload
  during elaboration.)
- **Field naming.** Upstream is snake_case (proto convention). prost
  also produces snake_case Rust fields. v2 payload formatters
  (STEP 18) need camelCase on the JSON wire to keep widgets working —
  add `serde` rename attributes in the formatter layer rather than
  fighting the generated types (per spec §7.12 footgun).

## Tests-first outline

- Compile test: types round-trip via `prost::Message::encode` /
  `Message::decode`.
- Vector tests: decode a captured `ServerToClient` byte dump
  (vendored under `crates/zwift-proto/tests/fixtures/`) and assert
  selected fields (athlete count, seqno, etc.) match.

To be fully elaborated when we start work on this step.
