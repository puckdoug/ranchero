# Step 12.9 — Confirm capture path before backgrounding; configurable watched athlete

**Status:** draft (2026-05-01).

## Background

After STEP-12.5 and STEP-12.6 the operator workflow

```
ranchero start --capture session.cap
sleep 10
ranchero follow session.cap
```

is structurally possible: the daemon runs the relay, opens the capture
writer, and accepts inbound traffic. Two gaps remain that prevent this
exact command sequence from working as written.

**Gap A — relative capture path crosses `chdir("/")` after fork.**
`ranchero start` defaults to backgrounded. `daemonize_self` calls
`chdir("/")` after the second fork (`src/daemon/runtime.rs:194`). The
capture path is forwarded as the `PathBuf` parsed by clap; if the
operator passes a relative path such as `session.cap`, the daemon
attempts to open `/session.cap` (almost certainly unwritable), while
`ranchero follow session.cap` resolves the same string against the
operator's shell CWD. The two processes reference different files.

There is also a second-order issue in pre-fork validation: S-4 in
STEP-12.8 probes the parent directory's writability from the operator's
CWD (because validation runs before `daemonize_self`). That probe can
succeed while the post-fork open fails, because the parent directory
the daemon sees after `chdir("/")` is different.

**Gap B — `watched_athlete_id` is not configurable.** STEP-12.6 noted
this as a deferred proposal. With monitor credentials alone, the relay
authenticates and connects, but `WatchedAthleteState::default()` is
constructed with no target rider (`src/daemon/relay.rs:1113, 1267`).
The captured stream may therefore lack records for any specific rider.
The athlete ID is public information (visible in the Zwift companion
app and in profile URLs), so storing it in `ranchero.toml` is safe and
removes the need for any main-account login.

## Scope

Two independent items, both addressed in this step:

| ID | Item |
|---|---|
| 1 | Capture path: resolve to absolute, create file pre-fork, retain handle across fork |
| 2 | `watched_athlete_id` configuration field (TOML + `ranchero configure` TUI) |

---

## Item 1 — Resolve, create, and retain the capture file before fork

### Required behaviour

1. **Full path is known before fork.** The capture path passed to
   `daemon::start` must be absolute. If the operator supplies a
   relative path on the command line, `cli::dispatch` normalises it
   against the operator's CWD before forwarding. After this normalisation
   no further path manipulation is required by the daemon.

2. **The file is created before fork.** The capture writer is opened
   (which writes the `RNCWCAP` magic header) inside `daemon::start`,
   between `validate_startup` and `daemonize_self`. If the open fails,
   the failure reaches stderr while the terminal is still connected and
   the process exits non-zero without forking.

3. **The file handle is retained across the fork.** On Unix, file
   descriptors survive `fork(2)` automatically, so the open
   `CaptureWriter` (and the underlying file) continues to be valid in
   the daemon grandchild. The existing `RelayRuntime::start_with_*_and_writer`
   variants already accept a pre-opened `Arc<CaptureWriter>`; the new
   wiring threads the pre-fork writer through `run_daemon` to those
   entry points instead of opening a fresh writer post-fork.

### Design

#### Path resolution

In `src/cli.rs`, the `Command::Start` arm normalises
`cli.global.capture` before forwarding:

```rust
let capture = cli.global.capture
    .as_ref()
    .map(|p| std::path::absolute(p))
    .transpose()?;
Ok(daemon::start(&resolved, cli.global.foreground, log_opts, capture)?)
```

`std::path::absolute` joins the path with the current CWD if relative
and lexically normalises it; it does not require the file or its
parent to exist. The `?` propagates an I/O error (e.g. CWD lookup
failure) as `DaemonError::Io`.

#### Pre-fork file creation

Extend `validate_startup` in `src/daemon/validate.rs` so that the S-4
check, when given a capture path, opens the file rather than only
probing the parent directory. The signature changes to return the
opened writer on success:

```rust
pub fn validate_startup(
    cfg: &ResolvedConfig,
    capture_path: Option<&Path>,
) -> Result<StartupArtifacts, StartupValidationErrors>;

pub struct StartupArtifacts {
    pub capture_writer: Option<Arc<zwift_relay::capture::CaptureWriter>>,
}
```

When `capture_path` is `Some`:
1. Resolve the parent directory and run the existing writability
   probe (catches missing directory and permissions issues with a
   focused error message before any partial file is created).
2. Call `zwift_relay::capture::CaptureWriter::open(path)` to create
   the file and write the format header.
3. Wrap the writer in `Arc` and return it through `StartupArtifacts`.

If the open fails, push a `DirectoryNotWritable { label: "capture
file", .. }` error (or a new `CaptureOpenFailed` variant — see
implementation steps) and return `Err`.

When `capture_path` is `None`, return
`StartupArtifacts { capture_writer: None }`.

#### Threading the writer through the daemon

`daemon::start` (`src/daemon/mod.rs`) and `runtime::start`
(`src/daemon/runtime.rs`) are extended:

```rust
pub fn start(
    cfg: &ResolvedConfig,
    foreground: bool,
    log_opts: LogOpts,
    capture_path: Option<PathBuf>,
) -> Result<ExitCode, DaemonError>;
```

Internally:

1. `runtime::start` calls `validate_startup(cfg, capture_path.as_deref())`
   and binds the returned `StartupArtifacts`.
2. After daemonize, `run_daemon` is invoked with the artifacts:
   ```rust
   run_daemon(paths, cfg, capture_path, artifacts.capture_writer).await
   ```
3. `run_daemon` calls a new wrapper around the existing
   `RelayRuntime::start_with_deps_and_writer` — call it
   `RelayRuntime::start_with_writer` — that takes
   `Option<Arc<CaptureWriter>>` and constructs the production DI
   types. When the writer is `None`, the existing `RelayRuntime::start`
   path is used; when `Some`, the writer is passed through.

The existing `RelayRuntime::start` continues to support the in-process
test path that opens its own writer from a path (used by
`tests/full_scope.rs`); only the daemon's production path uses the
pre-opened writer.

### Test surface

Unit tests in `src/daemon/validate.rs`:

- **S-4d** `validate_capture_path_returns_open_writer_with_header`
  — pass a writable capture path; assert `Ok` returns
  `Some(writer)` and the file on disk starts with `RNCWCAP`.
- **S-4e** `validate_capture_path_no_partial_file_on_writability_failure`
  — parent directory missing; assert `Err`, and assert the file
  was not created.

Subprocess tests in `tests/daemon_lifecycle.rs` (or a new file):

- **Sub-7** `start_canonicalizes_relative_capture_path` — invoke
  `ranchero start --capture session.cap` with a CWD inside a tempdir;
  assert the file appears at `<tempdir>/session.cap` (not `/session.cap`)
  and grows past the header bytes during a short sleep.
- **Sub-8** `start_exits_nonzero_when_capture_parent_unwritable` —
  pass a capture path under a read-only directory; assert non-zero
  exit, stderr contains `"capture file"` and `"not writable"`, no
  fork has occurred (no pidfile).
- **Sub-9** `capture_file_handle_survives_fork` — backgrounded start,
  poll the capture file under the operator's CWD; assert the file
  size grows after the foreground process has returned.

Library test in `tests/full_scope.rs`:

- Adjust `relay_runtime_start_with_capture_path_creates_capture_file`
  if necessary, since the production daemon path now opens pre-fork
  while the library test still opens inside `RelayRuntime::start`.

### Implementation steps

1. **Add `std::path::absolute` call in `src/cli.rs`** for
   `Command::Start`. Map any I/O error into `DaemonError::Io`.

2. **Refactor `validate_startup`** to return
   `Result<StartupArtifacts, StartupValidationErrors>`. Existing
   callers (foreground tests, the daemon entry point) bind the
   returned artifacts. Update all unit tests in `src/daemon/validate.rs`
   to match the new signature; existing assertions that only check
   the `Err` shape remain valid.

3. **Open the capture writer inside S-4** when a path is supplied.
   Keep the writability probe as a fast-fail before the open so that
   parent-directory issues produce a focused error message.

4. **Add `StartupArtifacts` to the `Ok` value** carrying
   `Option<Arc<CaptureWriter>>`.

5. **Thread the writer through `daemon::start` → `runtime::start` →
   `run_daemon`.** Add a new `RelayRuntime` constructor (e.g.
   `start_with_writer`) that takes `Option<Arc<CaptureWriter>>` and
   delegates to `start_with_deps_and_writer` with the production DI
   types.

6. **Wire the new constructor inside `run_daemon`.** When the writer
   is `Some`, the path argument to `RelayRuntime` becomes ignored
   (the writer carries the open file). When `None`, `RelayRuntime`
   continues to construct itself with no capture.

7. **Add the three subprocess tests (Sub-7, Sub-8, Sub-9).** These
   exercise the relative-path canonicalisation, the pre-fork open
   failure path, and the post-fork handle retention.

### Green-state verification

- The command sequence at the top of this document, run with monitor
  credentials configured and an absolute or relative capture path,
  produces a populated capture file readable by `ranchero follow`.
- All new unit and subprocess tests pass.
- `tests/daemon_lifecycle.rs` baseline (17 tests) continues to pass.

---

## Item 2 — `watched_athlete_id` configuration field

### Required behaviour

The operator can record a single watched-athlete ID in `ranchero.toml`,
either by hand-editing the file or through `ranchero configure`. When
present, the relay initialises `WatchedAthleteState` to track that
rider as soon as the daemon starts. When absent, the runtime continues
to construct `WatchedAthleteState::default()` and the operator may
later supply an ID through the existing
`RelayRuntime::switch_watched_athlete` API.

### Design

#### Schema

In `src/config/mod.rs`, extend `ZwiftConfig`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ZwiftConfig {
    pub auth_base: String,
    pub api_base:  String,
    pub watched_athlete_id: Option<u64>,
}
```

`Default::default()` for the field is `None`. Existing configuration
files without the field continue to load.

The TOML representation:

```toml
[zwift]
watched_athlete_id = 123456
```

#### Resolution

`ResolvedConfig` gains `watched_athlete_id: Option<u64>` populated
directly from `file.zwift.watched_athlete_id`. No environment override
or CLI flag at this stage; this is operator configuration, not a
per-invocation knob.

#### Wiring into the relay

In `src/daemon/relay.rs`, both `RelayRuntime` constructors that build
`watched_state` change from

```rust
watched_state: std::sync::Mutex::new(WatchedAthleteState::default()),
```

to

```rust
watched_state: std::sync::Mutex::new(
    cfg.watched_athlete_id
        .map(|id| WatchedAthleteState::for_athlete(id as i64))
        .unwrap_or_default(),
),
```

The cast is `as i64` because `WatchedAthleteState::for_athlete` and the
proto field both use `i64`; storing the field as `u64` in the schema
matches the unsigned semantics of an athlete ID without giving up
range.

#### `ranchero configure` TUI

In `src/tui/model.rs`, add a new `FieldId::WatchedAthleteId` and a
matching `EditorState`. Place the field on the existing Zwift /
endpoints screen, below `auth_base` / `api_base`. Validation is
"empty or a non-zero unsigned integer" — empty maps to `None`.

In `src/tui/view.rs`, render the field with the same layout as the
endpoint inputs.

In `src/tui/driver.rs`, the round-trip writes
`zwift.watched_athlete_id` to the loaded `ConfigFile` before
serialising back to disk.

#### `auth-check` output

In `src/cli.rs`'s `print_auth_check_to`, add a line under the existing
endpoint summary:

```
watched athlete: 123456
```

or `watched athlete: (unset)` when the field is `None`.

### Test surface

Schema tests in `src/config/mod.rs`:

- **C-1** `parse_watched_athlete_id_from_zwift_section` — TOML
  containing `[zwift] watched_athlete_id = 123456` parses with
  `cfg.zwift.watched_athlete_id == Some(123456)`.
- **C-2** `default_watched_athlete_id_is_none` — TOML with no
  `[zwift]` section yields `None`.
- **C-3** `resolve_carries_watched_athlete_id_through` — `ResolvedConfig::resolve`
  populates the field from the loaded `ConfigFile`.

Wiring tests in `src/daemon/relay.rs`:

- **W-1** `relay_runtime_initialises_watched_state_from_config` —
  `cfg.watched_athlete_id = Some(99_999)`; assert `WatchedAthleteState`
  is initialised with athlete ID `99_999` (probe via existing
  `watched_state` accessor or via a new test-only getter).
- **W-2** `relay_runtime_default_watched_state_when_none` —
  `cfg.watched_athlete_id = None`; assert state matches
  `WatchedAthleteState::default()`.

TUI tests in `src/tui/model.rs` and `src/tui/view.rs`:

- **TUI-1** `configure_screen_renders_watched_athlete_id_field` —
  render the configure model and assert the new field appears with
  its current value.
- **TUI-2** `configure_round_trip_persists_watched_athlete_id` —
  enter a value, run `to_config_file`, assert the resulting
  `ConfigFile` has the field populated.
- **TUI-3** `configure_round_trip_clears_watched_athlete_id` — enter
  empty, assert the field becomes `None`.

`auth-check` test in `tests/auth_check.rs`:

- **AC-1** `auth_check_reports_watched_athlete_id_when_set` — config
  with the field set; assert the rendered output contains the ID.
- **AC-2** `auth_check_reports_unset_when_field_absent` — output
  contains `(unset)`.

### Implementation steps

1. **Extend `ZwiftConfig`** with `watched_athlete_id: Option<u64>`.
   Add the schema test (C-1, C-2).

2. **Extend `ResolvedConfig`** with `watched_athlete_id: Option<u64>`
   and populate it inside `resolve`. Add the resolution test (C-3).

3. **Wire the field into `RelayRuntime`** at both construction sites
   (`src/daemon/relay.rs:1113` and `:1267`). Add wiring tests (W-1,
   W-2).

4. **Surface in `auth-check`** by extending `print_auth_check_to` and
   updating `tests/auth_check.rs`.

5. **Add to the configure TUI** (`FieldId`, `EditorState`, view
   rendering, driver round-trip). Add TUI tests (TUI-1, TUI-2,
   TUI-3).

6. **Update all `ResolvedConfig` test fixtures** (`src/daemon/relay.rs`,
   `src/daemon/validate.rs`, `tests/relay_runtime.rs`,
   `tests/full_scope.rs`, `tests/auth_check.rs`) to set
   `watched_athlete_id: None` so they continue to compile.

### Green-state verification

- All schema, wiring, TUI, and auth-check tests pass.
- A `ranchero.toml` containing `[zwift] watched_athlete_id = N`
  produces a relay whose `WatchedAthleteState` is initialised to
  athlete `N` at startup.
- `cargo test` reports zero failures across all suites.

---

## Cross-references

- `docs/plans/done/STEP-12.5-still-not-doing-the-job-as-specified.md` —
  the parent operator-path plan; this step closes the last two gaps
  noted in its review.
- `docs/plans/done/STEP-12.6-really-basic-implementation-details-that-were-screwed-up-anyway.md` —
  Item 2 here implements the `watched_athlete_id` config field
  proposal that STEP-12.6 deferred (lines 2245–2287 of that plan).
- `docs/plans/done/STEP-12.8-startup-validation.md` — Item 1 here
  extends S-4 to perform a real open rather than a probe-only check.
