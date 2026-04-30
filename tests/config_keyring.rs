//! Red-state tests for STEP-12.6 Defect 2: the password resolution path
//! must consult the OS keychain when no CLI flag is supplied.
//!
//! These tests target the new four-argument signature
//! `ResolvedConfig::resolve(&cli, &env, &keyring, file)` and the new
//! `ConfigError::KeyringError` variant. Until those changes are
//! implemented, this file fails to compile — that is the red state.
//!
//! Once the implementation is in place every test here must pass.

use ranchero::cli::GlobalOpts;
use ranchero::config::{ConfigError, Env, ResolvedConfig};
use ranchero::credentials::{InMemoryKeyringStore, KeyringEntry, KeyringError, KeyringStore};

struct EmptyEnv;
impl Env for EmptyEnv {
    fn get(&self, _: &str) -> Option<String> { None }
}

fn empty_cli() -> GlobalOpts { GlobalOpts::default() }

/// Stub keyring whose `get` always returns `Err(KeyringError::Backend)`.
/// Used to verify that backend failures bubble up through `resolve`
/// rather than being silently mapped to "credential absent".
#[derive(Default)]
struct FailingKeyringStore;

impl KeyringStore for FailingKeyringStore {
    fn set(&mut self, _: &str, _: &str, _: &str) -> Result<(), KeyringError> {
        Err(KeyringError::Backend("injected".into()))
    }
    fn get(&self, _: &str) -> Result<Option<KeyringEntry>, KeyringError> {
        Err(KeyringError::Backend("injected".into()))
    }
    fn delete(&mut self, _: &str) -> Result<(), KeyringError> {
        Err(KeyringError::Backend("injected".into()))
    }
}

#[test]
fn resolve_consults_keyring_for_absent_main_password() {
    let mut keyring = InMemoryKeyringStore::default();
    keyring.set("main", "rider@example.com", "keyring-secret").unwrap();
    let cli = empty_cli();
    let r = ResolvedConfig::resolve(&cli, &EmptyEnv, &keyring, None).unwrap();
    assert_eq!(
        r.main_password.expect("password should be sourced from keyring").expose(),
        "keyring-secret",
    );
}

#[test]
fn resolve_cli_main_password_takes_precedence_over_keyring() {
    let mut keyring = InMemoryKeyringStore::default();
    keyring.set("main", "rider@example.com", "keyring-secret").unwrap();
    let mut cli = empty_cli();
    cli.mainpassword = Some("cli-secret".to_string());
    let r = ResolvedConfig::resolve(&cli, &EmptyEnv, &keyring, None).unwrap();
    assert_eq!(
        r.main_password.expect("password should be present").expose(),
        "cli-secret",
        "CLI flag must override keyring entry",
    );
}

#[test]
fn resolve_propagates_keyring_backend_error() {
    let keyring = FailingKeyringStore;
    let cli = empty_cli();
    let err = ResolvedConfig::resolve(&cli, &EmptyEnv, &keyring, None)
        .expect_err("should surface the keyring backend failure");
    assert!(
        matches!(err, ConfigError::KeyringError(_)),
        "expected ConfigError::KeyringError, got: {err:?}",
    );
}

#[test]
fn resolve_absent_keyring_entry_yields_none_password() {
    let keyring = InMemoryKeyringStore::default();
    let cli = empty_cli();
    let r = ResolvedConfig::resolve(&cli, &EmptyEnv, &keyring, None).unwrap();
    assert!(
        r.main_password.is_none(),
        "no CLI flag and no keyring entry should leave main_password unset",
    );
    assert!(
        r.monitor_password.is_none(),
        "no CLI flag and no keyring entry should leave monitor_password unset",
    );
}

