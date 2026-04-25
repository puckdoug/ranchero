use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use ranchero::config::{ConfigError, ConfigFile};
use ranchero::config::store::ConfigStore;
use ranchero::tui::keyring::{InMemoryKeyringStore, KeyringStore};
use ranchero::tui::model::{Action, Model};

// ---------------------------------------------------------------------------
// Test double — FakeConfigStore
// ---------------------------------------------------------------------------

struct FakeConfigStore {
    initial: Option<ConfigFile>,
    pub saved: Option<ConfigFile>,
}

impl FakeConfigStore {
    fn new(initial: Option<ConfigFile>) -> Self {
        Self { initial, saved: None }
    }
}

impl ConfigStore for FakeConfigStore {
    fn load(&self) -> Result<Option<ConfigFile>, ConfigError> {
        Ok(self.initial.clone())
    }
    fn save(&mut self, cfg: &ConfigFile) -> Result<(), ConfigError> {
        self.saved = Some(cfg.clone());
        Ok(())
    }
    fn path(&self) -> &std::path::Path {
        std::path::Path::new("/fake/ranchero.toml")
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn key(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}
fn ctrl(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
}

fn drive_to_save(model: &mut Model) -> Action {
    model.dirty = true;
    model.update(ctrl(KeyCode::Char('s')))
}

fn drive_to_cancel(model: &mut Model) -> Action {
    model.update(key(KeyCode::Esc))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn drive_save_returns_save_action_and_store_receives_config() {
    let initial = {
        let mut c = ConfigFile::default();
        c.accounts.main.email = Some("r@test.com".to_string());
        c
    };
    let mut store = FakeConfigStore::new(Some(initial.clone()));
    let mut model = Model::new(initial);

    let action = drive_to_save(&mut model);
    assert!(matches!(action, Action::Save), "expected Save action");

    // Simulate what the driver does on Action::Save
    let cfg = model.to_config_file();
    store.save(&cfg).unwrap();
    assert!(store.saved.is_some(), "store should have received a save call");

    let saved = store.saved.as_ref().unwrap();
    assert_eq!(saved.accounts.main.email.as_deref(), Some("r@test.com"));

    // Serialized TOML must not contain any password key
    let toml_str = toml::to_string_pretty(saved).unwrap();
    assert!(!toml_str.contains("password"), "saved TOML must not contain password key");
}

#[test]
fn drive_save_stores_passwords_in_keyring_only() {
    let cfg = {
        let mut c = ConfigFile::default();
        c.accounts.main.email = Some("r@test.com".to_string());
        c
    };
    let mut model = Model::new(cfg);

    // Tab to MainPassword, enter edit mode, type a password, commit
    model.update(key(KeyCode::Tab));
    model.update(key(KeyCode::Enter));
    for c in "hunter2".chars() {
        model.update(key(KeyCode::Char(c)));
    }
    model.update(key(KeyCode::Enter));

    let action = drive_to_save(&mut model);
    assert!(matches!(action, Action::Save));

    // save_passwords is the public helper the driver calls
    let mut keyring = InMemoryKeyringStore::default();
    ranchero::tui::driver::save_passwords(&model, &mut keyring);

    let entry = keyring.get("main").expect("keyring should have main entry");
    assert_eq!(entry.password, "hunter2");

    // TOML serialization must not contain the password value
    let saved_cfg = model.to_config_file();
    let toml_str = toml::to_string_pretty(&saved_cfg).unwrap();
    assert!(!toml_str.contains("hunter2"), "password must not appear in TOML: {toml_str}");
}

#[test]
fn cancel_when_clean_produces_no_writes() {
    let store = FakeConfigStore::new(None);
    let mut model = Model::new(ConfigFile::default());
    let action = drive_to_cancel(&mut model);
    assert!(matches!(action, Action::Cancel));
    assert!(store.saved.is_none(), "cancel should not write to store");
}

#[test]
fn missing_config_starts_with_defaults() {
    let store = FakeConfigStore::new(None);
    let initial = store.load().unwrap();
    assert!(initial.is_none());
    let model = Model::new(initial.unwrap_or_default());
    assert_eq!(model.fields.server_port, "1080");
    assert_eq!(model.fields.server_bind, "127.0.0.1");
}
