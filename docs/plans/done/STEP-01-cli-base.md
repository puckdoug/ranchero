# Step 01 — Base CLI

## Goal

A `ranchero` binary that parses a command line, recognises all planned
subcommands and options, prints coherent `--help`, and dispatches to a
per-subcommand stub that simply reports which command was selected and which
options were supplied. No I/O beyond argument parsing and stdout; no network,
no config reading, no daemon fork. All logic is pure and therefore
exhaustively testable.

This is the scaffold that every later step plugs into.

## Subcommands

| Command | Purpose |
|---|---|
| `ranchero configure` | Open an interactive TUI to configure the application. (Stubbed in this step — actually launched in STEP 02.) |
| `ranchero start` | Start ranchero listening on a Zwift stream. (Stubbed; real behaviour in STEP 03.) |
| `ranchero stop` | Stop the currently-running ranchero process. (Stubbed; real behaviour in STEP 03.) |
| `ranchero status` | Dump statistics about the currently running ranchero process, or report shut down. (Stubbed; real behaviour in STEP 03.) |

## Global options

All options are parsed at the top level and made available to every
subcommand. (`clap`'s derive API with `#[command(flatten)]` on a shared
`GlobalOpts` struct is the straightforward way.)

| Long | Short | Argument | Description |
|---|---|---|---|
| `--verbose` | `-v` | — | Verbose mode. Repeatable later; boolean for now. |
| `--debug` | `-D` | — | Emit debug-level output. **Implies `--foreground`** (see below). |
| `--foreground` | — | — | Keep the process in the foreground (no daemonization). |
| `--mainuser` | — | `<email>` | Override main account login email. |
| `--mainpassword` | — | `<password>` | Override main account password. |
| `--monitoruser` | — | `<email>` | Override monitor account email. |
| `--monitorpassword` | — | `<password>` | Override monitor account password. |
| `--config` | — | `<path>` | Use an alternative configuration file. |

Notes:

- `-D` is a **non-standard** short flag for debug (clap's default would be
  `-d`). Since the user specified `-D` explicitly, wire it with
  `#[arg(short = 'D', long = "debug")]`.
- `--debug` setting `foreground = true` is resolved in `GlobalOpts::finalize()`
  so tests can assert it as a pure function rather than by side-effect.
- `--mainpassword` and `--monitorpassword` on the command line are an
  anti-pattern (they leak to `ps`) but the spec requires them; warn when
  they are set and `--verbose` is on.
- Option values are plain `Option<T>` in this step. The precedence merge
  (CLI > env > config file > default) belongs to STEP 02.

## Tests first

Put tests in `tests/cli_args.rs` (integration test against a public
`parse_args` function) plus unit tests inside `src/cli.rs` where
convenient. All tests must compile-and-fail before any production code
beyond the minimum to satisfy the compiler.

### Parsing tests

1. `parses_start_with_no_options` — `["ranchero", "start"]` yields
   `Command::Start` with every option `None`/`false`.
2. `parses_stop_and_status_and_configure` — one test per remaining
   subcommand, confirming the variant and default options.
3. `verbose_flag_long_and_short` — `-v` and `--verbose` each set
   `verbose = true`.
4. `debug_flag_uses_capital_d` — `-D` and `--debug` each set
   `debug = true`; assert `-d` (lowercase) is a parse error or is not
   accepted as debug.
5. `debug_implies_foreground` — after `GlobalOpts::finalize()`,
   `debug = true` causes `foreground = true` even if `--foreground` was
   not passed.
6. `explicit_foreground_without_debug` — `--foreground` alone sets
   `foreground = true` and leaves `debug = false`.
7. `main_credentials_capture_both_parts` —
   `--mainuser a@b --mainpassword x` populates both fields on `GlobalOpts`.
8. `monitor_credentials_capture_both_parts` — same for monitor fields.
9. `config_path_captured` — `--config /tmp/ranchero.toml` yields
   `global.config == Some(PathBuf::from("/tmp/ranchero.toml"))`.
10. `options_work_before_and_after_subcommand` — clap's default behaviour
    should allow `ranchero -v start` **and** `ranchero start -v`. Confirm
    both produce identical parsed structs.
11. `unknown_subcommand_is_error` — `ranchero explode` returns an
    `Err(ClapError)` whose kind is `InvalidSubcommand`.
12. `unknown_option_is_error` — `ranchero start --bogus` returns a
    `UnknownArgument` error.
13. `help_short_and_long_exits_success` — asking for `--help` (top-level
    *and* per-subcommand) returns the `DisplayHelp` error kind.
14. `version_flag_reports_crate_version` — `--version` returns
    `DisplayVersion` carrying `env!("CARGO_PKG_VERSION")`.

### Dispatch tests

15. `dispatch_returns_expected_stub_message` — a pure `run(cli: Cli) ->
    String` returns `"configure"`, `"start"`, `"stop"`, `"status"` for
    each variant (and includes global-option state when `--verbose`
    is set, e.g. `"start (verbose)"`).

### Anti-tests (guard against regressions we know we want)

16. `password_on_cli_without_verbose_is_silent` — asserting no warning
    string in the run output when `--mainpassword` is set but `-v` is
    not.
17. `password_on_cli_with_verbose_warns` — the opposite; a warning is
    present.

## Implementation outline

Single crate for now (STEP 01 doesn't need the workspace split yet):

```
ranchero/
  Cargo.toml            # add clap = { version = "4", features = ["derive"] }
  src/
    main.rs             # parse argv, call run(), print result, set exit code
    lib.rs              # pub mod cli;
    cli.rs              # Cli struct (with clap derive), Command enum,
                        # GlobalOpts struct, parse_from(args), run(cli)
  tests/
    cli_args.rs         # integration tests that exercise parse_from
```

Key types (sketch, not binding):

```rust
// src/cli.rs
#[derive(clap::Parser, Debug, PartialEq, Eq)]
#[command(name = "ranchero", version, about)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalOpts,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(clap::Args, Debug, PartialEq, Eq, Default)]
pub struct GlobalOpts {
    #[arg(short = 'v', long)]                pub verbose: bool,
    #[arg(short = 'D', long)]                pub debug: bool,
    #[arg(long)]                             pub foreground: bool,
    #[arg(long, value_name = "EMAIL")]       pub mainuser: Option<String>,
    #[arg(long, value_name = "PASSWORD")]    pub mainpassword: Option<String>,
    #[arg(long, value_name = "EMAIL")]       pub monitoruser: Option<String>,
    #[arg(long, value_name = "PASSWORD")]    pub monitorpassword: Option<String>,
    #[arg(long, value_name = "PATH")]        pub config: Option<PathBuf>,
}

#[derive(clap::Subcommand, Debug, PartialEq, Eq)]
pub enum Command { Configure, Start, Stop, Status }

pub fn parse_from<I, T>(args: I) -> Result<Cli, clap::Error>
where I: IntoIterator<Item = T>, T: Into<OsString> + Clone {
    let mut cli = Cli::try_parse_from(args)?;
    cli.global.finalize();
    Ok(cli)
}

pub fn run(cli: Cli) -> String { /* returns stub message */ }
```

`GlobalOpts::finalize()` is the single pure resolver for derived state:

```rust
impl GlobalOpts {
    pub fn finalize(&mut self) {
        if self.debug { self.foreground = true; }
    }
}
```

`main.rs` is tiny:

```rust
fn main() -> ExitCode {
    match ranchero::cli::parse_from(std::env::args_os()) {
        Ok(cli)   => { println!("{}", ranchero::cli::run(cli)); ExitCode::SUCCESS }
        Err(err)  => err.exit(),  // clap handles exit codes for help/version/errors
    }
}
```

## Acceptance criteria

- `cargo test` — all tests pass, no warnings.
- `cargo run -- --help` prints a help page listing every subcommand and
  every global option above.
- `cargo run -- start -v` prints the verbose stub message.
- `cargo run -- stop --mainuser a@b --mainpassword x` prints the stub
  message and exits 0.
- No other crates or modules introduced. No config reading, no network,
  no file I/O beyond stdout/stderr from clap.

## Deferred to later steps

- Actually honouring any option: STEP 02 (config), STEP 03 (daemon), STEP
  04 (logging), STEP 05 (credentials).
- The interactive TUI behind `ranchero configure`: STEP 02.
- Workspace split: triggered as soon as STEP 06 introduces `zwift-proto`.
- Env-var fallbacks (`RANCHERO_MAINUSER` etc.): STEP 02 when we introduce
  the full precedence chain.
