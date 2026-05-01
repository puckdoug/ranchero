//! Red-state tests for Defect 8: `ResolvedConfig.log_level` is read from
//! TOML but silently ignored by `logging::install` / `filter_directive`.
//!
//! Each test calls `filter_directive` with a fourth `configured_level`
//! argument that does not yet exist in the signature. The target fails
//! to compile until the signature is extended and the body consults it.

use ranchero::config::LogLevel;
use ranchero::logging::{filter_directive, LogOpts};

const FG: bool = true;

#[test]
fn filter_directive_uses_configured_level_when_no_cli_flags() {
    let dir = filter_directive(LogOpts::default(), FG, None, Some(LogLevel::Debug));
    assert!(
        dir.contains("debug"),
        "configured log level must appear when no CLI flags are set, got {dir:?}",
    );
}

#[test]
fn filter_directive_verbose_overrides_configured_level() {
    let dir = filter_directive(
        LogOpts { verbose: true, debug: false },
        FG,
        None,
        Some(LogLevel::Warn),
    );
    assert!(
        dir.contains("ranchero=info"),
        "verbose flag must override configured level, got {dir:?}",
    );
}

#[test]
fn filter_directive_debug_overrides_configured_level() {
    let dir = filter_directive(
        LogOpts { verbose: false, debug: true },
        FG,
        None,
        Some(LogLevel::Warn),
    );
    assert!(
        dir.contains("ranchero=debug"),
        "debug flag must override configured level, got {dir:?}",
    );
}

#[test]
fn filter_directive_rust_log_overrides_configured_level() {
    let dir = filter_directive(
        LogOpts::default(),
        FG,
        Some("error"),
        Some(LogLevel::Trace),
    );
    assert_eq!(dir, "error");
}
