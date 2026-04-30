//! Integration tests for the `ranchero` CLI argument parser and dispatcher.
//!
//! These tests exercise the public API exposed by the `ranchero` library
//! crate: `parse_from(args)` returns a parsed `Cli`, and `run(cli)`
//! returns a stub string describing which subcommand was selected.

use clap::error::ErrorKind;
use ranchero::cli::{parse_from, run, Command};

fn parse(args: &[&str]) -> ranchero::cli::Cli {
    parse_from(args).expect("args should parse")
}

// -- subcommand parsing ---------------------------------------------------

#[test]
fn parses_start_with_no_options() {
    let cli = parse(&["ranchero", "start"]);
    assert_eq!(cli.command, Command::Start);
    assert!(!cli.global.verbose);
    assert!(!cli.global.debug);
    assert!(!cli.global.foreground);
    assert!(cli.global.mainuser.is_none());
    assert!(cli.global.mainpassword.is_none());
    assert!(cli.global.monitoruser.is_none());
    assert!(cli.global.monitorpassword.is_none());
    assert!(cli.global.config.is_none());
}

#[test]
fn parses_stop_subcommand() {
    let cli = parse(&["ranchero", "stop"]);
    assert_eq!(cli.command, Command::Stop);
}

#[test]
fn parses_status_subcommand() {
    let cli = parse(&["ranchero", "status"]);
    assert_eq!(cli.command, Command::Status);
}

#[test]
fn parses_configure_subcommand() {
    let cli = parse(&["ranchero", "configure"]);
    assert_eq!(cli.command, Command::Configure);
}

#[test]
fn parses_auth_check_subcommand() {
    let cli = parse(&["ranchero", "auth-check"]);
    assert_eq!(cli.command, Command::AuthCheck);
}

#[test]
fn start_with_capture_flag_captures_path() {
    let cli = parse(&["ranchero", "start", "--capture", "/tmp/x.cap"]);
    assert_eq!(cli.command, Command::Start);
    assert_eq!(
        cli.global.capture.as_deref(),
        Some(std::path::Path::new("/tmp/x.cap")),
    );
}

#[test]
fn parses_replay_subcommand() {
    let cli = parse(&["ranchero", "replay", "/tmp/x.cap"]);
    match cli.command {
        Command::Replay { ref path, verbose } => {
            assert_eq!(path, std::path::Path::new("/tmp/x.cap"));
            assert!(!verbose, "verbose defaults to false");
        }
        other => panic!("expected Replay, got {other:?}"),
    }
}

#[test]
fn parses_replay_with_verbose() {
    let cli = parse(&["ranchero", "replay", "/tmp/x.cap", "--verbose"]);
    match cli.command {
        Command::Replay { verbose, .. } => assert!(verbose),
        other => panic!("expected Replay {{ verbose: true }}, got {other:?}"),
    }
}

// -- STEP-12.2: `follow` subcommand parsing ------------------------------

#[test]
fn parses_follow_subcommand() {
    let cli = parse(&["ranchero", "follow", "/tmp/x.cap"]);
    match cli.command {
        Command::Follow { ref path, decode, idle_timeout } => {
            assert_eq!(path, std::path::Path::new("/tmp/x.cap"));
            assert!(!decode, "decode defaults to false");
            assert!(idle_timeout.is_none(), "idle_timeout defaults to None");
        }
        other => panic!("expected Follow, got {other:?}"),
    }
}

#[test]
fn parses_follow_with_decode() {
    let cli = parse(&["ranchero", "follow", "/tmp/x.cap", "--decode"]);
    match cli.command {
        Command::Follow { decode, .. } => assert!(decode),
        other => panic!("expected Follow {{ decode: true }}, got {other:?}"),
    }
}

#[test]
fn parses_follow_with_idle_timeout() {
    let cli = parse(&["ranchero", "follow", "/tmp/x.cap", "--idle-timeout", "30"]);
    match cli.command {
        Command::Follow { idle_timeout, .. } => {
            assert_eq!(idle_timeout, Some(30));
        }
        other => panic!("expected Follow with idle_timeout, got {other:?}"),
    }
}

#[test]
fn dispatch_follow_stub() {
    let cli = parse(&["ranchero", "follow", "/tmp/x.cap"]);
    assert!(run(cli).contains("follow"));
}

// -- global flag parsing --------------------------------------------------

#[test]
fn verbose_flag_long() {
    let cli = parse(&["ranchero", "--verbose", "start"]);
    assert!(cli.global.verbose);
}

#[test]
fn verbose_flag_short() {
    let cli = parse(&["ranchero", "-v", "start"]);
    assert!(cli.global.verbose);
}

#[test]
fn debug_flag_long() {
    let cli = parse(&["ranchero", "--debug", "start"]);
    assert!(cli.global.debug);
}

#[test]
fn debug_flag_uses_capital_d() {
    let cli = parse(&["ranchero", "-D", "start"]);
    assert!(cli.global.debug);
}

#[test]
fn lowercase_d_is_not_debug() {
    // `-d` must not be accepted as a shorthand for --debug.
    let err = parse_from(["ranchero", "-d", "start"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn debug_implies_foreground() {
    let cli = parse(&["ranchero", "--debug", "start"]);
    assert!(cli.global.debug);
    assert!(
        cli.global.foreground,
        "--debug must imply --foreground after finalize()"
    );
}

#[test]
fn explicit_foreground_without_debug() {
    let cli = parse(&["ranchero", "--foreground", "start"]);
    assert!(cli.global.foreground);
    assert!(!cli.global.debug);
}

#[test]
fn main_credentials_capture_both_parts() {
    let cli = parse(&[
        "ranchero",
        "--mainuser",
        "rider@example.com",
        "--mainpassword",
        "hunter2",
        "start",
    ]);
    assert_eq!(cli.global.mainuser.as_deref(), Some("rider@example.com"));
    assert_eq!(cli.global.mainpassword.as_deref(), Some("hunter2"));
}

#[test]
fn monitor_credentials_capture_both_parts() {
    let cli = parse(&[
        "ranchero",
        "--monitoruser",
        "bot@example.com",
        "--monitorpassword",
        "s3cret",
        "start",
    ]);
    assert_eq!(cli.global.monitoruser.as_deref(), Some("bot@example.com"));
    assert_eq!(cli.global.monitorpassword.as_deref(), Some("s3cret"));
}

#[test]
fn config_path_captured() {
    let cli = parse(&["ranchero", "--config", "/tmp/ranchero.toml", "start"]);
    assert_eq!(
        cli.global.config.as_deref(),
        Some(std::path::Path::new("/tmp/ranchero.toml"))
    );
}

#[test]
fn options_work_before_subcommand() {
    let cli = parse(&["ranchero", "-v", "start"]);
    assert!(cli.global.verbose);
    assert_eq!(cli.command, Command::Start);
}

#[test]
fn options_work_after_subcommand() {
    let cli = parse(&["ranchero", "start", "-v"]);
    assert!(cli.global.verbose);
    assert_eq!(cli.command, Command::Start);
}

// -- error paths ----------------------------------------------------------

#[test]
fn unknown_subcommand_is_error() {
    let err = parse_from(["ranchero", "explode"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidSubcommand);
}

#[test]
fn unknown_option_is_error() {
    let err = parse_from(["ranchero", "start", "--bogus"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::UnknownArgument);
}

#[test]
fn help_flag_yields_display_help() {
    let err = parse_from(["ranchero", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn help_flag_per_subcommand_yields_display_help() {
    let err = parse_from(["ranchero", "start", "--help"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayHelp);
}

#[test]
fn version_flag_reports_crate_version() {
    let err = parse_from(["ranchero", "--version"]).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::DisplayVersion);
    let rendered = err.to_string();
    assert!(
        rendered.contains(env!("CARGO_PKG_VERSION")),
        "version output `{rendered}` missing crate version"
    );
}

// -- dispatch tests -------------------------------------------------------

#[test]
fn dispatch_configure_stub() {
    let cli = parse(&["ranchero", "configure"]);
    assert!(run(cli).contains("configure"));
}

#[test]
fn dispatch_start_stub() {
    let cli = parse(&["ranchero", "start"]);
    assert!(run(cli).contains("start"));
}

#[test]
fn dispatch_stop_stub() {
    let cli = parse(&["ranchero", "stop"]);
    assert!(run(cli).contains("stop"));
}

#[test]
fn dispatch_status_stub() {
    let cli = parse(&["ranchero", "status"]);
    assert!(run(cli).contains("status"));
}

#[test]
fn dispatch_replay_stub() {
    let cli = parse(&["ranchero", "replay", "/tmp/x.cap"]);
    assert!(run(cli).contains("replay"));
}

#[test]
fn dispatch_start_passes_capture_path_to_daemon() {
    // STEP-12.1 contract: dispatch must (a) not reject `--capture`
    // with the STEP-11.6 Fix-D guard, and (b) pass the capture
    // path through to `daemon::start`. The fully-wired test
    // requires an injection point so that the daemon's received
    // path is observable; until that lands, this test fails
    // because dispatch still returns the Fix-D guard error.
    //
    // The test runs `dispatch()` in-process. Because `dispatch()`
    // constructs an `OsKeyringStore` from the config file's
    // `[keyring].service` field and consults it during
    // `ResolvedConfig::resolve`, the test must point that field at
    // a non-production service so the macOS Keychain is not
    // queried for the operator's real Zwift credentials.
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join("ranchero.toml");
    std::fs::write(
        &config_path,
        "schema_version = 1\n\
         [keyring]\n\
         service = \"ranchero-test-cli-args\"\n",
    )
    .unwrap();

    let cli = parse(&[
        "ranchero",
        "--config",
        config_path.to_str().unwrap(),
        "--capture",
        "/tmp/x.cap",
        "start",
    ]);
    let result = ranchero::cli::dispatch(cli);
    let saw_fix_d_guard = matches!(
        &result,
        Err(e) if e.to_string().contains("--capture") && e.to_string().contains("STEP 12"),
    );
    assert!(
        !saw_fix_d_guard,
        "STEP-12.1 red state: the Fix-D guard from STEP-11.6 must be \
         removed and the capture path must be wired through to \
         daemon::start; got: {:?}",
        result.as_ref().err().map(|e| e.to_string()),
    );
}

#[test]
fn dispatch_auth_check_stub() {
    let cli = parse(&["ranchero", "auth-check"]);
    assert!(run(cli).contains("auth-check"));
}

#[test]
fn dispatch_reports_verbose_when_set() {
    let cli = parse(&["ranchero", "-v", "start"]);
    assert!(
        run(cli).contains("verbose"),
        "verbose flag should be reflected in the stub output"
    );
}

// -- password-leak warning guard -----------------------------------------

#[test]
fn password_on_cli_without_verbose_is_silent() {
    let cli = parse(&[
        "ranchero",
        "--mainpassword",
        "hunter2",
        "start",
    ]);
    let out = run(cli);
    assert!(
        !out.to_lowercase().contains("warning"),
        "should not warn about CLI password unless -v is set; got: {out}"
    );
}

#[test]
fn password_on_cli_with_verbose_warns() {
    let cli = parse(&[
        "ranchero",
        "-v",
        "--mainpassword",
        "hunter2",
        "start",
    ]);
    let out = run(cli);
    assert!(
        out.to_lowercase().contains("warning"),
        "should warn about CLI password when -v is set; got: {out}"
    );
}
