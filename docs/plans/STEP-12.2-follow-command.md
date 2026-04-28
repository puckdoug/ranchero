# Step 12.2 — `ranchero follow` command for live capture-file tailing

**Status:** planned (2026-04-28).

This is the second sub-step of STEP-12 (`STEP-12-game-monitor.md`).
It does not change the daemon or the protocol stack. It adds a
new top-level command, `ranchero follow <file>`, that reads a
wire-capture file as it is being written and prints each record
to standard output as text. The intended use is paired with
`ranchero start --capture <path>` (delivered by STEP-12.1) so
that a developer or operator can watch the live stream from a
second terminal while the daemon runs.

## Goal

A reader that opens a capture file, prints each existing record
to standard output, and then continues to print new records as
the writer appends them. The reader exits cleanly on `Ctrl-C`,
on an end-of-stream signal, or after a configurable inactivity
timeout. With an optional decode flag, the reader also runs
each payload through the `prost` decoder for the appropriate
direction and prints the decoded message fields.

The deliverable is a tool that turns a binary capture file into
a streaming text view in real time, suitable for confirming
during live validation that the daemon is producing the traffic
the log claims it is producing, and useful as a general
protocol-level debugging aid.

## Motivation

The verb `follow` is preferred to `replay --follow` because
"replay" implies after-the-fact processing of a complete file,
whereas this command is intended for a live, ongoing stream.
The two commands also differ in their exit conditions and
output cadence: `replay` runs to end-of-file and exits; `follow`
runs until interrupted.

## Scope

In scope:

- A `CaptureFollower` type alongside the existing
  `CaptureReader`. It reuses the same file format, the same
  error variants, and the same record type, but its iteration
  semantics differ at the end-of-file boundary.
- A new top-level `follow` subcommand on the `ranchero` CLI.
- A polling-based retry loop on end-of-file: when the reader
  reaches the end of the file, it sleeps for a short interval
  (default 100 ms) and retries. When the reader encounters a
  truncated-record condition mid-record, it treats it the same
  way: sleep, retry, and resume reading at the same offset.
- A `--decode` flag that, instead of the one-line summary,
  decodes each record's payload as `ServerToClient` (for
  inbound) or `ClientToServer` (for outbound) and prints the
  decoded message.
- A `--idle-timeout <seconds>` flag that exits the follower
  after a configurable period without observing a new record.
  The default is no timeout (run until interrupted).
- Graceful exit on `Ctrl-C`: the follower returns from its
  loop and the CLI dispatcher exits with status zero.

Out of scope:

- File-system event notification (`inotify` on Linux,
  `kqueue` on macOS). Polling at 100 ms is more than fast
  enough for the protocol's record cadence and avoids the
  platform-specific code. A future enhancement could swap to
  event-driven notification without changing the public
  surface.
- A persistent, restart-on-rotation mode. The capture format
  does not rotate (per STEP-11.5's design decision); one
  `ranchero start` invocation produces one file. If the
  capture file is replaced under the follower, the follower
  will exit at the next read error.
- A JSON output mode. The default is human-readable text. A
  future `--json` flag could be added later.
- Filtering by direction, transport, or message type. A
  developer can pipe the output through `grep` or similar; a
  built-in filter is left for a future enhancement if needed.

## Module layout

The follower lives next to the existing reader. There are two
acceptable layouts; the implementer chooses based on size at
the time:

- **Inline in `capture.rs`** — add `CaptureFollower` and
  related helpers to the existing file. Acceptable if the
  total file size remains under approximately 600 lines.
- **Split into a `capture/` subdirectory** — fold
  `capture.rs` into `capture/mod.rs`, `capture/format.rs`,
  `capture/writer.rs`, `capture/reader.rs`,
  `capture/follower.rs`. This was anticipated in the
  STEP-11.5 plan as the layout to adopt once the module
  develops independent sub-concerns.

The CLI surface change is in `src/cli.rs`: add a `Follow`
variant on the `Command` enum and a corresponding dispatch
arm.

## Public API surface (proposed)

```rust
// crates/zwift-relay/src/capture.rs (or capture/follower.rs)

/// Tailing reader over a wire-capture file. Like
/// [`CaptureReader`], but on end-of-file or a truncated-record
/// condition, the iterator sleeps and retries rather than
/// returning `None` or an error. Exits when the writer
/// signals that no further records will arrive (the file is
/// closed and a configured idle timeout elapses) or when the
/// caller drops the iterator.
pub struct CaptureFollower {
    reader:        std::io::BufReader<std::fs::File>,
    version:       u16,
    poll_interval: Duration,
    idle_timeout:  Option<Duration>,
}

impl CaptureFollower {
    /// Open `path`, validate the file header, return a
    /// follower with default tuning (`poll_interval = 100 ms`,
    /// `idle_timeout = None`).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CaptureError>;

    /// Override the polling interval used between
    /// end-of-file retries. Lower values reduce latency at
    /// the cost of CPU; the default is suitable for the
    /// protocol's record cadence.
    pub fn with_poll_interval(mut self, interval: Duration) -> Self;

    /// Set an idle timeout. When the follower has not
    /// observed a new record for this duration, the iterator
    /// returns `None`. The default is no timeout (the
    /// follower runs until the caller drops it).
    pub fn with_idle_timeout(mut self, timeout: Option<Duration>) -> Self;

    /// Format version from the file header (currently always 1).
    pub fn version(&self) -> u16;
}

impl Iterator for CaptureFollower {
    type Item = Result<CaptureRecord, CaptureError>;
    fn next(&mut self) -> Option<Self::Item>;
}
```

The follower is synchronous, matching `CaptureReader`. The CLI
dispatcher handles `Ctrl-C` by installing a signal handler
that drops the follower; the iterator's blocking sleep is
short enough (100 ms by default) that signal latency is
imperceptible.

## CLI surface (proposed)

```rust
// src/cli.rs — extend the existing Command enum

pub enum Command {
    // (existing variants …)

    /// Tail a wire-capture file and print each record to
    /// standard output as it is written.
    Follow {
        /// Path to the capture file.
        path: PathBuf,
        /// Decode each payload as `ServerToClient` (inbound) or
        /// `ClientToServer` (outbound) and print the decoded
        /// message instead of a one-line summary.
        #[arg(long)]
        decode: bool,
        /// Exit after this many seconds without a new record.
        /// Default: run until interrupted.
        #[arg(long, value_name = "SECONDS")]
        idle_timeout: Option<u64>,
    },
}
```

The dispatch arm reads the file with `CaptureFollower::open`,
applies the supplied options, and iterates. On each record,
the dispatcher emits one of two output forms:

**Default (summary):** the same one-line shape as
`replay --verbose`, suitable for piping through `grep`:

```
  #     0  in  UDP  ts=1700000000000000000ns  len=   11
  #     1  in  TCP  ts=1700000000004500000ns  len=  142
  #     2  out UDP  ts=1700000000010000000ns  len=   38
```

**With `--decode`:** a multi-line block per record showing
the decoded protobuf fields. The exact formatting is the
`Debug` representation of the decoded message, which is
verbose but precise:

```
  #     0  in  UDP  ts=1700000000000000000ns  len=   11
ServerToClient {
    world_time: Some(0),
    seqno: Some(1),
    ...
}
```

A future enhancement could emit a more selective format (a
chosen field subset, a JSON encoding, or a single-line
summary derived from message contents); for the first
version, `Debug` is adequate.

## Tests-first plan

### `crates/zwift-relay/tests/capture.rs` extensions

| Test | Asserts |
|---|---|
| `follower_reads_records_as_they_are_written` | Spawn a `CaptureWriter` in a background task that pushes one record every 100 ms for 1 s. A `CaptureFollower` opened on the same file observes all ten records in order, with `next()` returning each one as it becomes available. |
| `follower_resumes_after_truncated_record_at_eof` | Manually write a valid file header followed by partial bytes of a record header (5 of 15). `CaptureFollower::next()` does not return; on a separate task, complete the record header and write the payload after a delay; the original `next()` call resolves with the complete record. |
| `follower_idle_timeout_returns_none` | Open a follower with `idle_timeout = Some(50 ms)` on a file that contains the file header but no records. `next()` returns `None` after roughly the timeout elapses. |
| `follower_no_idle_timeout_blocks_indefinitely` | Open a follower with no idle timeout on a file that contains the file header but no records. Spawn a thread that drops the follower after 200 ms; `next()` returns at that point with no further iteration. |
| `follower_rejects_bad_magic` | A file written with non-magic bytes returns `Err(BadMagic)` from `CaptureFollower::open`, mirroring `CaptureReader`. |
| `follower_rejects_unsupported_version` | A file written with magic but version 2 returns `Err(UnsupportedVersion(2))`. |
| `follower_with_poll_interval_respects_setting` | A follower with `poll_interval = 5 ms` retries faster than the default; an indirect test that observes the latency between writer-append and follower-emit and asserts it is below a threshold. |

### `tests/cli_args.rs` extensions

| Test | Asserts |
|---|---|
| `parses_follow_subcommand` | `parse(["ranchero", "follow", "/tmp/x.cap"])` produces a `Command::Follow { path, decode: false, idle_timeout: None }`. |
| `parses_follow_with_decode` | `parse(["ranchero", "follow", "/tmp/x.cap", "--decode"])` sets `decode` to `true`. |
| `parses_follow_with_idle_timeout` | `parse(["ranchero", "follow", "/tmp/x.cap", "--idle-timeout", "30"])` sets `idle_timeout` to `Some(30)`. |
| `dispatch_follow_stub` | The stub `run()` output for a `Follow` command contains `"follow"`, mirroring the existing `dispatch_replay_stub` shape. |

## Implementation outline

1. Add `CaptureFollower` to `capture.rs` (or, if the file is
   already at the threshold, split `capture.rs` into a
   `capture/` subdirectory and place the follower in its own
   file). Reuse the existing `read_partial`,
   `CaptureRecord` parser logic by extracting it into a
   private helper that both `CaptureReader::next` and
   `CaptureFollower::next` call.
2. Implement the polling-on-EOF and polling-on-truncation
   logic. The simplest correct shape is to track the file
   offset before each read attempt; on a partial read, seek
   back to the offset before retrying. Alternatively, use a
   small in-memory buffer for the partial bytes and resume
   from where the partial read left off. Either approach
   passes the test set above; the implementer chooses based
   on whichever is clearer.
3. Implement the idle-timeout exit condition: track an
   instant of the last-observed record; if the current time
   exceeds `last_seen + idle_timeout` while the follower is
   in a polling loop, return `None`.
4. Add the `Follow` variant to `Command` in `src/cli.rs`,
   mirroring the existing `Replay` shape.
5. Add a `print_follow` function in `src/cli.rs` that opens
   the follower, applies the supplied options, iterates, and
   prints each record according to whether `--decode` is set.
6. Wire the dispatch arm.
7. Install a signal handler for `Ctrl-C` in the dispatch arm
   so that an interrupt drops the follower cleanly. The
   existing tokio runtime in the rest of the CLI is not
   required; the dispatcher can use `ctrlc::set_handler` (a
   small dependency) or `signal-hook` for synchronous
   handling.

## Manual validation procedure

This sub-step is straightforward enough that automated tests
cover most of its surface. A short manual check confirms the
end-to-end terminal experience:

1. In one terminal, run a writer-only smoke. Either:
   - Wait until STEP-12.1 is implemented and run
     `ranchero start --foreground --capture /tmp/x.cap` against
     production Zwift, or
   - Run the existing `writer_then_reader_round_trip_many_records`
     test in a loop (or write a minimal binary that opens a
     `CaptureWriter`, pushes a record per second, and never
     calls `flush_and_close`).
2. In a second terminal, run `ranchero follow /tmp/x.cap`. The
   summary line for each record appears as the writer produces
   it, with latency on the order of the polling interval.
3. Press `Ctrl-C` in the second terminal. `ranchero follow`
   exits with status zero.
4. Repeat with `ranchero follow --decode /tmp/x.cap`. Each
   record appears as a multi-line `Debug` block of the decoded
   message.
5. Repeat with `ranchero follow --idle-timeout 5 /tmp/x.cap`
   on a file where the writer has paused. The follower exits
   on its own after roughly five seconds.

## Acceptance criteria

- `cargo test --workspace` passes with the new follower and
  CLI tests in place.
- `cargo clippy --workspace --all-targets -- -D warnings`
  reports no warnings.
- `ranchero follow <path>` opens an existing capture file,
  prints existing records to standard output, and continues
  to print new records as they are appended.
- `ranchero follow --decode <path>` produces a `Debug`
  representation of each decoded `ServerToClient` (inbound)
  or `ClientToServer` (outbound).
- `ranchero follow --idle-timeout <secs> <path>` exits with
  status zero after the configured window without a new
  record.
- `Ctrl-C` exits the follower with status zero.
- The format-rejection paths
  (`BadMagic`, `UnsupportedVersion`) match `CaptureReader`'s
  behaviour exactly.

## Deferred

- File-system event notification (`inotify`, `kqueue`) for
  sub-polling-interval latency.
- Capture-file rotation support.
- A JSON output mode and selective field formatting for
  `--decode`.
- Filter flags (by direction, transport, message type).
- A "from offset" or "from timestamp" mode that begins
  reading partway through a long file rather than from the
  first record.

## Cross-references

- `docs/plans/STEP-12-game-monitor.md` — the parent plan,
  which lists the sub-steps that compose STEP-12.
- `docs/plans/STEP-12.1-tcp-end-to-end-smoke.md` — the
  preceding sub-step, which produces the live capture files
  that this command tails.
- `docs/plans/done/STEP-11.5-wire-capture.md` — the format
  specification and the `CaptureWriter` and `CaptureReader`
  this command extends.
