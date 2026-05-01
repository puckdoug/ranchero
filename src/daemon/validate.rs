use std::fmt;
use std::path::{Path, PathBuf};

use crate::config::ResolvedConfig;

#[derive(Debug)]
pub enum StartupValidationError {
    MissingEmail,
    MissingPassword,
    DirectoryNotWritable { label: &'static str, path: PathBuf, reason: String },
}

impl fmt::Display for StartupValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEmail =>
                write!(f, "missing main account email; set one via `ranchero configure`"),
            Self::MissingPassword =>
                write!(f, "missing main account password; set one via `ranchero configure`"),
            Self::DirectoryNotWritable { label, path, reason } =>
                write!(f, "{label} directory is not writable ({}): {reason}", path.display()),
        }
    }
}

#[derive(Debug)]
pub struct StartupValidationErrors(pub Vec<StartupValidationError>);

impl fmt::Display for StartupValidationErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "startup validation failed:")?;
        for err in &self.0 {
            write!(f, "\n  - {err}")?;
        }
        Ok(())
    }
}

fn probe_writable(dir: &Path) -> Result<(), String> {
    let probe = dir.join(format!(".ranchero-probe-{}", std::process::id()));
    std::fs::write(&probe, b"")
        .and_then(|_| std::fs::remove_file(&probe))
        .map_err(|e| e.to_string())
}

pub fn validate_startup(
    cfg: &ResolvedConfig,
    capture_path: Option<&Path>,
) -> Result<(), StartupValidationErrors> {
    let mut errors = Vec::new();

    // S-1: Relay credential presence (monitor account required)
    if cfg.relay_enabled {
        if cfg.monitor_email.is_none() {
            errors.push(StartupValidationError::MissingEmail);
        }
        if cfg.monitor_password.is_none() {
            errors.push(StartupValidationError::MissingPassword);
        }
    }

    // S-2: Pidfile directory writability
    if let Some(parent) = cfg.pidfile.parent() {
        if let Err(reason) = probe_writable(parent) {
            errors.push(StartupValidationError::DirectoryNotWritable {
                label: "pidfile",
                path: parent.to_path_buf(),
                reason,
            });
        }
    }

    // S-3: Log file directory writability
    if let Some(parent) = cfg.log_file.parent() {
        if let Err(reason) = probe_writable(parent) {
            errors.push(StartupValidationError::DirectoryNotWritable {
                label: "log file",
                path: parent.to_path_buf(),
                reason,
            });
        }
    }

    // S-4: Capture path directory writability
    if let Some(capture) = capture_path {
        if let Some(parent) = capture.parent() {
            if let Err(reason) = probe_writable(parent) {
                errors.push(StartupValidationError::DirectoryNotWritable {
                    label: "capture file",
                    path: parent.to_path_buf(),
                    reason,
                });
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(StartupValidationErrors(errors))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{EditingMode, RedactedString, ZwiftEndpoints};

    fn make_config(
        relay_enabled: bool,
        main_email: Option<&str>,
        main_password: Option<&str>,
        monitor_email: Option<&str>,
        monitor_password: Option<&str>,
        pidfile: PathBuf,
        log_file: PathBuf,
    ) -> ResolvedConfig {
        ResolvedConfig {
            main_email: main_email.map(str::to_owned),
            main_password: main_password.map(RedactedString::new),
            monitor_email: monitor_email.map(str::to_owned),
            monitor_password: monitor_password.map(RedactedString::new),
            server_bind: "127.0.0.1".to_string(),
            server_port: 1080,
            server_https: false,
            log_level: None,
            log_file,
            pidfile,
            config_path: None,
            editing_mode: EditingMode::Default,
            zwift_endpoints: ZwiftEndpoints {
                auth_base: "https://secure.zwift.com".to_string(),
                api_base: "https://us-or-rly101.zwift.com".to_string(),
            },
            relay_enabled,
        }
    }

    fn writable_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let pidfile = dir.path().join("ranchero.pid");
        let log_file = dir.path().join("ranchero.log");
        (dir, pidfile, log_file)
    }

    fn has_missing_email(errs: &StartupValidationErrors) -> bool {
        errs.0.iter().any(|e| matches!(e, StartupValidationError::MissingEmail))
    }

    fn has_missing_password(errs: &StartupValidationErrors) -> bool {
        errs.0.iter().any(|e| matches!(e, StartupValidationError::MissingPassword))
    }

    fn has_not_writable(errs: &StartupValidationErrors, expected_label: &str) -> bool {
        errs.0.iter().any(|e| {
            matches!(e, StartupValidationError::DirectoryNotWritable { label, .. } if *label == expected_label)
        })
    }

    // S-1a
    #[test]
    fn validate_relay_enabled_no_email_returns_missing_email() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(true, None, None, None, Some("secret"), pidfile, log_file);
        let err = validate_startup(&cfg, None).expect_err("should fail with missing email");
        assert!(has_missing_email(&err), "expected MissingEmail in errors");
        assert!(!has_missing_password(&err), "expected no MissingPassword, password is set");
    }

    // S-1b
    #[test]
    fn validate_relay_enabled_no_password_returns_missing_password() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(true, None, None, Some("monitor@example.com"), None, pidfile, log_file);
        let err = validate_startup(&cfg, None).expect_err("should fail with missing password");
        assert!(has_missing_password(&err), "expected MissingPassword in errors");
        assert!(!has_missing_email(&err), "expected no MissingEmail, email is set");
    }

    // S-1c
    #[test]
    fn validate_relay_enabled_both_missing_returns_both_errors() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(true, None, None, None, None, pidfile, log_file);
        let err = validate_startup(&cfg, None).expect_err("should fail with both missing");
        assert!(has_missing_email(&err), "expected MissingEmail");
        assert!(has_missing_password(&err), "expected MissingPassword");
        let email_pos = err.0.iter().position(|e| matches!(e, StartupValidationError::MissingEmail));
        let pw_pos = err.0.iter().position(|e| matches!(e, StartupValidationError::MissingPassword));
        assert!(email_pos < pw_pos, "MissingEmail should precede MissingPassword");
    }

    // S-1d
    #[test]
    fn validate_relay_disabled_skips_credential_check() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        assert!(validate_startup(&cfg, None).is_ok(), "relay disabled should skip credential check");
    }

    // S-1e
    #[test]
    fn validate_relay_enabled_both_present_is_ok() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(true, None, None, Some("monitor@example.com"), Some("secret"), pidfile, log_file);
        assert!(validate_startup(&cfg, None).is_ok(), "both credentials present should be ok");
    }

    // S-2a
    #[test]
    fn validate_pidfile_dir_missing_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pidfile = dir.path().join("nonexistent_subdir").join("ranchero.pid");
        let log_file = dir.path().join("ranchero.log");
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        let err = validate_startup(&cfg, None).expect_err("missing pidfile dir should fail");
        assert!(has_not_writable(&err, "pidfile"), "expected DirectoryNotWritable for pidfile");
    }

    // S-2b
    #[test]
    fn validate_pidfile_dir_writable_is_ok() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        assert!(validate_startup(&cfg, None).is_ok(), "writable pidfile dir should be ok");
    }

    // S-3a
    #[test]
    fn validate_log_dir_missing_returns_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pidfile = dir.path().join("ranchero.pid");
        let log_file = dir.path().join("nonexistent_subdir").join("ranchero.log");
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        let err = validate_startup(&cfg, None).expect_err("missing log dir should fail");
        assert!(has_not_writable(&err, "log file"), "expected DirectoryNotWritable for log file");
    }

    // S-3b
    #[test]
    fn validate_log_dir_writable_is_ok() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        assert!(validate_startup(&cfg, None).is_ok(), "writable log dir should be ok");
    }

    // S-4a
    #[test]
    fn validate_capture_dir_missing_returns_error() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        let capture = PathBuf::from("/nonexistent/path/that/cannot/exist/capture.bin");
        let err = validate_startup(&cfg, Some(&capture)).expect_err("missing capture dir should fail");
        assert!(has_not_writable(&err, "capture file"), "expected DirectoryNotWritable for capture file");
    }

    // S-4b
    #[test]
    fn validate_capture_none_skips_check() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        let result = validate_startup(&cfg, None);
        if let Err(ref errs) = result {
            assert!(
                !has_not_writable(errs, "capture file"),
                "capture check should be skipped when capture_path is None"
            );
        }
    }

    // S-4c
    #[test]
    fn validate_capture_dir_writable_is_ok() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pidfile = dir.path().join("ranchero.pid");
        let log_file = dir.path().join("ranchero.log");
        let capture = dir.path().join("capture.bin");
        let cfg = make_config(false, None, None, None, None, pidfile, log_file);
        assert!(validate_startup(&cfg, Some(&capture)).is_ok(), "writable capture dir should be ok");
    }

    // S-1f: Defect 11 — relay enabled, monitor email absent → error even if main email is set.
    #[test]
    fn validate_relay_enabled_no_monitor_email_returns_error() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(
            true,
            Some("main@example.com"), Some("main-pass"),
            None, Some("monitor-pass"),
            pidfile, log_file,
        );
        let err = validate_startup(&cfg, None)
            .expect_err("missing monitor email should fail validation");
        assert!(
            has_missing_email(&err),
            "Defect 11 red state: expected MissingEmail for absent monitor email; \
             currently only checks the main account email",
        );
    }

    // S-1g: Defect 11 — relay enabled, monitor password absent → error even if main password is set.
    #[test]
    fn validate_relay_enabled_no_monitor_password_returns_error() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(
            true,
            Some("main@example.com"), Some("main-pass"),
            Some("monitor@example.com"), None,
            pidfile, log_file,
        );
        let err = validate_startup(&cfg, None)
            .expect_err("missing monitor password should fail validation");
        assert!(
            has_missing_password(&err),
            "Defect 11 red state: expected MissingPassword for absent monitor password; \
             currently only checks the main account password",
        );
    }

    // S-1h: Defect 11 — monitor credentials present, main credentials absent → ok.
    // The monitor account is the only account required for relay startup.
    #[test]
    fn validate_relay_enabled_monitor_credentials_sufficient_without_main() {
        let (_dir, pidfile, log_file) = writable_paths();
        let cfg = make_config(
            true,
            None, None,
            Some("monitor@example.com"), Some("monitor-pass"),
            pidfile, log_file,
        );
        assert!(
            validate_startup(&cfg, None).is_ok(),
            "Defect 11 red state: monitor credentials alone must be sufficient; \
             currently fails because the main account credentials are absent",
        );
    }
}
