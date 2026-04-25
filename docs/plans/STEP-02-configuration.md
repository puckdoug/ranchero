# Step 02 — Configuration file & `ranchero configure` TUI

## Goal

Give ranchero a durable configuration surface so subsequent steps can read
settings without re-asking on every run:

1. A **config file** on disk (TOML) with a documented schema, loader,
   default-path resolver, and a precedence merge with CLI/env overrides.
2. An interactive **TUI** launched by `ranchero configure` — a full
   widget-based ratatui UI that reads the current file, lets the user edit
   each field, validates, and writes back. Built on ratatui from day one
   because the configuration surface is expected to grow (multiple
   sections, per-field validation, lists, route/world overrides, mod
   toggles, etc.) and switching frameworks later would be more work than
   absorbing ratatui's modest learning curve up front.

Credentials are **not** written to this file — they live in the keyring
(STEP 05). The config file only stores a username (or keyring account
handle) per role, plus non-secret tuning knobs.

## Schema (v1)

```toml
# ~/.config/ranchero/ranchero.toml (XDG_CONFIG_HOME on linux/mac, %APPDATA% on win)
schema_version = 1

[accounts.main]
email = "rider@example.com"   # password lives in the keyring

[accounts.monitor]
email = "monitor@example.com"

[server]
bind   = "127.0.0.1"
port   = 1080
https  = false                # true looks for ./https/{key,cert}.pem like sauce4zwift

[logging]
level  = "info"               # trace | debug | info | warn | error
file   = "~/.local/state/ranchero/ranchero.log"

[daemon]
pidfile = "~/.local/state/ranchero/ranchero.pid"
```

Every field has a compile-time default; the config file can be absent.
Schema versioning is reserved for future migrations — an unknown
`schema_version` fails loudly rather than silently loading partial data.

## Precedence

`CLI flag > env var (RANCHERO_*) > config file > built-in default`.

Tested as a pure `ResolvedConfig::resolve(cli: &GlobalOpts, env: &Env,
file: Option<ConfigFile>) -> ResolvedConfig` function. `Env` is a small
trait wrapping `std::env::var_os` so tests don't touch the real
environment.

## TUI architecture

Strict separation between **state** and **rendering** so every behavioural
test runs against the pure model and never needs a real terminal:

```
┌──────────────────┐   Event    ┌──────────────────┐   render()   ┌──────────────┐
│  crossterm I/O   │ ─────────▶ │   Model::update  │ ───────────▶ │  ratatui Frame
│  (real or fake)  │            │   pure function  │              │  (or TestBackend)
└──────────────────┘ ◀───────── │     (no I/O)     │              └──────────────┘
        ▲             Action     └────────┬─────────┘
        │  Save → ConfigStore + KeyringStore (traits, faked in tests)
```

Key types:

```rust
// src/tui/model.rs
pub struct Model {
    pub current_screen: Screen,
    pub focus: FieldId,            // which field has focus
    pub fields: Fields,            // typed editable values, mirrors ConfigFile
    pub draft_password_main: Option<String>,    // never serialized
    pub draft_password_monitor: Option<String>, // never serialized
    pub validation: ValidationReport,           // recomputed every update()
    pub status: StatusBar,                      // hint text, errors, "saved"
    pub dirty: bool,
    pub mode: Mode,                // Normal | Editing | ConfirmDiscard | Help
}

pub enum Screen { Accounts, Server, Logging, Daemon, Review }
pub enum FieldId { MainEmail, MainPassword, MonitorEmail, MonitorPassword,
                   ServerBind, ServerPort, ServerHttps,
                   LogLevel, LogFile, PidFile }

pub enum Action {
    None,
    Save,                          // dispatched to caller, returns Result
    Cancel,                        // exits without writing
    DiscardConfirmed,              // exits even though dirty
    Quit,                          // hard exit (cancel without confirm)
}

impl Model {
    pub fn new(initial: ConfigFile) -> Self;
    pub fn update(&mut self, ev: Event) -> Action;  // pure: no I/O
    pub fn render(&self, frame: &mut Frame);        // pure: no I/O
}
```

`Event` is a thin wrapper around `crossterm::event::Event` so tests can
construct them without depending on terminal types directly.

The driver:

```rust
// src/tui/driver.rs
pub fn run_configure(
    backend: &mut dyn TerminalBackend,        // real ratatui::Terminal in prod
    config_store: &mut dyn ConfigStore,
    keyring_store: &mut dyn KeyringStore,
) -> Result<ExitCode, ConfigureError>;
```

`TerminalBackend` is a thin abstraction over `ratatui::Terminal`'s
`draw` + event loop so tests can drive `Model::update` directly with a
scripted `Event` stream and inspect the resulting `TestBackend` buffer
without touching `run_configure`.

## Tests first

### Config loading / merging (pure)

Unit tests in `src/config.rs`:

1. `default_config_when_no_file_and_no_overrides` — empty CLI + empty env
   yields the documented defaults exactly.
2. `config_file_overrides_defaults` — a fixture TOML sets
   `server.port = 9999`; resolved config reflects it.
3. `env_overrides_file` — same fixture + `RANCHERO_SERVER_PORT=1234` →
   port 1234.
4. `cli_mainuser_overrides_file_main_email` — `--mainuser x@y` wins over
   `accounts.main.email` in the file.
5. `cli_mainpassword_handled_via_redacted_string` — after `resolve`, the
   `mainpassword` is wrapped in `RedactedString`; `Debug`/`Display`
   render `"[redacted]"`; the actual value is reachable only via
   `.expose()`.
6. `unknown_schema_version_errors` — a file with `schema_version = 99`
   returns `Err(ConfigError::UnknownSchemaVersion)`.
7. `malformed_toml_errors_with_path_and_line` — garbage TOML yields an
   error referencing the offending path and line.
8. `tilde_expansion_for_paths` — `~/foo` → `$HOME/foo` for `logging.file`
   and `daemon.pidfile`.
9. `config_path_flag_respected` — `--config /tmp/alt.toml` loads that
   file instead of the default location.
10. `config_missing_at_explicit_path_errors` — `--config /does/not/exist`
    errors; contrast with the default-location case which falls back to
    defaults silently.
11. `port_zero_rejected_at_resolve` — `server.port = 0` → `Err`.
12. `bind_must_parse_as_ip_or_hostname` — invalid `server.bind` → `Err`.

### Atomic file writes (pure-ish)

13. `atomic_write_creates_tempfile_and_renames` — invokes the writer
    against a tmpdir, asserts a `*.tmp` file is created and that the
    final file matches expected bytes; partial-write fault injection
    leaves the original intact.

### TUI model — pure `update(event) -> Action` tests

These don't render; they drive `Model::update` directly. Place under
`src/tui/model.rs` `#[cfg(test)] mod tests`.

14. `model_initial_state_reflects_loaded_config` — building a model
    from a fixture `ConfigFile` populates each field's draft equal to
    the source value.
15. `tab_advances_focus_within_screen` — focus moves through fields in
    document order; Shift-Tab moves backward.
16. `right_arrow_moves_to_next_screen` — Right (or Ctrl-Right, decide)
    advances `Screen::Accounts → Server → Logging → Daemon → Review`,
    wrapping or stopping at edges per spec.
17. `editing_mode_captures_typed_text_into_focused_field`.
18. `enter_in_editing_mode_commits_value_and_returns_to_normal`.
19. `escape_in_editing_mode_reverts_field`.
20. `numeric_field_rejects_non_digit_input` — `server.port` ignores
    letters; `validation` records the rejection.
21. `email_field_validation_runs_on_every_update` — invalid → field
    flagged in `ValidationReport`; valid → cleared.
22. `password_field_does_not_appear_in_serialized_config_file` — even if
    the user types one in, the `ConfigFile` we'd serialize has no
    password field.
23. `save_action_returns_only_when_no_validation_errors` — invalid model
    + Save key returns `Action::None` and shows status error; valid
    model returns `Action::Save`.
24. `cancel_when_clean_returns_quit_immediately`.
25. `cancel_when_dirty_returns_none_and_enters_confirm_discard`.
26. `confirm_discard_then_quit_returns_discard_confirmed`.
27. `help_key_toggles_help_overlay`.

### TUI rendering — `TestBackend` snapshot/assertion tests

ratatui's `TestBackend` exposes the rendered buffer. Tests render the
model into a fixed-size buffer and assert specific cells / lines.

28. `accounts_screen_shows_main_and_monitor_emails`.
29. `server_screen_shows_port_and_bind`.
30. `password_field_renders_as_asterisks_not_plaintext`.
31. `validation_error_marker_appears_next_to_invalid_field`.
32. `dirty_indicator_appears_in_status_bar_after_edit`.
33. `help_overlay_lists_keybindings_when_toggled`.

### Driver — fake stores

34. `run_configure_writes_file_atomically_on_save` — drive the model
    through a Save flow, assert the `FakeConfigStore` recorded an
    atomic write whose bytes match expected TOML.
35. `run_configure_calls_keyring_for_passwords_only` — assert the
    `FakeKeyringStore` got two entries (main, monitor) with the typed
    passwords; the written TOML contains no password fields.
36. `run_configure_aborts_without_writes_on_cancel`.
37. `run_configure_handles_missing_file_by_starting_with_defaults`.

Tests 34-37 use a `ScriptedEvents` adapter that feeds a `Vec<Event>`
into the run loop in place of real keyboard input.

## Implementation outline

Crates added (workspace-style features only; we stay single-crate until
STEP 06):

| Need | Crate |
|---|---|
| TUI rendering & event loop | `ratatui` |
| Terminal backend & key events | `crossterm` |
| Config (de)serialization | `serde`, `serde_derive`, `toml` |
| Standard config dirs | `directories` |
| `~` expansion | small in-tree helper (no extra crate) |

Module layout:

```
src/
  cli.rs                # extended: Configure dispatches into tui::run_configure
  lib.rs                # adds `pub mod config; pub mod tui;`
  config/
    mod.rs              # ConfigFile, ResolvedConfig, errors, RedactedString
    paths.rs            # default-path resolution + tilde expansion
    atomic_write.rs     # tempfile + fsync + rename
    store.rs            # ConfigStore trait + FileConfigStore impl
  tui/
    mod.rs              # re-exports
    model.rs            # Model, Screen, FieldId, Action, Mode, update(), pure
    view.rs             # render(model, frame) — widget composition only
    driver.rs           # run_configure(backend, stores) — wires Model + Backend
    backend.rs          # TerminalBackend trait + RatatuiBackend + ScriptedBackend
    keyring.rs          # KeyringStore trait + InMemoryKeyringStore (real one in STEP 05)
```

The `Model::update` boundary is the contract every behavioural test sits
on. `view.rs` is intentionally allowed to be loose-typed (lots of
`Spans`, `Layout`) because all of its observable behaviour is captured
through `TestBackend` cell assertions.

`src/cli.rs::run` grows a real `Command::Configure` dispatcher only at
the end of the step (after the model is fully covered); until then,
configure still returns the stub from STEP 01 so unrelated tests stay
green.

## Acceptance criteria

- All tests in this step pass; `cargo clippy --all-targets -- -D warnings` clean.
- `ranchero configure` against a clean home dir launches the ratatui UI;
  the user can edit every field, validation feedback is live, and Save
  writes a valid TOML + populates the in-memory keyring.
- The serialized TOML byte-for-byte matches a golden file for a known
  set of edits.
- `ranchero start --mainuser x@y` (still stubbed in STEP 02) prints the
  resolved main email as `x@y`, demonstrating CLI-over-file precedence.
- `cargo run -- configure` exits 0 on a successful save, non-zero on
  hard quit without save (so callers / scripts can react).

## Deferred

- Real keyring backend → STEP 05 (here we use the trait + in-memory fake).
- Migrations on `schema_version` bump — not needed until v2.
- Mouse support / resize handling beyond what `ratatui` does for free.
- Settings categories beyond v1 schema (mods, route overrides, etc.) —
  the screen enum + field enum are designed so adding a new section is
  a localized change.
