//! Structured logging configuration for ranchero.
//!
//! Three pure helpers form the testable core; [`install`] wires them
//! into the global `tracing_subscriber` and returns a guard that flushes
//! the non-blocking appender on drop. Callers must keep the guard alive
//! for the duration of the process — dropping it stops the writer.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

pub use tracing_appender::non_blocking::WorkerGuard;

/// Inputs that influence the EnvFilter directive choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogOpts {
    pub verbose: bool,
    pub debug: bool,
}

/// Where logs should land.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogSink {
    /// Foreground mode: write to stderr.
    Stderr,
    /// Background mode: write to a file at this path (append-mode).
    File(PathBuf),
}

/// Compute the EnvFilter directive given `--verbose` / `--debug`, the
/// foreground bit, the `RUST_LOG` environment value, and the configured
/// log level from the TOML file. Precedence, highest to lowest:
///
/// 1. `RUST_LOG` (non-empty) — returned verbatim.
/// 2. `--debug` → `"info,ranchero=debug,zwift_relay=debug,zwift_api=debug"`
///    so the per-packet tracing emitted from the relay and auth crates
///    reaches the log alongside the daemon's own events (STEP-12.12).
/// 3. `--verbose` or background → `"warn,ranchero=info"`
/// 4. `configured_level` (from `[logging] level` in TOML) →
///    `"warn,ranchero=<level>"`
/// 5. Foreground default → `"warn"`
pub fn filter_directive(
    opts: LogOpts,
    foreground: bool,
    rust_log: Option<&str>,
    configured_level: Option<crate::config::LogLevel>,
) -> String {
    if let Some(s) = rust_log
        && !s.is_empty()
    {
        return s.to_string();
    }
    if opts.debug {
        "info,ranchero=debug,zwift_relay=debug,zwift_api=debug".to_string()
    } else if opts.verbose || !foreground {
        "warn,ranchero=info".to_string()
    } else if let Some(level) = configured_level {
        format!("warn,ranchero={level}")
    } else {
        "warn".to_string()
    }
}

/// Pick where logs should be written. Foreground → stderr; background →
/// the configured `log_file` path.
pub fn select_sink(foreground: bool, log_file: &Path) -> LogSink {
    if foreground {
        LogSink::Stderr
    } else {
        LogSink::File(log_file.to_path_buf())
    }
}

/// Open `path` for appending, creating parent directories as needed.
/// Existing contents are preserved.
pub fn open_log_for_append(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(path)
}

/// Install the global tracing subscriber and return its non-blocking
/// writer guard.
///
/// **Fork warning:** the non-blocking appender spawns a worker thread
/// that does not survive `fork(2)`. Callers that daemonize must invoke
/// `install` *after* the final fork, in the long-running child.
pub fn install(
    opts: LogOpts,
    foreground: bool,
    log_file: &Path,
    configured_level: Option<crate::config::LogLevel>,
) -> io::Result<WorkerGuard> {
    use tracing_subscriber::{EnvFilter, fmt, prelude::*};

    let rust_log = std::env::var("RUST_LOG").ok();
    let directive = filter_directive(opts, foreground, rust_log.as_deref(), configured_level);
    let env_filter =
        EnvFilter::try_new(&directive).unwrap_or_else(|_| EnvFilter::new("warn"));

    let (non_blocking, guard) = match select_sink(foreground, log_file) {
        LogSink::Stderr => tracing_appender::non_blocking(std::io::stderr()),
        LogSink::File(path) => {
            let file = open_log_for_append(&path)?;
            tracing_appender::non_blocking(file)
        }
    };

    let layer = fmt::layer().with_writer(non_blocking).with_ansi(false);
    let subscriber = tracing_subscriber::registry().with(env_filter).with(layer);
    let _ = tracing::subscriber::set_global_default(subscriber);
    Ok(guard)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // -- filter_directive ---------------------------------------------------

    const FG: bool = true;
    const BG: bool = false;

    #[test]
    fn foreground_defaults_to_warn() {
        let dir = filter_directive(LogOpts::default(), FG, None, None);
        assert_eq!(
            dir, "warn",
            "no flags + foreground + no RUST_LOG should be bare `warn`, got {dir:?}"
        );
    }

    #[test]
    fn background_defaults_promote_ranchero_to_info() {
        // Lifecycle events (info!) must reach the configured logfile
        // even without -v; this is the regression for an empty logfile
        // after `ranchero start; ranchero stop` with default flags.
        let dir = filter_directive(LogOpts::default(), BG, None, None);
        assert!(
            dir.contains("ranchero=info"),
            "backgrounded daemon default must let ranchero=info through, got {dir:?}"
        );
        assert!(
            dir.contains("warn"),
            "deps should still be at warn for backgrounded default, got {dir:?}"
        );
    }

    #[test]
    fn subscriber_respects_verbose_flag() {
        let dir = filter_directive(LogOpts { verbose: true, debug: false }, FG, None, None);
        assert!(
            dir.contains("ranchero=info"),
            "verbose should set ranchero=info, got {dir:?}"
        );
        assert!(
            dir.contains("warn"),
            "verbose should keep deps at warn, got {dir:?}"
        );
        assert!(
            !dir.contains("ranchero=debug"),
            "verbose must not promote ranchero to debug, got {dir:?}"
        );
    }

    #[test]
    fn subscriber_respects_debug_flag() {
        let dir = filter_directive(LogOpts { verbose: false, debug: true }, FG, None, None);
        assert!(
            dir.contains("ranchero=debug"),
            "debug should set ranchero=debug, got {dir:?}"
        );
        assert!(
            dir.contains("info"),
            "debug should put deps at info, got {dir:?}"
        );
    }

    #[test]
    fn debug_overrides_verbose_when_both_set() {
        // --debug takes precedence; ranchero=debug, deps at info.
        let dir = filter_directive(LogOpts { verbose: true, debug: true }, FG, None, None);
        assert!(
            dir.contains("ranchero=debug"),
            "debug should win over verbose, got {dir:?}"
        );
    }

    #[test]
    fn rust_log_env_wins_over_flags() {
        // Even with both flags set, a non-empty RUST_LOG passes through verbatim.
        let dir = filter_directive(
            LogOpts { verbose: true, debug: true },
            FG,
            Some("trace"),
            None,
        );
        assert_eq!(dir, "trace", "RUST_LOG should be returned verbatim");
    }

    #[test]
    fn rust_log_env_wins_for_background_too() {
        // RUST_LOG overrides even the lifecycle-friendly background default.
        let dir = filter_directive(LogOpts::default(), BG, Some("error"), None);
        assert_eq!(dir, "error");
    }

    #[test]
    fn rust_log_env_wins_with_complex_directive() {
        let dir = filter_directive(
            LogOpts::default(),
            FG,
            Some("hyper=warn,ranchero=trace"),
            None,
        );
        assert_eq!(dir, "hyper=warn,ranchero=trace");
    }

    #[test]
    fn empty_rust_log_falls_back_to_flags() {
        // An empty RUST_LOG should be treated as unset, not as the directive "".
        let dir = filter_directive(LogOpts { verbose: true, debug: false }, FG, Some(""), None);
        assert!(
            dir.contains("ranchero=info"),
            "empty RUST_LOG must not silence flag-derived directives, got {dir:?}"
        );
    }

    // -- select_sink --------------------------------------------------------

    #[test]
    fn select_sink_foreground_is_stderr() {
        let path = PathBuf::from("/tmp/ranchero.log");
        assert_eq!(select_sink(true, &path), LogSink::Stderr);
    }

    #[test]
    fn select_sink_background_is_logfile() {
        let path = PathBuf::from("/tmp/ranchero.log");
        assert_eq!(select_sink(false, &path), LogSink::File(path));
    }

    // -- open_log_for_append ------------------------------------------------

    #[test]
    fn logfile_is_opened_for_append_when_backgrounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Nested path forces parent-directory creation.
        let path = dir.path().join("nested").join("ranchero.log");

        {
            let mut f = open_log_for_append(&path).expect("first open should succeed");
            writeln!(f, "first line").unwrap();
        }
        {
            let mut f = open_log_for_append(&path).expect("second open should succeed");
            writeln!(f, "second line").unwrap();
        }

        let contents = std::fs::read_to_string(&path).expect("logfile readable");
        assert!(
            contents.contains("first line"),
            "append mode must preserve prior content, got {contents:?}"
        );
        assert!(
            contents.contains("second line"),
            "second write should be appended, got {contents:?}"
        );
    }

    #[test]
    fn open_log_for_append_creates_missing_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("a").join("b").join("c").join("ranchero.log");

        let mut f = open_log_for_append(&path).expect("open with missing parents");
        writeln!(f, "hello").unwrap();
        drop(f);

        let contents = std::fs::read_to_string(&path).expect("file written");
        assert!(contents.contains("hello"));
    }
}
