use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use crate::config::ConfigFile;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Accounts,
    Server,
    Logging,
    Daemon,
    Review,
}

impl Screen {
    pub const ALL: [Screen; 5] = [
        Screen::Accounts, Screen::Server, Screen::Logging, Screen::Daemon, Screen::Review,
    ];

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[(idx + 1).min(Self::ALL.len() - 1)]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|s| *s == self).unwrap_or(0);
        Self::ALL[idx.saturating_sub(1)]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldId {
    MainEmail,
    MainPassword,
    MonitorEmail,
    MonitorPassword,
    ServerBind,
    ServerPort,
    ServerHttps,
    LogLevel,
    LogFile,
    PidFile,
}

impl FieldId {
    fn for_screen(screen: Screen) -> &'static [FieldId] {
        match screen {
            Screen::Accounts => &[
                FieldId::MainEmail, FieldId::MainPassword,
                FieldId::MonitorEmail, FieldId::MonitorPassword,
            ],
            Screen::Server  => &[FieldId::ServerBind, FieldId::ServerPort, FieldId::ServerHttps],
            Screen::Logging => &[FieldId::LogLevel, FieldId::LogFile],
            Screen::Daemon  => &[FieldId::PidFile],
            Screen::Review  => &[],
        }
    }

    pub fn is_password(self) -> bool {
        matches!(self, FieldId::MainPassword | FieldId::MonitorPassword)
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, FieldId::ServerPort)
    }

    pub fn is_boolean(self) -> bool {
        matches!(self, FieldId::ServerHttps)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Editing,
    ConfirmDiscard,
    Help,
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Save,
    Cancel,
    DiscardConfirmed,
}

#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub errors: Vec<(FieldId, String)>,
}

impl ValidationReport {
    pub fn is_valid(&self) -> bool { self.errors.is_empty() }

    pub fn error_for(&self, field: FieldId) -> Option<&str> {
        self.errors.iter()
            .find(|(f, _)| *f == field)
            .map(|(_, msg)| msg.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct Fields {
    pub main_email: String,
    pub main_password: String,       // draft only; never serialized
    pub monitor_email: String,
    pub monitor_password: String,    // draft only; never serialized
    pub server_bind: String,
    pub server_port: String,         // kept as string for editing
    pub server_https: bool,
    pub log_level: String,
    pub log_file: String,
    pub pid_file: String,
}

impl Fields {
    pub fn from_config(cfg: &ConfigFile) -> Self {
        Self {
            main_email:       cfg.accounts.main.email.clone().unwrap_or_default(),
            main_password:    String::new(),
            monitor_email:    cfg.accounts.monitor.email.clone().unwrap_or_default(),
            monitor_password: String::new(),
            server_bind:      cfg.server.bind.clone(),
            server_port:      cfg.server.port.to_string(),
            server_https:     cfg.server.https,
            log_level:        cfg.logging.level.to_string(),
            log_file:         cfg.logging.file.clone(),
            pid_file:         cfg.daemon.pidfile.clone(),
        }
    }

    pub fn get_mut(&mut self, field: FieldId) -> Option<&mut String> {
        match field {
            FieldId::MainEmail       => Some(&mut self.main_email),
            FieldId::MainPassword    => Some(&mut self.main_password),
            FieldId::MonitorEmail    => Some(&mut self.monitor_email),
            FieldId::MonitorPassword => Some(&mut self.monitor_password),
            FieldId::ServerBind      => Some(&mut self.server_bind),
            FieldId::ServerPort      => Some(&mut self.server_port),
            FieldId::LogLevel        => Some(&mut self.log_level),
            FieldId::LogFile         => Some(&mut self.log_file),
            FieldId::PidFile         => Some(&mut self.pid_file),
            FieldId::ServerHttps     => None,  // boolean, toggled not edited
        }
    }

    pub fn get(&self, field: FieldId) -> &str {
        match field {
            FieldId::MainEmail       => &self.main_email,
            FieldId::MainPassword    => &self.main_password,
            FieldId::MonitorEmail    => &self.monitor_email,
            FieldId::MonitorPassword => &self.monitor_password,
            FieldId::ServerBind      => &self.server_bind,
            FieldId::ServerPort      => &self.server_port,
            FieldId::LogLevel        => &self.log_level,
            FieldId::LogFile         => &self.log_file,
            FieldId::PidFile         => &self.pid_file,
            FieldId::ServerHttps     => if self.server_https { "true" } else { "false" },
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatusBar {
    pub message: String,
    pub is_error: bool,
}

impl StatusBar {
    pub fn info(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), is_error: false }
    }
    pub fn error(msg: impl Into<String>) -> Self {
        Self { message: msg.into(), is_error: true }
    }
    pub fn clear() -> Self { Self { message: String::new(), is_error: false } }
}

#[derive(Debug)]
pub struct Model {
    pub current_screen: Screen,
    pub focus: FieldId,
    pub fields: Fields,
    pub validation: ValidationReport,
    pub status: StatusBar,
    pub dirty: bool,
    pub mode: Mode,
    /// Snapshot of `fields` at model creation, for cancel-revert logic.
    pub initial_fields: Fields,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl Model {
    pub fn new(cfg: ConfigFile) -> Self {
        let fields = Fields::from_config(&cfg);
        let initial_fields = fields.clone();
        let mut m = Self {
            current_screen: Screen::Accounts,
            focus: FieldId::MainEmail,
            fields,
            validation: ValidationReport::default(),
            status: StatusBar::info("Tab/Shift-Tab: move  Enter: edit  Ctrl-→/←: screen  Ctrl-S: save  ?: help"),
            dirty: false,
            mode: Mode::Normal,
            initial_fields,
        };
        m.validate();
        m
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

impl Model {
    fn validate(&mut self) {
        let mut errors = Vec::new();

        for field in [FieldId::MainEmail, FieldId::MonitorEmail] {
            let v = self.fields.get(field);
            if !v.is_empty() && !looks_like_email(v) {
                errors.push((field, "must be a valid email address".to_string()));
            }
        }

        let port_str = self.fields.get(FieldId::ServerPort);
        match port_str.parse::<u32>() {
            Ok(0) | Err(_) => errors.push((
                FieldId::ServerPort,
                "port must be a number 1-65535".to_string(),
            )),
            Ok(n) if n > 65535 => errors.push((
                FieldId::ServerPort,
                "port must be a number 1-65535".to_string(),
            )),
            _ => {}
        }

        self.validation = ValidationReport { errors };
    }
}

fn looks_like_email(s: &str) -> bool {
    let parts: Vec<&str> = s.splitn(2, '@').collect();
    matches!(parts.as_slice(), [local, domain] if !local.is_empty() && domain.contains('.'))
}

// ---------------------------------------------------------------------------
// Focus navigation helpers
// ---------------------------------------------------------------------------

impl Model {
    fn fields_on_current_screen(&self) -> &'static [FieldId] {
        FieldId::for_screen(self.current_screen)
    }

    fn advance_focus(&mut self, forward: bool) {
        let fields = self.fields_on_current_screen();
        if fields.is_empty() { return; }
        let idx = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        let next = if forward {
            (idx + 1) % fields.len()
        } else {
            (idx + fields.len() - 1) % fields.len()
        };
        self.focus = fields[next];
    }

    fn first_field_on_screen(&self) -> Option<FieldId> {
        FieldId::for_screen(self.current_screen).first().copied()
    }

    fn go_to_screen(&mut self, screen: Screen) {
        self.current_screen = screen;
        if let Some(f) = self.first_field_on_screen() {
            self.focus = f;
        }
    }
}

// ---------------------------------------------------------------------------
// update — pure event handler
// ---------------------------------------------------------------------------

impl Model {
    pub fn update(&mut self, ev: Event) -> Action {
        match &self.mode {
            Mode::ConfirmDiscard => self.handle_confirm_discard(ev),
            Mode::Help => self.handle_help(ev),
            Mode::Editing => self.handle_editing(ev),
            Mode::Normal => self.handle_normal(ev),
        }
    }

    fn handle_normal(&mut self, ev: Event) -> Action {
        let Event::Key(key) = ev else { return Action::None; };
        match (key.modifiers, key.code) {
            // Navigation
            (KeyModifiers::NONE, KeyCode::Tab) => {
                self.advance_focus(true);
                Action::None
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.advance_focus(false);
                Action::None
            }
            (KeyModifiers::CONTROL, KeyCode::Right) => {
                let next = self.current_screen.next();
                self.go_to_screen(next);
                Action::None
            }
            (KeyModifiers::CONTROL, KeyCode::Left) => {
                let prev = self.current_screen.prev();
                self.go_to_screen(prev);
                Action::None
            }
            // Editing
            (KeyModifiers::NONE, KeyCode::Enter) => {
                // Boolean fields toggle on Enter; text fields enter editing mode
                if self.focus.is_boolean() {
                    self.fields.server_https = !self.fields.server_https;
                    self.dirty = true;
                    self.validate();
                } else {
                    self.mode = Mode::Editing;
                    self.status = StatusBar::info("Editing — Enter: confirm  Esc: cancel");
                }
                Action::None
            }
            // Save
            (KeyModifiers::CONTROL, KeyCode::Char('s')) => {
                if !self.validation.is_valid() {
                    self.status = StatusBar::error("Fix validation errors before saving");
                    return Action::None;
                }
                Action::Save
            }
            // Cancel / quit
            (KeyModifiers::NONE, KeyCode::Esc)
            | (KeyModifiers::CONTROL, KeyCode::Char('c'))
            | (KeyModifiers::NONE, KeyCode::Char('q')) => {
                if self.dirty {
                    self.mode = Mode::ConfirmDiscard;
                    self.status = StatusBar::error("Unsaved changes. Press y to discard, n to go back");
                    Action::None
                } else {
                    Action::Cancel
                }
            }
            // Help
            (KeyModifiers::NONE, KeyCode::Char('?')) => {
                self.mode = Mode::Help;
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_editing(&mut self, ev: Event) -> Action {
        let Event::Key(key) = ev else { return Action::None; };
        match key.code {
            KeyCode::Enter | KeyCode::Tab => {
                self.mode = Mode::Normal;
                self.status = StatusBar::info("Tab/Shift-Tab: move  Enter: edit  Ctrl-→/←: screen  Ctrl-S: save  ?: help");
                self.validate();
            }
            KeyCode::Esc => {
                // revert field to its initial value
                let init_val = match self.focus {
                    FieldId::MainEmail       => self.initial_fields.main_email.clone(),
                    FieldId::MainPassword    => String::new(),
                    FieldId::MonitorEmail    => self.initial_fields.monitor_email.clone(),
                    FieldId::MonitorPassword => String::new(),
                    FieldId::ServerBind      => self.initial_fields.server_bind.clone(),
                    FieldId::ServerPort      => self.initial_fields.server_port.clone(),
                    FieldId::LogLevel        => self.initial_fields.log_level.clone(),
                    FieldId::LogFile         => self.initial_fields.log_file.clone(),
                    FieldId::PidFile         => self.initial_fields.pid_file.clone(),
                    FieldId::ServerHttps     => String::new(),
                };
                if let Some(field) = self.fields.get_mut(self.focus) {
                    *field = init_val;
                }
                self.mode = Mode::Normal;
                self.validate();
            }
            KeyCode::Backspace => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    field.pop();
                    self.dirty = true;
                    self.validate();
                }
            }
            KeyCode::Char(c) => {
                // Reject non-digit input for numeric fields
                if self.focus.is_numeric() && !c.is_ascii_digit() {
                    return Action::None;
                }
                if let Some(field) = self.fields.get_mut(self.focus) {
                    field.push(c);
                    self.dirty = true;
                    self.validate();
                }
            }
            _ => {}
        }
        Action::None
    }

    fn handle_confirm_discard(&mut self, ev: Event) -> Action {
        let Event::Key(key) = ev else { return Action::None; };
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => Action::DiscardConfirmed,
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.status = StatusBar::info("Tab/Shift-Tab: move  Enter: edit  Ctrl-→/←: screen  Ctrl-S: save  ?: help");
                Action::None
            }
            _ => Action::None,
        }
    }

    fn handle_help(&mut self, ev: Event) -> Action {
        let Event::Key(KeyEvent { code: KeyCode::Char('?') | KeyCode::Esc, .. }) = ev else {
            return Action::None;
        };
        self.mode = Mode::Normal;
        self.status = StatusBar::clear();
        Action::None
    }

    /// Build a `ConfigFile` from the current draft fields (no passwords).
    pub fn to_config_file(&self) -> ConfigFile {
        use crate::config::{AccountEntry, AccountsConfig, DaemonConfig, LoggingConfig, LogLevel, ServerConfig};
        let port = self.fields.server_port.parse::<u32>().unwrap_or(1080);
        let log_level = match self.fields.log_level.as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "warn"  => LogLevel::Warn,
            "error" => LogLevel::Error,
            _       => LogLevel::Info,
        };
        ConfigFile {
            schema_version: crate::config::CURRENT_SCHEMA_VERSION,
            accounts: AccountsConfig {
                main:    AccountEntry { email: Some(self.fields.main_email.clone()).filter(|s| !s.is_empty()) },
                monitor: AccountEntry { email: Some(self.fields.monitor_email.clone()).filter(|s| !s.is_empty()) },
            },
            server: ServerConfig {
                bind:  self.fields.server_bind.clone(),
                port,
                https: self.fields.server_https,
            },
            logging: LoggingConfig {
                level: log_level,
                file:  self.fields.log_file.clone(),
            },
            daemon: DaemonConfig {
                pidfile: self.fields.pid_file.clone(),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ConfigFile;

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }
    fn ctrl(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::CONTROL))
    }
    fn shift_backtab() -> Event {
        Event::Key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT))
    }

    fn model_with_valid_config() -> Model {
        let mut cfg = ConfigFile::default();
        cfg.accounts.main.email = Some("rider@example.com".to_string());
        cfg.accounts.monitor.email = Some("monitor@example.com".to_string());
        Model::new(cfg)
    }

    #[test]
    fn model_initial_state_reflects_loaded_config() {
        let m = model_with_valid_config();
        assert_eq!(m.fields.main_email, "rider@example.com");
        assert_eq!(m.fields.monitor_email, "monitor@example.com");
        assert_eq!(m.current_screen, Screen::Accounts);
        assert_eq!(m.focus, FieldId::MainEmail);
        assert!(!m.dirty);
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn tab_advances_focus_within_screen() {
        let mut m = model_with_valid_config();
        assert_eq!(m.focus, FieldId::MainEmail);
        m.update(key(KeyCode::Tab));
        assert_eq!(m.focus, FieldId::MainPassword);
        m.update(key(KeyCode::Tab));
        assert_eq!(m.focus, FieldId::MonitorEmail);
        m.update(key(KeyCode::Tab));
        assert_eq!(m.focus, FieldId::MonitorPassword);
        // wraps around
        m.update(key(KeyCode::Tab));
        assert_eq!(m.focus, FieldId::MainEmail);
    }

    #[test]
    fn shift_tab_moves_focus_backward() {
        let mut m = model_with_valid_config();
        assert_eq!(m.focus, FieldId::MainEmail);
        m.update(shift_backtab());
        // wraps to last field on screen
        assert_eq!(m.focus, FieldId::MonitorPassword);
    }

    #[test]
    fn ctrl_right_advances_screen() {
        let mut m = model_with_valid_config();
        assert_eq!(m.current_screen, Screen::Accounts);
        m.update(ctrl(KeyCode::Right));
        assert_eq!(m.current_screen, Screen::Server);
        m.update(ctrl(KeyCode::Right));
        assert_eq!(m.current_screen, Screen::Logging);
    }

    #[test]
    fn ctrl_left_moves_screen_back() {
        let mut m = model_with_valid_config();
        m.update(ctrl(KeyCode::Right)); // → Server
        m.update(ctrl(KeyCode::Left));  // ← Accounts
        assert_eq!(m.current_screen, Screen::Accounts);
    }

    #[test]
    fn screen_change_resets_focus_to_first_field() {
        let mut m = model_with_valid_config();
        m.update(ctrl(KeyCode::Right)); // → Server
        assert_eq!(m.focus, FieldId::ServerBind);
    }

    #[test]
    fn enter_in_normal_mode_starts_editing() {
        let mut m = model_with_valid_config();
        m.update(key(KeyCode::Enter));
        assert_eq!(m.mode, Mode::Editing);
    }

    #[test]
    fn editing_mode_captures_typed_text_into_focused_field() {
        let mut m = model_with_valid_config();
        m.update(key(KeyCode::Enter));
        for c in "new@email.com".chars() {
            // clear first char
            if m.fields.main_email == "rider@example.com" {
                // wipe existing with backspaces
                let len = m.fields.main_email.len();
                for _ in 0..len {
                    m.update(key(KeyCode::Backspace));
                }
            }
            m.update(key(KeyCode::Char(c)));
        }
        assert_eq!(m.fields.main_email, "new@email.com");
        assert!(m.dirty);
    }

    #[test]
    fn enter_in_editing_mode_commits_and_returns_to_normal() {
        let mut m = model_with_valid_config();
        m.update(key(KeyCode::Enter)); // enter editing
        m.update(key(KeyCode::Enter)); // commit
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn escape_in_editing_mode_reverts_field() {
        let mut m = model_with_valid_config();
        let original = m.fields.main_email.clone();
        m.update(key(KeyCode::Enter)); // editing
        m.update(key(KeyCode::Backspace));
        m.update(key(KeyCode::Backspace));
        m.update(key(KeyCode::Esc)); // revert
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.fields.main_email, original);
    }

    #[test]
    fn numeric_field_rejects_non_digit_input() {
        let mut m = model_with_valid_config();
        m.update(ctrl(KeyCode::Right)); // → Server
        m.update(key(KeyCode::Tab)); // ServerBind → ServerPort
        m.update(key(KeyCode::Enter)); // edit
        let before = m.fields.server_port.clone();
        m.update(key(KeyCode::Char('x')));
        assert_eq!(m.fields.server_port, before, "non-digit should be ignored");
        m.update(key(KeyCode::Char('9')));
        assert!(m.fields.server_port.ends_with('9'));
    }

    #[test]
    fn email_field_validation_runs_on_every_update() {
        let mut m = model_with_valid_config();
        m.update(key(KeyCode::Enter)); // edit MainEmail
        // Clear field
        let len = m.fields.main_email.len();
        for _ in 0..len {
            m.update(key(KeyCode::Backspace));
        }
        // Type invalid email
        for c in "notanemail".chars() {
            m.update(key(KeyCode::Char(c)));
        }
        assert!(m.validation.error_for(FieldId::MainEmail).is_some(),
            "invalid email should produce a validation error");
        // Fix it
        m.update(key(KeyCode::Char('@')));
        for c in "b.com".chars() {
            m.update(key(KeyCode::Char(c)));
        }
        assert!(m.validation.error_for(FieldId::MainEmail).is_none(),
            "valid email should clear the validation error");
    }

    #[test]
    fn password_field_not_present_in_serialized_config_file() {
        let mut m = model_with_valid_config();
        m.update(key(KeyCode::Tab)); // focus MainPassword
        m.update(key(KeyCode::Enter));
        for c in "hunter2".chars() {
            m.update(key(KeyCode::Char(c)));
        }
        m.update(key(KeyCode::Enter));
        let cfg = m.to_config_file();
        // Serialize to TOML — password must not appear
        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        assert!(!toml_str.contains("password"), "TOML must not contain password");
        assert!(!toml_str.contains("hunter2"), "TOML must not contain password value");
    }

    #[test]
    fn save_action_requires_no_validation_errors() {
        let mut m = model_with_valid_config();
        // Corrupt port
        m.update(ctrl(KeyCode::Right)); // → Server
        m.update(key(KeyCode::Tab)); // → ServerPort
        m.update(key(KeyCode::Enter));
        let len = m.fields.server_port.len();
        for _ in 0..len { m.update(key(KeyCode::Backspace)); }
        for c in "0".chars() { m.update(key(KeyCode::Char(c))); }
        m.update(key(KeyCode::Enter));
        // port=0 is invalid; save should produce None, not Save
        let action = m.update(ctrl(KeyCode::Char('s')));
        assert!(matches!(action, Action::None));
        assert!(m.status.is_error, "status should report error");
    }

    #[test]
    fn save_action_returned_when_valid() {
        let mut m = model_with_valid_config();
        // force dirty so we know it was actually tested
        m.dirty = true;
        let action = m.update(ctrl(KeyCode::Char('s')));
        assert!(matches!(action, Action::Save));
    }

    #[test]
    fn cancel_when_clean_returns_cancel() {
        let m = model_with_valid_config();
        // not dirty
        let mut m = m;
        let action = m.update(key(KeyCode::Esc));
        assert!(matches!(action, Action::Cancel));
    }

    #[test]
    fn cancel_when_dirty_enters_confirm_discard() {
        let mut m = model_with_valid_config();
        m.dirty = true;
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::ConfirmDiscard);
    }

    #[test]
    fn confirm_discard_y_returns_discard_confirmed() {
        let mut m = model_with_valid_config();
        m.dirty = true;
        m.update(key(KeyCode::Esc));
        let action = m.update(key(KeyCode::Char('y')));
        assert!(matches!(action, Action::DiscardConfirmed));
    }

    #[test]
    fn confirm_discard_n_returns_to_normal() {
        let mut m = model_with_valid_config();
        m.dirty = true;
        m.update(key(KeyCode::Esc));
        m.update(key(KeyCode::Char('n')));
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn help_key_toggles_help_overlay() {
        let mut m = model_with_valid_config();
        assert_ne!(m.mode, Mode::Help);
        m.update(key(KeyCode::Char('?')));
        assert_eq!(m.mode, Mode::Help);
        m.update(key(KeyCode::Char('?')));
        assert_eq!(m.mode, Mode::Normal);
    }
}
