//! Structured logging configuration for ranchero.
//!
//! STEP 04 surface, ahead of implementation. Three pure helpers form the
//! testable core; a future `install()` wraps them and registers the
//! global `tracing_subscriber`. All bodies currently `todo!()`; the
//! tests at the bottom of this file capture the contract and will fail
//! red until implementation lands.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

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
    /// Background mode: write to a rolling file at this path.
    File(PathBuf),
}

/// Compute the EnvFilter directive given `--verbose` / `--debug` and the
/// `RUST_LOG` environment value, if any. `RUST_LOG` wins verbatim if set
/// and non-empty.
///
/// - neither flag, no env → `"warn"`
/// - `--verbose` → `"warn,ranchero=info"`
/// - `--debug` → `"info,ranchero=debug"`
/// - `RUST_LOG=X` (non-empty) → `X` regardless of flags
pub fn filter_directive(_opts: LogOpts, _rust_log: Option<&str>) -> String {
    todo!("STEP-04: filter_directive implementation pending")
}

/// Pick where logs should be written. Foreground → stderr; background →
/// the configured `log_file` path.
pub fn select_sink(_foreground: bool, _log_file: &Path) -> LogSink {
    todo!("STEP-04: select_sink implementation pending")
}

/// Open `path` for appending, creating parent directories as needed.
/// Existing contents are preserved.
pub fn open_log_for_append(_path: &Path) -> io::Result<File> {
    todo!("STEP-04: open_log_for_append implementation pending")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // -- filter_directive ---------------------------------------------------

    #[test]
    fn defaults_to_warn() {
        let dir = filter_directive(LogOpts::default(), None);
        assert_eq!(
            dir, "warn",
            "no flags + no RUST_LOG should produce a bare `warn` directive, got {dir:?}"
        );
    }

    #[test]
    fn subscriber_respects_verbose_flag() {
        let dir = filter_directive(
            LogOpts { verbose: true, debug: false },
            None,
        );
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
        let dir = filter_directive(
            LogOpts { verbose: false, debug: true },
            None,
        );
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
        let dir = filter_directive(
            LogOpts { verbose: true, debug: true },
            None,
        );
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
            Some("trace"),
        );
        assert_eq!(dir, "trace", "RUST_LOG should be returned verbatim");
    }

    #[test]
    fn rust_log_env_wins_with_complex_directive() {
        let dir = filter_directive(
            LogOpts::default(),
            Some("hyper=warn,ranchero=trace"),
        );
        assert_eq!(dir, "hyper=warn,ranchero=trace");
    }

    #[test]
    fn empty_rust_log_falls_back_to_flags() {
        // An empty RUST_LOG should be treated as unset, not as the directive "".
        let dir = filter_directive(
            LogOpts { verbose: true, debug: false },
            Some(""),
        );
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
