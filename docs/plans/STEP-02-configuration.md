# Step 02 — Configuration file & `ranchero configure` TUI

## Goal

Give ranchero a durable configuration surface so subsequent steps can read
settings without re-asking on every run:

1. A **config file** on disk (TOML) with a documented schema, loader,
   default-path resolver, and a precedence merge with CLI/env overrides.
2. An interactive **TUI** launched by `ranchero configure` that reads the
   current file, lets the user edit each field, validates, and writes back.

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

Tested as a pure `Config::resolve(cli_global: &GlobalOpts, env: &HashMap,
file: Option<ConfigFile>) -> ResolvedConfig` function.

## Tests first

Config loading / merging (unit tests in `src/config.rs`):

1. `default_config_when_no_file_and_no_overrides` — empty CLI + empty env
   yields the documented defaults exactly.
2. `config_file_overrides_defaults` — a fixture TOML sets
   `server.port = 9999`; resolved config reflects it.
3. `env_overrides_file` — same fixture + `RANCHERO_SERVER_PORT=1234` →
   port 1234.
4. `cli_overrides_env_and_file` — also pass `--port 5555` (once we add a
   generic override mechanism, or specifically for credentials in STEP
   02) → 5555.
5. `cli_mainuser_overrides_file_main_email` — `--mainuser x@y` wins over
   `accounts.main.email` in the file.
6. `cli_mainpassword_not_written_anywhere` — after `resolve`, the
   resolved struct **does not** expose `mainpassword`; it is handed to
   the secrets API (STEP 05 mock) instead. (Explicit assertion: field
   does not exist or is typed `RedactedString`.)
7. `unknown_schema_version_errors` — a file with `schema_version = 99`
   returns `Err(ConfigError::UnknownSchemaVersion)`.
8. `malformed_toml_errors_with_path_and_line` — garbage TOML yields an
   error referencing the offending path and line.
9. `tilde_expansion_for_paths` — `~/foo` → `$HOME/foo` for `logging.file`
   and `daemon.pidfile`.
10. `config_path_flag_respected` — `--config /tmp/alt.toml` loads that
    file instead of the default location.
11. `config_missing_at_explicit_path_errors` — `--config /does/not/exist`
    errors; contrast with the default-location case which falls back to
    defaults silently.

TUI (behavioural tests; drive through an in-memory terminal backend, not
a real PTY):

12. `tui_renders_initial_fields_from_current_config` — fires up the
    configure screen against a fixture and asserts the on-screen values
    match.
13. `tui_edits_main_email_and_writes_atomically` — simulate keystrokes
    to change main email, press save; assert the on-disk file is
    rewritten atomically (temp file + rename) with just that field
    changed.
14. `tui_save_stores_password_in_mock_keyring` — a keyring trait with a
    test implementation captures the password write; assert the file
    does *not* contain the password.
15. `tui_validates_email_format` — empty or malformed email → the save
    action is refused and the field is highlighted; no file write
    occurs.
16. `tui_cancel_discards_changes` — edit, then cancel; original file
    unchanged; no keyring write.
17. `tui_works_against_missing_file` — no config file exists → TUI starts
    with defaults and can create one on save.

## Implementation outline

- Crates: add `serde`, `serde_derive`, `toml`, `directories` (for XDG
  paths), `shellexpand` (or a small local helper for `~`), and a TUI
  library — candidate `ratatui` + `crossterm`, but the TUI surface is
  small so `inquire` (prompt-based) is the lower-effort first pass.
  **Decision for this step:** start with `inquire` for simplicity; we
  can upgrade to `ratatui` later if we need a richer widget-based layout.
- New module `src/config.rs` with:
  - `ConfigFile` (TOML-deserializable, exact schema above).
  - `ResolvedConfig` (the merged runtime view — `NonZeroU16` for port,
    `PathBuf` for paths, etc.).
  - `Config::load(path: Option<&Path>) -> Result<ConfigFile, …>`.
  - `ResolvedConfig::resolve(cli: &GlobalOpts, env: &Env, file:
    Option<ConfigFile>) -> Self`.
  - `RedactedString(String)` with a `Debug`/`Display` that prints
    `"[redacted]"`.
- New module `src/tui.rs` providing `run_configure(&mut dyn
  ConfigStore, &mut dyn KeyringStore) -> Result<()>`. The two traits are
  injected so tests can swap in fakes.
- `src/cli.rs` grows a `Command::Configure` dispatch that calls
  `tui::run_configure` against real `FileConfigStore` and `OsKeyring`
  implementations (the latter stubbed until STEP 05).

## Acceptance criteria

- All tests above pass.
- `ranchero configure` against a clean home dir walks the user through
  the schema, writes a valid TOML, and stores passwords in the mock
  keyring (real keyring in STEP 05).
- `ranchero start --mainuser x@y` (which still stubs in STEP 02)
  demonstrates the resolved config's main email is `x@y`, overriding any
  file value.

## Deferred

- Real keyring (STEP 05) — here we use a trait + in-memory fake.
- Migrations on `schema_version` bump — not needed until v2.
- Config validation beyond "it parses and emails look like emails" —
  port range, bind IP parse, file-path writability.
