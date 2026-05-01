use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use ranchero::config::{ConfigError, ConfigFile};
use ranchero::config::store::ConfigStore;
use ranchero::credentials::{InMemoryKeyringStore, KeyringStore};
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

    // Down to MainPassword, enter edit mode, type a password, commit
    model.update(key(KeyCode::Down));
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

    let entry = keyring.get("main").unwrap().expect("keyring should have main entry");
    assert_eq!(entry.password, "hunter2");

    // TOML serialization must not contain the password value
    let saved_cfg = model.to_config_file();
    let toml_str = toml::to_string_pretty(&saved_cfg).unwrap();
    assert!(!toml_str.contains("hunter2"), "password must not appear in TOML: {toml_str}");
}

#[test]
fn load_passwords_pre_populates_password_fields_from_keyring() {
    use ranchero::tui::model::FieldId;
    // Simulate a second `ranchero configure` run: config file has the
    // emails, and the keyring already holds the passwords from the
    // previous save. The TUI must show those passwords on startup.
    let cfg = {
        let mut c = ConfigFile::default();
        c.accounts.main.email    = Some("r@test.com".to_string());
        c.accounts.monitor.email = Some("mon@test.com".to_string());
        c
    };
    let mut model = Model::new(cfg);

    let mut keyring = InMemoryKeyringStore::default();
    keyring.set("main",    "r@test.com",   "main-pw").unwrap();
    keyring.set("monitor", "mon@test.com", "monitor-pw").unwrap();

    ranchero::tui::driver::load_passwords(&mut model, &keyring);

    assert_eq!(model.fields.text(FieldId::MainPassword),    "main-pw");
    assert_eq!(model.fields.text(FieldId::MonitorPassword), "monitor-pw");
}

#[test]
fn load_passwords_with_empty_keyring_leaves_fields_blank() {
    use ranchero::tui::model::FieldId;
    let cfg = ConfigFile::default();
    let mut model = Model::new(cfg);

    let keyring = InMemoryKeyringStore::default();
    ranchero::tui::driver::load_passwords(&mut model, &keyring);

    assert_eq!(model.fields.text(FieldId::MainPassword),    "");
    assert_eq!(model.fields.text(FieldId::MonitorPassword), "");
}

#[test]
fn load_passwords_does_not_mark_model_dirty() {
    // Loading passwords from the keyring is initialisation, not a user
    // edit; the model must not start out dirty (otherwise plain `:q`
    // would trigger the discard-confirm flow on every launch).
    let cfg = ConfigFile::default();
    let mut model = Model::new(cfg);
    assert!(!model.dirty, "freshly constructed model should be clean");

    let mut keyring = InMemoryKeyringStore::default();
    keyring.set("main", "u@x", "pw").unwrap();
    ranchero::tui::driver::load_passwords(&mut model, &keyring);

    assert!(!model.dirty, "load_passwords must not dirty the model");
}

#[test]
fn load_passwords_makes_esc_revert_to_loaded_password() {
    // After load_passwords seeds a password, entering edit mode and
    // pressing Esc must revert to the loaded value, not to "".
    use ranchero::config::EditingModeConfig;
    use ranchero::tui::model::FieldId;
    let mut cfg = ConfigFile::default();
    cfg.tui.editing_mode = EditingModeConfig::Emacs; // Esc means revert
    let mut model = Model::new(cfg);

    let mut keyring = InMemoryKeyringStore::default();
    keyring.set("main", "u@x", "loaded-pw").unwrap();
    ranchero::tui::driver::load_passwords(&mut model, &keyring);

    // Focus MainPassword (Down once from MainEmail) and enter edit mode.
    model.update(key(KeyCode::Down));
    assert_eq!(model.focus, FieldId::MainPassword);
    model.update(key(KeyCode::Enter));
    // Type extra characters then Esc to revert.
    for c in "junk".chars() {
        model.update(key(KeyCode::Char(c)));
    }
    model.update(key(KeyCode::Esc));

    assert_eq!(
        model.fields.text(FieldId::MainPassword), "loaded-pw",
        "Esc should revert to the loaded password, not to blank",
    );
}

#[test]
fn cancel_when_clean_produces_no_writes() {
    use ranchero::config::EditingModeConfig;
    // Explicitly use emacs mode — vi treats Esc as a no-op (safe), so the
    // Esc → Cancel contract only applies to default/emacs mode.
    let store = FakeConfigStore::new(None);
    let mut cfg = ConfigFile::default();
    cfg.tui.editing_mode = EditingModeConfig::Emacs;
    let mut model = Model::new(cfg);
    let action = drive_to_cancel(&mut model);
    assert!(matches!(action, Action::Cancel));
    assert!(store.saved.is_none(), "cancel should not write to store");
}

#[test]
fn missing_config_starts_with_defaults() {
    let store = FakeConfigStore::new(None);
    let initial = store.load().unwrap();
    assert!(initial.is_none());
    use ranchero::tui::model::FieldId;
    let model = Model::new(initial.unwrap_or_default());
    assert_eq!(model.fields.text(FieldId::ServerPort), "1080");
    assert_eq!(model.fields.text(FieldId::ServerBind), "127.0.0.1");
}

// ---------------------------------------------------------------------------
// Vi navigation integration tests (STEP-02.2)
// ---------------------------------------------------------------------------

fn vi_model() -> Model {
    use ranchero::config::EditingModeConfig;
    let mut cfg = ConfigFile::default();
    cfg.accounts.main.email = Some("r@test.com".to_string());
    cfg.tui.editing_mode = EditingModeConfig::Vi;
    Model::new(cfg)
}

#[test]
fn vi_save_via_colon_wq() {
    let mut store = FakeConfigStore::new(None);
    let mut model = vi_model();

    // :wq sequence
    for c in ":wq".chars() {
        model.update(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    let action = model.update(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(matches!(action, Action::Save), "expected Save from :wq");

    // Simulate driver save
    let cfg = model.to_config_file();
    store.save(&cfg).unwrap();
    assert!(store.saved.is_some(), ":wq should have triggered a store write");
}

#[test]
fn vi_force_quit_via_colon_q_bang() {
    let store = FakeConfigStore::new(None);
    let mut model = vi_model();
    model.dirty = true;

    for c in ":q!".chars() {
        model.update(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
    }
    let action = model.update(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(matches!(action, Action::DiscardConfirmed), "expected DiscardConfirmed from :q!");
    assert!(store.saved.is_none(), ":q! should not write to store");
}

#[test]
fn vi_write_only_clears_dirty_and_stays_open() {
    let mut store = FakeConfigStore::new(None);
    let mut model = vi_model();
    model.dirty = true;

    model.update(Event::Key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::NONE)));
    model.update(Event::Key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)));
    let action = model.update(Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    assert!(matches!(action, Action::WriteOnly), "expected WriteOnly from :w");
    assert!(!model.dirty, ":w should clear dirty flag");

    // Simulate driver handling WriteOnly
    let cfg = model.to_config_file();
    store.save(&cfg).unwrap();
    assert!(store.saved.is_some(), ":w should write to store");
    // Model is still open — mode should be Normal (not closed)
    use ranchero::tui::model::Mode;
    assert_eq!(model.mode, Mode::Normal, "model should remain in Normal mode after :w");
}

#[test]
fn vi_insert_indicator_disappears_on_esc_to_normal() {
    use ranchero::tui::model::{Mode, status_bar_content, FieldId};
    use ranchero::config::EditingMode;

    let mut model = vi_model();

    // Before editing: Normal mode, no INSERT
    let content = status_bar_content(&model.mode, None, EditingMode::Vi);
    assert!(!content.contains("INSERT"), "should not show INSERT in Normal mode");

    // Press 'i' → Mode::Editing, EditorMode::Insert → -- INSERT --
    model.update(Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE)));
    assert_eq!(model.mode, Mode::Editing);
    let editor_mode = model.fields.get_editor(FieldId::MainEmail).map(|e| e.mode);
    let content = status_bar_content(&model.mode, editor_mode, EditingMode::Vi);
    assert_eq!(content, "-- INSERT --", "should show -- INSERT -- when in Insert mode");

    // Esc → EditorMode::Normal (stay in Mode::Editing) → blank
    model.update(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    let editor_mode = model.fields.get_editor(FieldId::MainEmail).map(|e| e.mode);
    let content = status_bar_content(&model.mode, editor_mode, EditingMode::Vi);
    assert!(content.is_empty(), "-- INSERT -- should disappear when in vi Normal sub-mode");

    // Second Esc → exit Mode::Editing
    model.update(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    assert_eq!(model.mode, Mode::Normal);
    let content = status_bar_content(&model.mode, None, EditingMode::Vi);
    assert!(!content.contains("INSERT"), "should not show INSERT after exiting editing");
}
