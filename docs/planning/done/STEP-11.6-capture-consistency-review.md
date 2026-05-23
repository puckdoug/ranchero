# Step 11.6 — Capture and stream-logging consistency review

**Status:** review (2026-04-27).

## Summary checklist

The remediation items to be addressed in this step. Each item is
detailed inline below at the matching anchor. Items annotated with
`→ STEP 12` are appropriately deferred to that step and are listed
here only for tracking.

- [x] **A. Outdated comment in `zwift-relay/src/lib.rs`** — change
  the `currently stubs` annotation on the capture module to
  `implemented`. See [§Fix A](#fix-a--outdated-comment-in-zwift-relaysrclibrs).
- [x] **B. Stale `docs/plans/README.md` status table** — set the
  status for steps 04 through 11.5 to `☑`, change each link to
  point into `done/`, and add a row for **11.6**. See
  [§Fix B](#fix-b--docsplansreadmemd-status-table).
- [x] **C. `flush_and_close` drain semantics not pinned by a test**
  — add `flush_and_close_drains_pending_records` to
  `crates/zwift-relay/tests/capture.rs`. See
  [§Fix C](#fix-c--pin-flush_and_close-drain-semantics).
- [x] **D. `--capture` is silently ignored on `start`** —
  reject the flag in `cli::dispatch` with a clear "deferred to
  STEP 12" error, and add a test in `cli_args.rs` that asserts the
  message. See [§Fix D](#fix-d--reject---capture-on-start-until-step-12).
- [ ] ~~F.~~ Pass `Arc<CaptureWriter>` into the channel configurations — **→ STEP 12**.
- [ ] ~~G.~~ Establish a relay session and the UDP and TCP channels from the daemon — **→ STEP 12**.
- [ ] ~~H.~~ The supervisor must call `flush_and_close()` on shutdown (Finding 6) — **→ STEP 12**.

Note: the user manages git tracking of plan documents and the
movement of completed plans into `docs/plans/done/`; those
operations are not part of this step's scope.

Total scope of step 11.6: four changes (one comment edit, one
README edit, two new tests, and one source change). The net
production-source change is approximately six lines in `cli.rs`
plus roughly twenty-five lines of test code; the remaining
changes are documentation.

## Goal

Audit the work introduced in commits `bdba2cf` → `5f93bda` →
`8ba8e60` (STEP-11.5 wire capture and replay) against
`docs/plans/done/STEP-11.5-wire-capture.md`, and against the
broader expectation that the daemon should be able to connect to
Zwift and log the stream. Close the gaps that do not legitimately
belong to STEP-12 so that the supervisor work does not inherit
silent inconsistencies.

This document is a findings log and remediation plan; it does not
introduce new functionality.

## Build and test status (2026-04-27)

- `cargo build --workspace`: clean.
- `cargo test --workspace`: **416 tests pass**, 5 ignored (four
  keyring integration tests in `tests/credentials.rs`, one protobuf
  scenario in `crates/zwift-proto/tests/server_to_client.rs`).
  Test totals by binary:
  - `ranchero` lib: 172 · `cli_args`: 34 · `config`: 15 ·
    `credentials`: 19 · `daemon_lifecycle`: 8 · `logging`: 6 ·
    `tui`: 12
  - `zwift-api` `auth`: 9
  - `zwift-proto` `roundtrip`: 17 · `server_to_client`: 2
  - `zwift-relay` `capture`: 18 · `crypto`: 6 · `frame`: 12 ·
    `header`: 6 · `iv`: 3 · `session`: 15 · `tcp`: 17 ·
    `time_sync`: 5 · `udp`: 14 · `world_timer`: 5
- `cargo clippy --workspace --all-targets`: only pre-existing
  stylistic warnings (`large_enum_variant` on
  `ChannelEvent::Inbound(ServerToClient)`, two `collapsible_if`
  warnings, and one `clone_on_copy` in zwift-proto tests). No new
  warnings were introduced by the step 11.5 surface.
- Manual verification: `ranchero replay <file>` (both summary and
  `--verbose` modes) parses a synthesized capture and prints the
  specified output format. `ranchero --capture <path> start` does
  not open a capture file (see Finding 1 and Fix D).

## Cross-check of the STEP-11.5 plan against the implementation

| Plan item | Status | Notes |
|---|---|---|
| File format: ten-byte file header, fifteen-byte record header, little-endian payload | ✅ | `crates/zwift-relay/src/capture.rs:21-44` |
| `CaptureWriter::{open, open_with_capacity, record, dropped_count, flush_and_close}` | ✅ | `record()` is synchronous via `try_send`; `capture.rs:140-197` |
| `CaptureReader` synchronous `Iterator<Item = Result<…>>` and `version()` accessor | ✅ | `capture.rs:228-313` |
| `CaptureError` variants | ✅ | exact match (`capture.rs:105-124`) |
| Tap points: UDP receive after decryption and before decoding; UDP send after encoding and before envelope wrapping | ✅ | `udp.rs:242, 290, 392, 563` via `record_inbound` and `record_outbound` |
| Tap points: TCP receive after decryption and before decoding; TCP send after encoding and before envelope wrapping; outbound carries the `hello` flag | ✅ | `tcp.rs:231, 397` |
| `process_inbound{,_packet}` refactored to return `Vec<u8>` plaintext | ✅ | applied in both channels |
| `UdpChannelConfig::capture` and `TcpChannelConfig::capture` default to `None` | ✅ | `udp.rs:155`, `tcp.rs:106`; the "default-is-none" tests are present |
| Plan tests: format and header (4) | ✅ | all four are present in `tests/capture.rs` |
| Plan tests: round-trip (6) | ✅ | all six are present |
| Plan tests: truncation and error paths (4) | ✅ | all four are present |
| Plan tests: drop-on-saturation (2, including `writer_record_is_non_blocking`) | ✅ | both are present |
| Plan tests: capture-off zero overhead (2) | ✅ | both are present |
| Plan tests: channel tap (4) | ✅ | `udp_channel_with_capture_records_{inbound_packets, outbound_player_state}` and `tcp_channel_with_capture_records_{inbound_packets, outbound_packets_with_hello_flag}` |
| Plan tests: CLI parser (4) | ✅ | `start_with_capture_flag_captures_path`, `parses_replay_subcommand`, `parses_replay_with_verbose`, `dispatch_replay_stub` |
| `ranchero replay <path>` summary and `--verbose` modes | ✅ | manual verification confirms the specified format |
| SPDX `AGPL-3.0-only` header on new files | ✅ | present in `capture.rs` and `tests/capture.rs` |

The capture machinery itself, the four channel taps, and the
replay subcommand all conform to the plan. The remaining findings
concern wiring around the machinery rather than the machinery
itself.

## Findings

### 1 — `--capture <path>` is parsed but never opens a writer at runtime  ·  Fix D, with the remainder deferred to STEP 12

`cli::dispatch::Command::Start` (`src/cli.rs:181-187`) does not
read `cli.global.capture`. The function `daemon::start(&resolved,
foreground, log_opts)` (`src/daemon/mod.rs:85-91`) takes no
capture argument. Manual verification of `ranchero --foreground
--capture /tmp/wired.cap start` confirms that the daemon starts
and exits cleanly without creating a capture file.

The step 11.5 plan stated that `dispatch()` for `start` should:

1. Open the writer when the path is `Some`.
2. Pass the `Arc<CaptureWriter>` into both channel configurations
   via the STEP 12 supervisor.
3. Register a graceful-shutdown hook that calls
   `flush_and_close()`.

Item 2 is appropriately deferred to STEP 12, because no channels
exist in the daemon today. Items 1 and 3 fell within the scope of
step 11.5 but were not completed. With no channels available,
opening the writer would produce only a ten-byte file containing
the magic bytes and version; this provides no useful data. The
appropriate remediation in step 11.6 is therefore to reject the
flag at dispatch time (see [Fix D](#fix-d--reject---capture-on-start-until-step-12));
the actual wiring will be implemented as part of the supervisor
work.

### 2 — The daemon does not yet connect and log the stream  ·  → STEP 12

`daemon/runtime.rs:run_daemon` is still the STEP-03 placeholder
event loop over the Unix domain socket. A search of the source
tree finds no references to `RelaySession`, `UdpChannel`, or
`TcpChannel` anywhere under `src/`. The relay session, UDP
channel, TCP channel, and capture machinery are each individually
green and unit-tested, but ranchero today cannot produce a
non-empty capture against a live Zwift session.

This work belongs to STEP 12. The framing in the message of
commit `8ba8e60` ("completed work to log the stream") is true at
the level of the machinery (bytes flow through the four taps
when a channel is configured) but not at the end-to-end level.

### 3 — Outdated comment in `crates/zwift-relay/src/lib.rs:14-15`  ·  Fix A

```rust
// - Wire capture / replay: `CaptureWriter`, `CaptureReader`.
//   STEP 11.5; currently stubs.
```

The implementation is complete (`capture.rs` is 383 lines, fully
tested). The comment should read `STEP 11.5; implemented.` to
match the surrounding lines.

### 4 — `docs/plans/README.md` status column is significantly out of date  ·  Fix B

Steps 04 through 11.5 are all marked `☐ planned`, but each one
has been completed and resides under `done/`. The README itself
states:

> Update this README's status column when the step lands.

This practice has not been followed since step 04. Additionally,
the link column points to the in-progress paths
(`STEP-04-logging.md`, and similar) rather than `done/STEP-04-logging.md`.
Steps 03 and 06 already use the `done/` path, so the convention
is established but has not been applied to the other completed
steps.

### 5 — Open verification point #4 lacks regression coverage  ·  Fix C

The step 11.5 plan explicitly noted:

> closing while a record is mid-flight should not lose it (or should
> clearly drop + count).

`flush_and_close` (`capture.rs:177-196`) drops the sender and then
awaits the `JoinHandle`, which should drain the channel: the
writer task's `while let Some(record) = rx.recv().await { … }`
loop continues until the channel is empty and closed, after which
it flushes and calls `sync_all` (`capture.rs:199-209`). However,
no test asserts this contract. If a future refactor were to abort
the writer task before it finished draining, no existing test
would detect the regression.

### 6 — Dropping a `CaptureWriter` without `flush_and_close` is non-fatal but lossy  ·  → STEP 12 (must be addressed in the supervisor)

If a `CaptureWriter` is dropped instead of being passed to
`flush_and_close()` (for example on a panic, or as a result of a
programming error), the spawned writer task is detached. It will
eventually drain the channel and call `sync_all`, but the process
may exit before that work completes. This is acceptable for the
fixture use case described in step 11.5. The STEP-12 supervisor
must call `flush_and_close()` from its shutdown path; relying on
`Drop` would silently truncate captures.

### 7 — `--capture` is on `GlobalOpts` rather than `StartArgs`  ·  Will not address

`cli.rs:56-63` places the flag on the global options, so
`ranchero --capture x.cap status` parses successfully and
silently ignores the flag. The step 11.5 plan suggested placing
it on `StartArgs` instead. The help text mitigates this by noting
"(only meaningful with `start`)". This is a defensible choice:
clap's `global = true` behavior is consistent with the existing
`--config` flag in the same struct. The choice is documented here
and will not be changed.

---

## Concrete implementation steps

Each step lists the exact files, lines, and code to be changed.
The steps are independent and may be committed individually or
together. The recommended order follows the project's
test-driven-development practice: tests first for items C and D,
then the source change for D, and finally the documentation
changes A and B.

### Fix A — Outdated comment in `zwift-relay/src/lib.rs`

Edit `crates/zwift-relay/src/lib.rs` lines 14-15.

```diff
 // - Wire capture / replay: `CaptureWriter`, `CaptureReader`.
-//   STEP 11.5; currently stubs.
+//   STEP 11.5; implemented.
```

There is no test impact. Run `cargo build -p zwift-relay` to
confirm the file still compiles.

### Fix B — `docs/plans/README.md` status table

Edit `docs/plans/README.md`. Two columns require updates: the
status changes from `☐` to `☑` for steps 04 through 11.5, and
the link column gains the `done/` prefix where it is currently
absent.

Replace lines 31-39 of the current file with:

```markdown
|  04 | ☑ | Structured logging & verbose/debug flags | [STEP-04-logging.md](done/STEP-04-logging.md) |
|  05 | ☑ | Credential storage in OS keyring | [STEP-05-credentials.md](done/STEP-05-credentials.md) |
|  06 | ☑ | `zwift-proto` crate — prost-build against vendored zwift-offline proto tree (`crates/zwift-proto/proto/*.proto`, proto2) | [STEP-06-proto-crate.md](done/STEP-06-proto-crate.md) |
|  07 | ☑ | `zwift-api` — OAuth2 password grant + token refresh + REST client | [STEP-07-auth-and-rest.md](done/STEP-07-auth-and-rest.md) |
|  08 | ☑ | `zwift-relay` codec — header flags, `RelayIv`, AES-128-GCM-4 wire format | [STEP-08-relay-codec.md](done/STEP-08-relay-codec.md) |
|  09 | ☑ | Relay login (`/api/users/login`) + session refresh supervisor | [STEP-09-relay-session.md](done/STEP-09-relay-session.md) |
|  10 | ☑ | UDP channel with 25-shot hello handshake and world-time offset sync | [STEP-10-udp-channel.md](done/STEP-10-udp-channel.md) |
|  11 | ☑ | TCP channel with exponential backoff reconnect and watchdog | [STEP-11-tcp-channel.md](done/STEP-11-tcp-channel.md) |
| 11.5 | ☑ | Wire capture & replay — `ranchero start --capture <path>` + `ranchero replay`; produces the fixtures STEPS 08/18/19 consume | [STEP-11.5-wire-capture.md](done/STEP-11.5-wire-capture.md) |
| 11.6 | ☑ | Capture & stream-logging consistency review (this file) | [STEP-11.6-capture-consistency-review.md](done/STEP-11.6-capture-consistency-review.md) |
```

The 11.6 row links to the `done/` path. This file resides under
`docs/plans/done/`; the user manages plan-document movement and
git tracking.

There is no test impact. Verify by previewing the rendered
output in a Markdown renderer.

### Fix C — Pin `flush_and_close` drain semantics

Add a test to `crates/zwift-relay/tests/capture.rs`. Place it
immediately after `writer_record_is_non_blocking`, which is the
final test in the "drop-on-saturation" section (line 347 in the
current file).

```rust
// --- 4b. flush_and_close drain semantics ---------------------------

#[tokio::test]
async fn flush_and_close_drains_pending_records() {
    // Pins open verification point #4 of STEP-11.5: closing while
    // records are queued must drain them rather than truncate. The
    // requirement is that every accepted record (i.e. every push
    // that did not increment dropped_count) must be readable after
    // close.
    let path = NamedTempFile::new().expect("tempfile");
    let writer = CaptureWriter::open_with_capacity(path.path(), 4)
        .await
        .expect("open");

    let n = 100usize;
    for i in 0..n {
        writer.record(record_with_payload(
            Direction::Inbound,
            TransportKind::Udp,
            vec![(i & 0xFF) as u8; 16],
        ));
    }
    let dropped = writer.dropped_count() as usize;
    writer.flush_and_close().await.expect("close");

    let reader = CaptureReader::open(path.path()).expect("read");
    let count = reader.count();
    assert_eq!(
        count,
        n - dropped,
        "every accepted record must survive flush_and_close (n={n}, dropped={dropped}, recovered={count})",
    );
}
```

The expected outcome is that the test passes against the current
implementation, because `writer_task` already drains the channel
through `while let Some(...) = rx.recv()`. If the test fails on
the first run, that information is itself useful: the remediation
would be to ensure the writer task does not exit before the
channel is empty (the current implementation already does so; see
`capture.rs:203-209`).

Verify with:

```bash
cargo test -p zwift-relay --test capture flush_and_close_drains_pending_records
```

### Fix D — Reject `--capture` on `start` until STEP 12

This fix has two parts: a source-level guard in `cli::dispatch`,
and a parser test that asserts the guard fires. The test is
written first, in line with the project's test-driven-development
practice (see `MEMORY.md → feedback_tdd_workflow.md`).

#### D.1 — Failing test in `tests/cli_args.rs`

Place the new test after the existing `dispatch_replay_stub`
test (around line 250 in the current file). It reuses the public
`parse` and `dispatch` helpers already in scope.

```rust
#[test]
fn dispatch_start_with_capture_errors_until_step12() {
    // STEP 11.6, Fix D: --capture is parsed, but the supervisor
    // wiring is implemented in STEP 12. Return an error early
    // with a clear message rather than silently ignoring the
    // flag.
    let cli = parse(&["ranchero", "--capture", "/tmp/x.cap", "start"]);
    let err = ranchero::cli::dispatch(cli).expect_err("dispatch must reject");
    let msg = err.to_string();
    assert!(
        msg.contains("--capture") && msg.contains("STEP 12"),
        "error must reference both --capture and STEP 12; got: {msg}",
    );
}
```

`parse` is the existing helper that wraps `parse_from`; if it
is defined inside a `mod helpers { ... }` block, mirror the
call site used by `dispatch_replay_stub`.

#### D.2 — Source change in `src/cli.rs`

Add a guard near the top of `dispatch()`. Placing the guard at
the top of the function, rather than inside the `Command::Start`
arm, allows the guard to run before configuration is loaded; the
test in D.1 therefore does not require a working configuration
file.

```diff
 pub fn dispatch(cli: Cli) -> Result<ExitCode, Box<dyn std::error::Error>> {
     use crate::config::{self, OsEnv, ResolvedConfig, store::FileConfigStore};
     use crate::credentials::OsKeyringStore;
     use crate::daemon;
     use crate::tui;
+
+    if matches!(cli.command, Command::Start) && cli.global.capture.is_some() {
+        return Err(
+            "--capture is parsed but its supervisor wiring is implemented in STEP 12; \
+             see docs/plans/STEP-12-game-monitor.md"
+            .into(),
+        );
+    }

     match cli.command {
```

Verify with:

```bash
cargo test -p ranchero --test cli_args dispatch_start_with_capture_errors_until_step12
ranchero --capture /tmp/x.cap start   # must print the error and exit non-zero
```

When STEP 12 is implemented and the writer is wired in
properly, this guard must be removed: the supervisor will open
the writer in its place. The test from D.1 must then either be
deleted or rewritten to assert that `--capture` succeeds and
produces a non-empty capture file.

## Acceptance criteria

- `cargo test --workspace` passes; one new test in
  `crates/zwift-relay/tests/capture.rs`, and one new test in
  `tests/cli_args.rs`. The workspace total increases from 416 to
  418.
- `cargo build -p zwift-relay` succeeds with no warnings (Fix A
  is comment-only).
- `crates/zwift-relay/src/lib.rs:15` no longer contains the
  string `currently stubs`.
- `docs/plans/README.md` shows `☑` for every step from 03
  through 11.6 inclusive, and every link in those rows resolves
  to a file under `done/`.
- Manual verification: `ranchero --capture /tmp/x.cap start`
  exits with a non-zero status code and prints a stderr message
  containing both `--capture` and `STEP 12`.

## Deferred to STEP 12

| Item | Reason it belongs in STEP 12 |
|---|---|
| Pass `Arc<CaptureWriter>` into `UdpChannelConfig::capture` and `TcpChannelConfig::capture` (Finding 1, item 2) | The supervisor is the only component that constructs channels, so the wiring belongs in the supervisor. |
| Establish a relay session and the UDP and TCP channels from the daemon (Finding 2) | The `GameMonitor` is the orchestration layer (specification §4.8 and §4.13). |
| Call `flush_and_close()` from the supervisor's shutdown path (Finding 6) | The supervisor owns the lifecycle; the daemon's shutdown handler hands off to the supervisor. Relying on `Drop` truncates captures on a panic. |

When STEP 12 is implemented, this file should be revisited: the
guard introduced by Fix D must be removed, and each deferred
item above must be supported by a passing test.

## Cross-references

- `docs/plans/done/STEP-11.5-wire-capture.md` — the plan that
  this step audits.
- `docs/plans/STEP-12-game-monitor.md` — where the deferred
  items will be addressed.
- `docs/ARCHITECTURE-AND-RUST-SPEC.md` — §7.12 (no TCP
  keepalive; the application heartbeat is the liveness signal),
  §4.8 and §4.13 (the responsibilities of the GameMonitor).
