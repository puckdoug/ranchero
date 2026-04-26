//! PID file: small wrapper around a path that holds the daemon PID.
//!
//! Writes are atomic via [`crate::config::atomic_write`], which renames a
//! tempfile into place after fsync. Reads tolerate a missing file (returning
//! `None`) and trim trailing whitespace so concurrent reads of partially-
//! written files surface as parse errors rather than truncated PIDs.

use std::io;
use std::path::{Path, PathBuf};

use crate::config::atomic_write;

#[derive(Debug, Clone)]
pub struct Pidfile {
    path: PathBuf,
}

impl Pidfile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path { &self.path }

    /// Atomically write `pid` followed by a newline to the file. Creates
    /// parent directories as needed.
    pub fn write(&self, pid: u32) -> io::Result<()> {
        let body = format!("{pid}\n");
        atomic_write(&self.path, body.as_bytes())
            .map_err(|e| io::Error::other(e.to_string()))
    }

    /// Read the PID. Returns:
    /// - `Ok(Some(pid))` if the file exists and parses cleanly,
    /// - `Ok(None)` if the file does not exist,
    /// - `Err(_)` if the file exists but cannot be read or parsed.
    pub fn read(&self) -> io::Result<Option<u32>> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("pid file {} is empty", self.path.display()),
                    ));
                }
                let pid: u32 = trimmed.parse().map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("pid file {} contains invalid pid: {e}", self.path.display()),
                    )
                })?;
                Ok(Some(pid))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Remove the file if it exists; missing-file is not an error.
    pub fn remove(&self) -> io::Result<()> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_encoder_writes_pid_and_newline() {
        let dir = tempfile::tempdir().unwrap();
        let p = Pidfile::new(dir.path().join("ranchero.pid"));
        p.write(12345).unwrap();
        let body = std::fs::read_to_string(p.path()).unwrap();
        assert_eq!(body, "12345\n");
    }

    #[test]
    fn pid_file_reader_returns_pid_or_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = Pidfile::new(dir.path().join("ranchero.pid"));
        // Missing → None
        assert_eq!(p.read().unwrap(), None);
        // Present → Some(pid)
        p.write(99).unwrap();
        assert_eq!(p.read().unwrap(), Some(99));
    }

    #[test]
    fn pid_file_remove_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = Pidfile::new(dir.path().join("ranchero.pid"));
        // Removing a non-existent file is OK.
        p.remove().unwrap();
        p.write(1).unwrap();
        p.remove().unwrap();
        assert_eq!(p.read().unwrap(), None);
    }

    #[test]
    fn pid_file_reader_rejects_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ranchero.pid");
        std::fs::write(&path, "not a number\n").unwrap();
        let p = Pidfile::new(path);
        assert!(p.read().is_err());
    }
}
