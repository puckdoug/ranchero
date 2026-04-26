//! OS-keychain credential storage, sauce4zwift-compatible.
//!
//! sauce4zwift's `src/secrets.mjs` stores Zwift credentials in the OS
//! keychain under service name `"Zwift Credentials - Sauce for Zwift"`,
//! keyed by `"zwift-login"` (main account) and `"zwift-monitor-login"`
//! (monitor account), with each value a `JSON.stringify({username,
//! password})` blob. ranchero uses the same service name, account names,
//! and JSON format so an existing sauce install's keychain entries are
//! picked up unchanged.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// OS keychain service name used by sauce4zwift's `src/secrets.mjs`.
/// Sharing it lets ranchero pick up existing sauce installs unchanged.
pub const SAUCE_SERVICE_NAME: &str = "Zwift Credentials - Sauce for Zwift";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyringEntry {
    pub username: String,
    pub password: String,
}

#[derive(Debug)]
pub enum KeyringError {
    UnknownRole(String),
    Backend(String),
    Serialization(String),
}

impl std::fmt::Display for KeyringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyringError::UnknownRole(r)   => write!(f, "unknown credential role: {r}"),
            KeyringError::Backend(s)       => write!(f, "keyring backend error: {s}"),
            KeyringError::Serialization(s) => write!(f, "credential blob malformed: {s}"),
        }
    }
}

impl std::error::Error for KeyringError {}

/// Translate a domain role ID to the OS-keychain account name sauce4zwift
/// uses. `"main"` -> `"zwift-login"`, `"monitor"` -> `"zwift-monitor-login"`.
pub fn sauce_account_name(role: &str) -> Result<&'static str, KeyringError> {
    match role {
        "main"    => Ok("zwift-login"),
        "monitor" => Ok("zwift-monitor-login"),
        other     => Err(KeyringError::UnknownRole(other.to_string())),
    }
}

/// Serialize a credential pair to the same compact JSON sauce4zwift's
/// `JSON.stringify({username, password})` produces:
/// `{"username":"<u>","password":"<p>"}`.
pub fn serialize_credentials(username: &str, password: &str) -> Result<String, KeyringError> {
    #[derive(Serialize)]
    struct Wire<'a> {
        username: &'a str,
        password: &'a str,
    }
    serde_json::to_string(&Wire { username, password })
        .map_err(|e| KeyringError::Serialization(e.to_string()))
}

/// Parse a sauce-shaped JSON credential blob. Tolerates extra fields;
/// rejects malformed JSON or missing `username`/`password`.
pub fn parse_credentials(blob: &str) -> Result<KeyringEntry, KeyringError> {
    #[derive(Deserialize)]
    struct Wire {
        username: String,
        password: String,
    }
    let w: Wire = serde_json::from_str(blob)
        .map_err(|e| KeyringError::Serialization(e.to_string()))?;
    Ok(KeyringEntry { username: w.username, password: w.password })
}

pub trait KeyringStore {
    fn set(&mut self, role: &str, username: &str, password: &str) -> Result<(), KeyringError>;
    fn get(&self, role: &str) -> Result<Option<KeyringEntry>, KeyringError>;
    fn delete(&mut self, role: &str) -> Result<(), KeyringError>;
}

#[derive(Default)]
pub struct InMemoryKeyringStore {
    entries: HashMap<String, KeyringEntry>,
}

impl KeyringStore for InMemoryKeyringStore {
    fn set(&mut self, role: &str, username: &str, password: &str) -> Result<(), KeyringError> {
        self.entries.insert(
            role.to_string(),
            KeyringEntry { username: username.to_string(), password: password.to_string() },
        );
        Ok(())
    }

    fn get(&self, role: &str) -> Result<Option<KeyringEntry>, KeyringError> {
        Ok(self.entries.get(role).cloned())
    }

    fn delete(&mut self, role: &str) -> Result<(), KeyringError> {
        self.entries.remove(role);
        Ok(())
    }
}

/// OS-keychain-backed `KeyringStore` (macOS Keychain, Windows Credential
/// Manager, libsecret on Linux), via the `keyring` crate.
pub struct OsKeyringStore {
    service: String,
}

impl OsKeyringStore {
    /// Production constructor — uses the sauce4zwift service name. All
    /// non-test code paths must use this.
    pub fn new() -> Self {
        Self { service: SAUCE_SERVICE_NAME.to_string() }
    }

    /// Test-only constructor letting callers write under a disposable
    /// service name so test entries cannot collide with the user's real
    /// sauce4zwift credentials.
    pub fn with_service_name(service: &str) -> Self {
        Self { service: service.to_string() }
    }

    fn entry_for(&self, role: &str) -> Result<keyring::Entry, KeyringError> {
        let account = sauce_account_name(role)?;
        keyring::Entry::new(&self.service, account)
            .map_err(|e| KeyringError::Backend(e.to_string()))
    }
}

impl Default for OsKeyringStore {
    fn default() -> Self { Self::new() }
}

impl KeyringStore for OsKeyringStore {
    fn set(&mut self, role: &str, username: &str, password: &str) -> Result<(), KeyringError> {
        let entry = self.entry_for(role)?;
        let blob = serialize_credentials(username, password)?;
        entry.set_password(&blob)
            .map_err(|e| KeyringError::Backend(e.to_string()))
    }

    fn get(&self, role: &str) -> Result<Option<KeyringEntry>, KeyringError> {
        let entry = self.entry_for(role)?;
        match entry.get_password() {
            Ok(blob) => Ok(Some(parse_credentials(&blob)?)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeyringError::Backend(e.to_string())),
        }
    }

    fn delete(&mut self, role: &str) -> Result<(), KeyringError> {
        let entry = self.entry_for(role)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeyringError::Backend(e.to_string())),
        }
    }
}
