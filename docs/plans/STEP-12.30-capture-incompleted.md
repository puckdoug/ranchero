# STEP-12.30 — Capture file `follow` output is unusable; HTTP exchanges absent

**Status:** open (2026-05-09). Findings and implementation plan.

## 0. Progress checklist

### Phase 1 — Fix `follow`: decrypt frames; remove `--decode` flag

- [x] 1a — Failing tests: `follow_decrypts_outbound_tcp_frame`, `follow_decrypts_inbound_tcp_frame`, `follow_subcommand_has_no_decode_flag`
- [x] 1b — Implement decryption in `print_follow_to`; remove `decode` parameter and `--decode` CLI flag

### Phase 2 — Wire `CaptureSink`; extend capture format to v3 with content-type field

- [x] 2a — Failing test: `login_http_exchange_appears_in_capture`
- [x] 2b — Bump capture format to v3; add `ContentType` enum and `content_type` byte to record header
- [x] 2c — Add `CaptureContentType` enum to `zwift-api`; extend `CaptureSink::record()`; update all `record_outbound`/`record_inbound` call sites
- [x] 2d — Implement `HttpCaptureSink` in daemon; call `set_capture_sink` at construction and re-login sites

### Phase 3 — Fix `post_empty()` capture calls

- [x] 3 — Add `record_outbound`/`record_inbound` calls in `post_empty()`

### Phase 4 — Print Manifest records in `follow` output

- [x] 4a — Failing test: `follow_output_includes_manifest_summary`
- [ ] 4b — Print manifest summary line in `print_follow_to`

### Phase 5 — Decode HTTP record payloads in `follow` output

- [ ] 5a — Failing tests: `follow_http_json_payload_is_pretty_printed`, `follow_http_urlencoded_payload_is_displayed`, `follow_http_protobuf_payload_is_decoded`, `follow_http_empty_payload_prints_empty_marker`
- [ ] 5b — Dispatch on `record.content_type` to JSON / URL-encoded / protobuf / fallback decoders

### Phase 6 — Replace `{:#?}` with field-by-field display that omits absent fields

- [ ] 6a — Failing tests: `follow_output_contains_no_some_wrappers`, `follow_output_contains_no_none_fields`
- [ ] 6b — Add `prost-reflect` to `zwift-proto`; enable file descriptor generation; replace `{:#?}` in `print_follow_to` with a reflective field iterator that prints only present fields with unwrapped values

## 1. Observed symptom

Running `ranchero start --capture output.cap` followed by
`ranchero follow output.cap` produces output like:

```
ranchero follow output.cap
Format version: 2

  #     0  in  TCP  ts=1746789491000000000ns  len=  217
  #     1  out TCP  ts=1746789491000100000ns  len=   34  hello
  #     2  in  UDP  ts=1746789491000500000ns  len=   48
  …
```

Adding `--decode` appends `(decode error: …)` after every frame record.
All HTTP exchanges (token grant, profile fetch, player state, relay
session login) and all decoded protobuf messages visible in the daemon
log are absent from the output.

## 2. What the capture file actually stores

Per `docs/plans/STEP-12.12-log-shit-properly.md` §"Capture contract —
wire bytes, not plaintext," the capture file is specified to store the
exact encrypted bytes that crossed the socket. The `RNCWCAP\0` v2 file
contains two record kinds:

- **`RecordKind::Manifest`** — per-session decrypt material: AES-128
  key, device/channel discriminants, starting IV seqnos per direction
  per transport, relay_id, conn_id, expiry.
- **`RecordKind::Frame`** — the encrypted wire bytes for each TCP or UDP
  exchange.

The TCP/UDP call sites in `tcp.rs` and `udp.rs` are correctly positioned
at the wire-byte boundary as specified. There is nothing wrong with what
is written into the file.

## 3. Root causes (six independent defects)

### Defect A — `follow` never decrypts: it skips all Manifest records

and proto-decodes encrypted bytes directly

**Where:** `src/cli.rs:256-313`, `crates/zwift-relay/src/capture.rs:796-809`.

`print_follow_to` drives `CaptureFollower` through its `Iterator` impl.
The `Iterator::next` implementation at `capture.rs:799-809` explicitly
skips every `Manifest` record, yielding only `Frame` records. Because
the manifest is never surfaced, `print_follow_to` never holds the AES
key needed to decrypt frame payloads.

The `--decode` path at `cli.rs:294-309` calls
`ServerToClient::decode(record.payload.as_slice())` or
`ClientToServer::decode(…)` directly on the still-encrypted ciphertext.
Every call fails with a proto decode error. The metadata line (index,
direction, transport, timestamp, length) is derived from the record
header and is always visible; the payload contents are never shown. This
is why the output appears to contain only timestamps.

To decrypt a frame, `follow` must:

1. Call `CaptureFollower::next_item()` instead of iterating.
2. When a `Manifest` is returned, save it and reset per-direction
   per-transport seqno counters to the manifest's starting values.
3. When a `Frame` is returned, build a `RelayIv` from the saved
   manifest's conn_id, device_type, channel_type, and the current
   tracked seqno for that direction+transport; decrypt using AES-128-GCM
   with the manifest key; strip the plaintext envelope (TCP: 2 leading
   bytes `[version][hello]`; UDP: 1 leading byte `[version]`); then
   proto-decode the remaining bytes.
4. Increment the seqno counter after each successful decrypt. If the
   frame header carries an explicit seqno (hello frames and some inbound
   frames), use that value directly as in `tcp.rs:349-351`.

**Wire format note:** outbound TCP frame records store the full wire
frame including the 2-byte length prefix (`frame_tcp` output), so
`follow` must skip the first 2 bytes before parsing the header. Inbound
TCP frame records store only the body after the length prefix
(`payload_owned`), so no skip is needed. Confirm at implementation time
by cross-referencing `tcp.rs:282` vs `tcp.rs:412`.

### Defect B — The `--decode` flag is redundant and must be removed

`follow` exists to show the session's exchanged messages. Producing
encrypted hex blobs unless an extra flag is supplied is not useful
behavior. The flag is defined at `src/cli.rs:108-109`, threaded through
`Command::Follow` at line 236, `print_follow` at line 322, and
`print_follow_to` at line 258. All four sites must be changed to remove
the flag entirely; `follow` must always decrypt and decode.

### Defect C — HTTP exchanges are never written to the capture file

`zwift_api::ZwiftAuth` exposes a `set_capture_sink(Arc<dyn CaptureSink>)`
method (`crates/zwift-api/src/lib.rs:268`) and calls
`self.inner.record_outbound(…)` / `self.inner.record_inbound(…)` for
every HTTP exchange when a sink is present. In the daemon, `ZwiftAuth`
is constructed at `src/daemon/relay.rs:1192` and at line 1326
(`start_with_all_deps`). Neither site ever calls `set_capture_sink`.
No code path in `relay.rs` connects the `CaptureWriter` to the
`CaptureSink` slot on the `ZwiftAuth` object.

Consequence: token grant, profile fetch, player-state GET, relay session
login POST, and relay session refresh POST produce no records in the
capture file at all.

#### Sub-defect C2 — `post_empty()` is missing capture calls entirely

Even with a wired `CaptureSink`, the private `post_empty()` method at
`crates/zwift-api/src/lib.rs:819-857` (used by `logout()` and `leave()`)
makes its HTTP round-trip without calling `record_outbound` or
`record_inbound`. Every other method in that file calls them;
`post_empty` was overlooked.

### Defect E — Proto messages are printed using Rust's `Debug` format, exposing `Option<T>` wrappers

**Where:** `src/cli.rs` — the `writeln!(out, "{msg:#?}")` call in `print_follow_to`.

prost generates every proto optional field as `Option<T>` in Rust. Formatting a decoded message
with `{:#?}` has two consequences:

1. **All fields appear**, including those that are absent (`None`). A `ServerToClient` message
   has approximately 100 optional fields; a single record expands to hundreds of lines when most
   fields are unset.

2. **Present values are wrapped in `Some(...)`**. The output contains lines like
   `lb_realm: Some(1,)`, `world_time: Some(1000000)`, which is Rust-internal notation that
   is not meaningful to an operator reading session traffic.

Running `ranchero follow output.cap` against a 228-record capture produced 121,000 lines of output
from this single formatting choice.

The correct fix is to iterate over only the fields that are actually present in the decoded message
and print their values directly, without the `Option` wrapper. This must be done at the point where
the decoded message is formatted — not by post-processing the text that `{:#?}` emits.

`zwift-proto` carries only `Clone`, `PartialEq`, and `::prost::Message` derives. It has no
`serde` derives and no `prost-reflect` support. Field-level reflection requires adding
`prost-reflect` to `zwift-proto` and enabling file descriptor generation in its `build.rs`.

### Defect D — Manifest records are invisible in `follow` output

Even after fixing A, the manifest itself is not printed. Operators
cannot confirm the session key was written or inspect its content
without a separate tool. The `follow` output would be more transparent
if each manifest produced a summary line.

## 4. What is currently being captured

| Record                             | Written at                      | Status                                               |
| ---------------------------------- | ------------------------------- | ---------------------------------------------------- |
| `Manifest`                         | `relay.rs:1682`, `1734`, `1758` | Written, but silently skipped by `follow`            |
| Outbound TCP frames (encrypted)    | `tcp.rs:282`                    | Written, but encrypted and not decrypted by `follow` |
| Inbound TCP frames (encrypted)     | `tcp.rs:412`                    | Written, but encrypted and not decrypted by `follow` |
| Outbound UDP datagrams (encrypted) | `udp.rs:271`, `udp.rs:463`      | Written, but encrypted and not decrypted by `follow` |
| Inbound UDP datagrams (encrypted)  | `udp.rs:300`, `udp.rs:595`      | Written, but encrypted and not decrypted by `follow` |
| HTTP exchanges                     | —                               | Not written (defect C)                               |
| `POST /api/users/logout`           | —                               | Not written (defect C2)                              |
| `POST /relay/worlds/1/leave`       | —                               | Not written (defect C2)                              |

## 5. Implementation plan

Four phases in dependency order. Each phase is a TDD pair.

### Phase 1 — Fix `follow`: decrypt frames using manifest; remove `--decode` flag

#### 1a — Failing tests

Add to `tests/cli_follow.rs` (or the existing follow test module):

- **`follow_decrypts_outbound_tcp_frame`** — writes a capture file
  containing a `Manifest` record followed by a synthetic outbound TCP
  frame built with the same AES key and IV; runs `print_follow_to`;
  asserts the output contains a successfully decoded `ClientToServer`
  payload (or at minimum that no `(decode error:` line appears).
  Currently RED: `follow` skips the manifest and fails to decrypt.

- **`follow_decrypts_inbound_tcp_frame`** — same pattern for a
  synthetic inbound TCP frame; asserts a `ServerToClient` decode
  succeeds. Currently RED.

- **`follow_decode_flag_does_not_exist`** — asserts that
  `print_follow_to` no longer accepts a `decode` parameter (compile
  test) and that the `follow` sub-command has no `--decode` argument.
  Currently RED: the flag still exists.

#### 1b — Implementation

1. Remove the `decode: bool` parameter from `print_follow_to` and
   `print_follow` at `src/cli.rs:258` and `322`. Update the call site
   at `cli.rs:237`.

2. Remove `decode: bool` from `Command::Follow` at `cli.rs:108-109`
   and its destructure at `cli.rs:236`.

3. In `print_follow_to`, replace `for (idx, result) in follower.enumerate()`
   with a loop driven by `follower.next_item()`. Maintain local state:

   ```
   let mut manifest: Option<SessionManifest> = None;
   let mut seqno_out_tcp: u32 = 0;
   let mut seqno_in_tcp:  u32 = 0;
   let mut seqno_out_udp: u32 = 0;
   let mut seqno_in_udp:  u32 = 0;
   ```

   When `CaptureItem::Manifest(m)` is returned: save to `manifest`,
   reset the four counters from `m.send_iv_seqno_tcp`,
   `m.recv_iv_seqno_tcp`, `m.send_iv_seqno_udp`, `m.recv_iv_seqno_udp`.

4. When `CaptureItem::Frame(rec)` is returned for TCP or UDP, if
   `manifest` is `Some`:
   - Select the appropriate seqno counter (direction × transport).
   - Build a `RelayIv { device, channel, conn_id: manifest.conn_id as u16, seqno }`.
     Device/channel values per direction: outbound TCP → `TcpClient`,
     inbound TCP → `TcpServer`; outbound UDP → confirm from `udp.rs:260`
     and `udp.rs:447`; inbound UDP → confirm from `udp.rs:562`.
   - Parse the wire frame to extract header (AAD) and ciphertext. For
     outbound TCP, skip the 2-byte length prefix first; for inbound TCP,
     no skip. Use `decode_header` to locate the boundary.
   - Decrypt with `decrypt(&manifest.aes_key, &iv.to_bytes(), aad, cipher)`.
   - Strip envelope bytes with `parse_tcp_plaintext` or
     `parse_udp_plaintext` to reach the raw proto bytes.
   - Proto-decode and print. If decryption or decode fails, print an
     error line with detail.
   - Increment the seqno counter. If the frame header carried an explicit
     seqno, use that value before incrementing.

5. For `TransportKind::Http` frame records, print the payload length and
   direction; no decryption or proto decode is applied (HTTP bodies are
   not proto — see §7).

Verify with Phase 1a tests plus `cargo test --workspace`.

### Phase 2 — Fix defect C: wire `CaptureSink` into the daemon's `ZwiftAuth`; extend capture format to v3 with content-type field

Every HTTP record written today carries no information about its payload
encoding. Rather than having `follow` guess (fragile, slow), the record
header stores the content type alongside direction and transport so the
reader always knows exactly what it has.

#### 2a — Failing test

- **`login_http_exchange_appears_in_capture`** — drives
  `RelayRuntime::start_with_all_deps` with standard stub factories and
  a real `CaptureWriter`; reads the resulting capture file via
  `CaptureReader::next_item`; asserts at least one
  `TransportKind::Http` `Frame` record exists with a non-empty payload
  and a `ContentType` field that is not `Unspecified`.
  Currently RED: no HTTP records are written.

#### 2b — Format extension: capture v3

Add a `content_type` byte to the record header, bumping the format to
v3. The v3 layout is:

```
ts_unix_ns(8) + kind(1) + direction(1) + transport(1) + flags(1)
  + content_type(1) + len(4)
= 17 bytes
```

In `crates/zwift-relay/src/capture.rs`:

1. Bump `VERSION` from `2` to `3`. Update `RECORD_HEADER_LEN` from `16`
   to `17`. Update the constant doc comment to describe the v3 layout.

2. Add a `ContentType` enum with `as_byte`/`from_byte` conversions:

   ```rust
   pub enum ContentType {
       Unspecified     = 0,  // TCP/UDP wire frames
       Json            = 1,
       UrlEncoded      = 2,  // application/x-www-form-urlencoded
       ProtobufLite    = 3,  // application/x-protobuf-lite
       Empty           = 4,  // zero-length body, explicit marker
   }
   ```

3. Add `content_type: ContentType` to `CaptureRecord` (default
   `Unspecified` for TCP/UDP call sites — no existing call site needs
   to change).

4. Update the writer (`writer_task`) to encode `content_type.as_byte()`
   at the new offset in the record header.

5. Update `CaptureFollower::next_item` and `CaptureReader::next_item`
   to read the new byte and populate `CaptureRecord::content_type`.

#### 2c — `CaptureSink` extension in `zwift-api`

In `crates/zwift-api/src/lib.rs`:

1. Add `CaptureContentType` enum mirroring the relay-side values:

   ```rust
   pub enum CaptureContentType {
       Unspecified,
       Json,
       UrlEncoded,
       ProtobufLite,
       Empty,
   }
   ```

2. Extend `CaptureSink::record` to include it:

   ```rust
   fn record(
       &self,
       direction: CaptureDirection,
       transport: CaptureTransport,
       content_type: CaptureContentType,
       payload: &[u8],
   );
   ```

3. Update `ZwiftAuthInner::record_outbound` and `record_inbound` to
   accept and forward a `CaptureContentType` argument.

4. Update every call site in `lib.rs` to pass the correct value:

   | Method                    | Direction | Content type                                                                                                                            |
   | ------------------------- | --------- | --------------------------------------------------------------------------------------------------------------------------------------- |
   | `token_grant` outbound    | Outbound  | `UrlEncoded`                                                                                                                            |
   | `token_grant` inbound     | Inbound   | `Json`                                                                                                                                  |
   | `get_profile_me` outbound | Outbound  | `Empty`                                                                                                                                 |
   | `get_profile_me` inbound  | Inbound   | `Json`                                                                                                                                  |
   | `fetch` (GET) outbound    | Outbound  | `Empty`                                                                                                                                 |
   | `fetch` (GET) inbound     | Inbound   | confirm at call site (JSON or ProtobufLite)                                                                                             |
   | `post` outbound           | Outbound  | `ProtobufLite` if `content_type` param starts with `application/x-protobuf-lite`; else `UrlEncoded`/`Json`/`Unspecified` as appropriate |
   | `post` inbound            | Inbound   | mirror from the request's `is_protobuf` flag: `ProtobufLite` or `Json`                                                                  |
   | `post_empty` outbound     | Outbound  | `Empty`                                                                                                                                 |
   | `post_empty` inbound      | Inbound   | `Empty`                                                                                                                                 |
   | `refresh` (internal)      | per call  | confirm at implementation time                                                                                                          |

#### 2d — Daemon adapter

1. Add a private `HttpCaptureSink(Arc<CaptureWriter>)` struct in
   `src/daemon/relay.rs` implementing `zwift_api::CaptureSink`:

   ```rust
   impl zwift_api::CaptureSink for HttpCaptureSink {
       fn record(
           &self,
           direction: zwift_api::CaptureDirection,
           _transport: zwift_api::CaptureTransport,
           content_type: zwift_api::CaptureContentType,
           payload: &[u8],
       ) {
           use zwift_relay::capture::{CaptureRecord, ContentType, Direction, TransportKind};
           self.0.record(CaptureRecord {
               ts_unix_ns: now_unix_ns(),
               direction: match direction {
                   zwift_api::CaptureDirection::Outbound => Direction::Outbound,
                   zwift_api::CaptureDirection::Inbound  => Direction::Inbound,
               },
               transport: TransportKind::Http,
               content_type: match content_type {
                   zwift_api::CaptureContentType::Json         => ContentType::Json,
                   zwift_api::CaptureContentType::UrlEncoded   => ContentType::UrlEncoded,
                   zwift_api::CaptureContentType::ProtobufLite => ContentType::ProtobufLite,
                   zwift_api::CaptureContentType::Empty        => ContentType::Empty,
                   zwift_api::CaptureContentType::Unspecified  => ContentType::Unspecified,
               },
               hello: false,
               payload: payload.to_vec(),
           });
       }
   }
   ```

2. At `relay.rs:1192` and `relay.rs:1326`, after constructing `auth`,
   if `capture_writer` is `Some(writer)`:

   ```rust
   auth.set_capture_sink(Arc::new(HttpCaptureSink(Arc::clone(writer))));
   ```

3. At `relay.rs:1734` and `relay.rs:1758` (re-login / refresh), call
   `set_capture_sink` again so the new session's HTTP exchanges reach
   the same file.

### Phase 3 — Fix defect C2: add capture calls to `post_empty()`

In `crates/zwift-api/src/lib.rs`, in `post_empty()`, add:

```rust
self.inner.record_outbound(&[]);  // before send()
// … existing send / bytes …
self.inner.record_inbound(&body_bytes);  // after resp.bytes()
```

Mirror the exact pattern from every other method in the same file.
No new tests needed; the regression guard in Phase 2a covers this path.

### Phase 4 — Print Manifest records in `follow` output

#### 4a — Failing test

- **`follow_output_includes_manifest_summary`** — writes a Manifest
  record to a capture file; runs `print_follow_to`; asserts the output
  string contains `"Manifest"` and the relay_id value.
  Currently RED: manifests are silently skipped.

#### 4b — Implementation

Phase 1 already switches `print_follow_to` to `next_item()`. In that
loop, when `CaptureItem::Manifest(m)` is returned, write:

```
  Manifest  relay_id=42  conn_id=7  key=<16-byte hex>  expires=2026-05-09T18:00:00Z
```

The AES key should be hex-encoded (useful for offline tooling). The
expiry should be formatted as human-readable UTC from `expires_at_unix_ns`.

### Phase 5 — Decode HTTP record payloads in `follow` output

Phase 2 stores a `ContentType` field in every HTTP record. `follow`
reads that field and dispatches directly to the correct decoder; no
heuristics are needed.

#### 5a — Failing tests

- **`follow_http_json_payload_is_pretty_printed`** — writes a capture
  file with an inbound HTTP frame whose `content_type` is `Json` and
  whose payload is a JSON object; runs `print_follow_to`; asserts the
  output contains the top-level JSON key.
  Currently RED: HTTP frames print only the summary line.

- **`follow_http_urlencoded_payload_is_displayed`** — writes an outbound
  HTTP frame with `content_type = UrlEncoded` and payload
  `client_id=foo&grant_type=password`; asserts the output contains
  `grant_type=password` in readable form.
  Currently RED.

- **`follow_http_protobuf_payload_is_decoded`** — writes an outbound
  HTTP frame with `content_type = ProtobufLite` and a valid encoded
  protobuf body; asserts the output contains a decoded field value.
  Currently RED.

- **`follow_http_empty_payload_prints_empty_marker`** — writes an HTTP
  frame with `content_type = Empty`; asserts the output line contains
  `(empty)`. Currently RED.

#### 5b — Implementation

In `print_follow_to`, for `TransportKind::Http` frame records, after
writing the summary line, dispatch on `record.content_type`:

- `ContentType::Empty` → print `  (empty)`.

- `ContentType::Json` → `serde_json::from_slice::<serde_json::Value>`
  and `serde_json::to_string_pretty`; print the pretty-printed JSON
  indented by two spaces. On failure print the raw bytes as lossy UTF-8.

- `ContentType::UrlEncoded` → `form_urlencoded::parse` and print each
  `key=value` pair on its own line. On failure print the raw bytes as
  lossy UTF-8.

- `ContentType::ProtobufLite` → attempt protobuf decode with the known
  HTTP proto types used for relay login/refresh and player state.
  Confirm the exact types from the call sites in `relay.rs` at
  implementation time. Print the first successful `{msg:#?}` result.
  On total failure print a hex dump (16 bytes per line).

- `ContentType::Unspecified` → print raw bytes as lossy UTF-8, or a
  hex dump if non-printable.

### Phase 6 — Replace `{:#?}` debug output with human-readable field display

#### 6a — Failing tests

- **`follow_output_contains_no_some_wrappers`** — writes a capture file with a Manifest record
  and a synthetic outbound TCP frame encoding a `ClientToServer` message with `seqno` set to 7;
  runs `print_follow_to`; asserts that the output does **not** contain the substring `"Some("`.
  Currently RED: `{:#?}` wraps every present value in `Some(...)`.

- **`follow_output_contains_no_none_fields`** — same setup; asserts the output does **not**
  contain the substring `"None"`.
  Currently RED: `{:#?}` prints every absent optional field as `None`.

Both tests should also assert that the output does contain the expected field value (`"7"` for
`seqno`) to confirm that present fields are still displayed.

#### 6b — Implementation

1. Add `prost-reflect` to `crates/zwift-proto/Cargo.toml`:

   ```toml
   [dependencies]
   prost-reflect = { version = "0.14", features = ["derive"] }

   [build-dependencies]
   prost-reflect-build = "0.14"
   ```

2. In `crates/zwift-proto/build.rs`, replace or extend the existing `prost_build` call to use
   `prost_reflect_build::Builder`, which emits a `file_descriptor_pool` alongside the generated
   structs. The generated types will gain a `::prost_reflect::ReflectMessage` derive, providing
   a `.transcode_to_dynamic()` method that returns a `DynamicMessage`.

3. In `src/cli.rs`, replace the `writeln!(out, "{msg:#?}")` call with a helper function
   `print_message<W: io::Write>(out: &mut W, msg: &impl prost_reflect::ReflectMessage)` that:
   - Calls `msg.transcode_to_dynamic()` to obtain a `DynamicMessage`.
   - Iterates over `dynamic_msg.fields()`, which yields only present fields.
   - For each `(FieldDescriptor, Value)` pair, writes `  field_name: display_value\n` to `out`.
   - Recurse for nested `Message` values; print scalar values (`u32`, `i32`, `string`, etc.)
     directly without any wrapper.

4. Replace all `writeln!(out, "{msg:#?}")` sites in `print_follow_to` with calls to
   `print_message(&mut out, &msg)`.

Verify with the Phase 6a tests plus the full `cargo test --workspace`.

## 6. Verification gate

After all four phases:

```
ranchero start --capture output.cap
sleep 10
ranchero follow output.cap
ranchero stop
```

Expected output from `follow`:

- A `Manifest` line near the top.
- `in  HTT` / `out HTT` records for the token grant, profile fetch, and
  relay login — each followed by the decoded payload (JSON pretty-printed,
  URL-encoded form as key=value pairs, or decoded protobuf as appropriate).
- `in  TCP` / `out TCP` records with successfully decoded proto printed
  (no `(decode error: …)` lines).
- `in  UDP` / `out UDP` records with successfully decoded proto.
- After `ranchero stop`: `in  HTT` / `out HTT` records for logout and
  leave, each showing `(empty)`.

## 7. Out of scope for this plan

- **The `TransportKind::Http` variant in `zwift_relay::capture`.** It
  already exists and is wired into the file format; no format change
  is needed beyond the v3 header extension in Phase 2.
- **The `SessionManifest` AES key retention.** The key is retained for
  any out-of-process tool that wants to reproduce the exact encrypted
  wire bytes. The stored frame bytes remain encrypted wire bytes, which
  is unchanged from the v3 format.
