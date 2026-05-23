# Step 12.8 — Startup validation

**Status:** draft (2026-04-29).

## Background

`ranchero start` currently attempts to daemonize first and validates
runtime prerequisites after the fork. When a prerequisite fails —
missing credentials being the clearest case — the error occurs inside
the grandchild process whose stdin/stdout/stderr have already been
redirected to `/dev/null`. The operator sees nothing on the terminal.
In foreground mode the error does reach the terminal through
`main()`'s `eprintln!`, but it arrives after `ranchero stopped` is
printed on stdout, which implies a clean lifecycle rather than a
startup failure.

The root cause is that no explicit validation pass runs before
daemonization. The daemon tries to start, discovers mid-flight that it
cannot, and by then the operator's connection to the process output
is severed.

This document specifies a `validate_startup` function that collects
every fatal precondition failure before the process forks, prints all
problems to stderr while the terminal is still connected, and returns
a non-zero exit code without forking.

## Scope

Validation covers every condition that would prevent a successful start
and that can be checked synchronously, cheaply, and without touching
the network. Conditions that require I/O against a remote endpoint
(auth server reachability, Zwift API health) are explicitly out of
scope: they belong to a later retry-and-backoff layer, not a
startup gate.

Four checks are required:

| ID | Name | Condition gated on |
|---|---|---|
| S-1 | Relay credential presence | `relay_enabled = true` |
| S-2 | Pidfile directory writability | always |
| S-3 | Log file directory writability | always |
| S-4 | Capture path directory writability | `capture_path` is `Some` |

All four checks run unconditionally within their gate; if more than
one fails, all failures are reported together so the operator can fix
everything in one pass.

## Design

### Module and function

Add `src/daemon/validate.rs` as a new submodule of `src/daemon/`.
Declare it in `src/daemon/mod.rs` as `pub mod validate;`.

The public surface is a single function:

```rust
pub fn validate_startup(
    cfg: &ResolvedConfig,
    capture_path: Option<&std::path::Path>,
) -> Result<(), StartupValidationErrors>
```

`StartupValidationErrors` is a wrapper around `Vec<StartupValidationError>`
with a `Display` implementation that lists each failure on its own
indented line, suitable for `eprintln!` from `main()`.

### Error variants

```rust
pub enum StartupValidationError {
    MissingEmail,
    MissingPassword,
    DirectoryNotWritable { label: &'static str, path: PathBuf, reason: String },
}
```

`label` is a human-readable name for context
(`"pidfile"`, `"log file"`, `"capture file"`).
`reason` is the `io::Error` message from the write probe.

`Display` for `StartupValidationError`:

```
missing main account email; set one via `ranchero configure`
missing main account password; set one via `ranchero configure`
pidfile directory is not writable (/home/user/.local/state/ranchero): Permission denied (os error 13)
```

`Display` for `StartupValidationErrors`:

```
startup validation failed:
  - missing main account email; set one via `ranchero configure`
  - pidfile directory is not writable (/home/user/.local/state/ranchero): Permission denied
```

### Writability probe

For each directory check, resolve the parent directory of the target
path and attempt to create and immediately remove a probe file:

```rust
fn probe_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".ranchero-probe-{}", std::process::id()));
    std::fs::write(&probe, b"")
        .and_then(|_| std::fs::remove_file(&probe))
        .map_err(|e| e.to_string())
}
```

If the parent directory does not exist, the probe will fail with
`No such file or directory`; no separate existence check is needed.
Using a probe rather than inspecting permission bits handles ACLs,
read-only mounts, and quota exhaustion correctly.

### Call site

`validate_startup` is called in `daemon::start`
(`src/daemon/runtime.rs`) immediately after the existing `preflight`
call and before `daemonize_self`:

```rust
pub fn start(
    cfg: &ResolvedConfig,
    foreground: bool,
    log_opts: LogOpts,
    capture_path: Option<std::path::PathBuf>,
) -> Result<ExitCode, DaemonError> {
    let paths = DaemonPaths::from_config(cfg);
    preflight(&paths, &OsProcessProbe)?;
    validate::validate_startup(cfg, capture_path.as_deref())
        .map_err(|e| DaemonError::StartupValidation(e))?;

    if !foreground {
        // ... daemonize_self() ...
    }
    // ...
}
```

`DaemonError` gains a new variant:

```rust
DaemonError::StartupValidation(StartupValidationErrors)
```

with a `Display` that forwards to `StartupValidationErrors::fmt`, so
`main()`'s `eprintln!("error: {e}")` prints the full list to stderr
before the process exits.

Because `validate_startup` runs before `daemonize_self`, the error
message always reaches the terminal regardless of foreground/background
mode.

## Test surface

All unit tests live in `src/daemon/validate.rs` under `#[cfg(test)]`.
The subprocess tests extend `tests/daemon_lifecycle.rs`.

### S-1: Relay credential presence

**S-1a** `validate_relay_enabled_no_email_returns_missing_email`
- Build a `ResolvedConfig` with `relay_enabled = true`, `main_email =
  None`, `main_password = Some(...)`.
- Call `validate_startup`.
- Assert `Err` contains exactly `StartupValidationError::MissingEmail`.

**S-1b** `validate_relay_enabled_no_password_returns_missing_password`
- `main_email = Some(...)`, `main_password = None`.
- Assert `Err` contains `MissingPassword`.

**S-1c** `validate_relay_enabled_both_missing_returns_both_errors`
- Both absent.
- Assert `Err` contains both `MissingEmail` and `MissingPassword`, in
  that order.

**S-1d** `validate_relay_disabled_skips_credential_check`
- `relay_enabled = false`, `main_email = None`, `main_password = None`.
- Assert `Ok(())`.

**S-1e** `validate_relay_enabled_both_present_is_ok`
- Both set.
- Assert `Ok(())` (assuming writable directories — tests use a tempdir).

### S-2: Pidfile directory writability

**S-2a** `validate_pidfile_dir_missing_returns_error`
- Set `cfg.pidfile` to a path whose parent directory does not exist.
- Assert `Err` contains `DirectoryNotWritable { label: "pidfile", .. }`.

**S-2b** `validate_pidfile_dir_writable_is_ok`
- Set `cfg.pidfile` to a path inside a tempdir.
- Assert `Ok(())`.

### S-3: Log file directory writability

**S-3a** `validate_log_dir_missing_returns_error`
- Set `cfg.log_file` to a path whose parent does not exist.
- Assert `Err` contains `DirectoryNotWritable { label: "log file", .. }`.

**S-3b** `validate_log_dir_writable_is_ok`
- Set `cfg.log_file` inside a tempdir.
- Assert `Ok(())`.

### S-4: Capture path directory writability

**S-4a** `validate_capture_dir_missing_returns_error`
- Pass a `capture_path` whose parent does not exist.
- Assert `Err` contains `DirectoryNotWritable { label: "capture file", .. }`.

**S-4b** `validate_capture_none_skips_check`
- Pass `capture_path = None`.
- Assert no `DirectoryNotWritable` error for capture.

**S-4c** `validate_capture_dir_writable_is_ok`
- Pass a capture path inside a tempdir.
- Assert `Ok(())`.

### Subprocess tests (extend `tests/daemon_lifecycle.rs`)

**Sub-1** `start_exits_nonzero_and_prints_error_when_email_missing`
- Config: `relay_enabled = true` (no `[relay]` section), no email.
  `auth_base = "http://127.0.0.1:1"`.
- Invoke `ranchero --foreground start`.
- Assert: process exits non-zero.
- Assert: stderr contains `"missing main account email"`.
- Assert: process exits within 2 seconds (no network I/O attempted).

**Sub-2** `start_exits_nonzero_and_prints_error_when_password_missing`
- Config: `relay_enabled = true`, `main_email` set (via config or
  `--mainuser`), no password, `auth_base = "http://127.0.0.1:1"`.
- Same assertions with `"missing main account password"` in stderr.

**Sub-3** `start_does_not_write_pidfile_when_validation_fails`
- Config as in Sub-1.
- Assert: pidfile does not exist after process exits.

**Sub-4** `start_does_not_write_socket_when_validation_fails`
- Config as in Sub-1.
- Assert: socket does not exist after process exits.

**Sub-5** `start_exits_nonzero_when_pidfile_directory_missing`
- Config: `relay_enabled = false` (so credential check passes),
  `pidfile` inside a directory that does not exist.
- Assert: non-zero exit, stderr contains `"pidfile directory"` and
  `"not writable"`.

**Sub-6** `start_exits_nonzero_when_log_directory_missing`
- Config: `relay_enabled = false`, valid pidfile dir,
  `log_file` inside a directory that does not exist.
- Assert: non-zero exit, stderr contains `"log file directory"`.

Subprocess tests assert that the process exits within 2 seconds:
validation must not attempt any network I/O.

## Implementation steps

1. **Add `src/daemon/validate.rs`.**
   - Define `StartupValidationError` (enum) and
     `StartupValidationErrors` (newtype wrapper over `Vec`) with
     `Display` impls as described above.
   - Implement `probe_writable(dir: &Path) -> Result<(), String>`.
   - Implement `validate_startup(cfg, capture_path)` with the four
     checks in order: S-1 (credential presence when relay enabled),
     S-2 (pidfile dir), S-3 (log dir), S-4 (capture dir).
   - Add `#[cfg(test)] mod tests` covering all unit tests listed above.

2. **Declare the module in `src/daemon/mod.rs`.**
   - Add `pub mod validate;`.
   - Re-export `StartupValidationErrors` if callers outside the module
     need to name the type (only `DaemonError::StartupValidation`
     wraps it, so re-export is optional).

3. **Add `DaemonError::StartupValidation(StartupValidationErrors)`
   in `src/daemon/mod.rs`.**
   - Add a `Display` arm: forward to `StartupValidationErrors::fmt`
     directly (no additional prefix; the struct's own display already
     reads `"startup validation failed: ..."`)

4. **Call `validate_startup` in `daemon::start`
   (`src/daemon/runtime.rs`).**
   - Insert the call between `preflight` and `daemonize_self`.
   - Map the error to `DaemonError::StartupValidation`.

5. **Remove the relay-start credential check duplication (optional
   cleanup).**
   The `MissingEmail` / `MissingPassword` checks in `start_inner`
   (`src/daemon/relay.rs:677-685`) become unreachable when relay is
   enabled, because `validate_startup` fires first. Leave them in
   place as a defence-in-depth guard; do not remove them.

## Green-state verification

- All unit tests pass.
- All six subprocess tests pass and complete within 2 seconds each.
- Existing `start_exits_nonzero_when_relay_start_fails` (from
  STEP-12.6) continues to pass: its config omits credentials and sets
  `auth_base = "http://127.0.0.1:1"`. After this change the failure
  is caught at validation rather than at `RelayRuntime::start`; the
  observable behaviour (non-zero exit, no pidfile, no socket) is
  identical, so the test continues to pass without modification.
- `cargo test` reports 0 failures across all suites.
