use std::collections::HashMap;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use edtui::{EditorEventHandler, EditorMode, EditorState, Lines};

use crate::config::{ConfigFile, EditingMode};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

pub struct Fields {
    pub main_email:       EditorState,
    pub main_password:    EditorState,   // rendered as *** in view; never serialized
    pub monitor_email:    EditorState,
    pub monitor_password: EditorState,   // rendered as *** in view; never serialized
    pub server_bind:      EditorState,
    pub server_port:      EditorState,   // only digits accepted
    pub server_https:     bool,          // toggled, not text-edited
    pub log_level:        EditorState,
    pub log_file:         EditorState,
    pub pid_file:         EditorState,
}

fn make_editor(text: &str) -> EditorState {
    let mut s = EditorState::new(Lines::from(text));
    s.set_single_line(true);
    s
}

fn editor_text(state: &EditorState) -> String {
    state.lines.flatten(&Some('\n')).into_iter().collect()
}

impl Fields {
    pub fn from_config(cfg: &ConfigFile) -> Self {
        Self {
            main_email:       make_editor(&cfg.accounts.main.email.clone().unwrap_or_default()),
            main_password:    make_editor(""),
            monitor_email:    make_editor(&cfg.accounts.monitor.email.clone().unwrap_or_default()),
            monitor_password: make_editor(""),
            server_bind:      make_editor(&cfg.server.bind),
            server_port:      make_editor(&cfg.server.port.to_string()),
            server_https:     cfg.server.https,
            log_level:        make_editor(&cfg.logging.level.to_string()),
            log_file:         make_editor(&cfg.logging.file),
            pid_file:         make_editor(&cfg.daemon.pidfile),
        }
    }

    /// Return the current text content of a field.
    pub fn text(&self, field: FieldId) -> String {
        match field {
            FieldId::MainEmail       => editor_text(&self.main_email),
            FieldId::MainPassword    => editor_text(&self.main_password),
            FieldId::MonitorEmail    => editor_text(&self.monitor_email),
            FieldId::MonitorPassword => editor_text(&self.monitor_password),
            FieldId::ServerBind      => editor_text(&self.server_bind),
            FieldId::ServerPort      => editor_text(&self.server_port),
            FieldId::LogLevel        => editor_text(&self.log_level),
            FieldId::LogFile         => editor_text(&self.log_file),
            FieldId::PidFile         => editor_text(&self.pid_file),
            FieldId::ServerHttps     => if self.server_https { "true" } else { "false" }.to_string(),
        }
    }

    /// Overwrite the text content of a field (used for revert-on-Esc).
    pub fn set_text(&mut self, field: FieldId, text: &str) {
        if let Some(ed) = self.get_editor_mut(field) {
            *ed = make_editor(text);
        }
    }

    pub fn get_editor_mut(&mut self, field: FieldId) -> Option<&mut EditorState> {
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
            FieldId::ServerHttps     => None,
        }
    }

    pub fn get_editor(&self, field: FieldId) -> Option<&EditorState> {
        match field {
            FieldId::MainEmail       => Some(&self.main_email),
            FieldId::MainPassword    => Some(&self.main_password),
            FieldId::MonitorEmail    => Some(&self.monitor_email),
            FieldId::MonitorPassword => Some(&self.monitor_password),
            FieldId::ServerBind      => Some(&self.server_bind),
            FieldId::ServerPort      => Some(&self.server_port),
            FieldId::LogLevel        => Some(&self.log_level),
            FieldId::LogFile         => Some(&self.log_file),
            FieldId::PidFile         => Some(&self.pid_file),
            FieldId::ServerHttps     => None,
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

pub struct Model {
    pub current_screen: Screen,
    pub focus: FieldId,
    pub fields: Fields,
    pub validation: ValidationReport,
    pub status: StatusBar,
    pub dirty: bool,
    pub mode: Mode,
    pub editing_mode: EditingMode,
    /// Text snapshots taken at construction time; used to revert on Esc.
    initial_texts: HashMap<FieldId, String>,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl Model {
    pub fn new(cfg: ConfigFile) -> Self {
        let editing_mode = match cfg.tui.editing_mode {
            crate::config::EditingModeConfig::Vi    => EditingMode::Vi,
            crate::config::EditingModeConfig::Emacs => EditingMode::Emacs,
            crate::config::EditingModeConfig::Default => EditingMode::Default,
        };
        let fields = Fields::from_config(&cfg);
        // Snapshot initial text for every text field (used by Esc revert)
        let initial_texts = [
            FieldId::MainEmail, FieldId::MainPassword,
            FieldId::MonitorEmail, FieldId::MonitorPassword,
            FieldId::ServerBind, FieldId::ServerPort,
            FieldId::LogLevel, FieldId::LogFile, FieldId::PidFile,
        ].into_iter()
            .map(|f| (f, fields.text(f)))
            .collect();
        let mut m = Self {
            current_screen: Screen::Accounts,
            focus: FieldId::MainEmail,
            fields,
            validation: ValidationReport::default(),
            status: StatusBar::info("Tab/Shift-Tab: move  Enter: edit  Ctrl-→/←: screen  Ctrl-S: save  ?: help"),
            dirty: false,
            mode: Mode::Normal,
            editing_mode,
            initial_texts,
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
            let v = self.fields.text(field);
            if !v.is_empty() && !looks_like_email(&v) {
                errors.push((field, "must be a valid email address".to_string()));
            }
        }

        let port_str = self.fields.text(FieldId::ServerPort);
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
                    // Set editor mode and position cursor correctly for the field.
                    // EditorState defaults to Normal; emacs/default need Insert at end-of-text.
                    if let Some(ed) = self.fields.get_editor_mut(self.focus) {
                        match self.editing_mode {
                            EditingMode::Vi => {
                                ed.mode = EditorMode::Normal;
                            }
                            EditingMode::Emacs | EditingMode::Default => {
                                ed.mode = EditorMode::Insert;
                                // Move cursor to end of existing text (Ctrl+E in emacs).
                                let mut h = EditorEventHandler::emacs_mode();
                                h.on_key_event(
                                    KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
                                    ed,
                                );
                            }
                        }
                    }
                    self.mode = Mode::Editing;
                    self.status = StatusBar::info("Editing — Enter: confirm  Esc: cancel  ?: help");
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

        // Enter always commits and exits editing, regardless of mode
        if key.code == KeyCode::Enter {
            self.mode = Mode::Normal;
            self.status = StatusBar::info("Tab/Shift-Tab: move  Enter: edit  Ctrl-→/←: screen  Ctrl-S: save  ?: help");
            self.validate();
            return Action::None;
        }

        // Esc: in vi mode, only exit our Editing layer when already in Normal;
        // let edtui handle Insert→Normal first. In emacs/default, always exit.
        if key.code == KeyCode::Esc {
            let already_normal = self.fields.get_editor(self.focus)
                .map(|ed| ed.mode == EditorMode::Normal)
                .unwrap_or(true);
            let should_exit = match self.editing_mode {
                EditingMode::Vi => already_normal,
                EditingMode::Emacs | EditingMode::Default => true,
            };
            if should_exit {
                let init_text = self.initial_texts.get(&self.focus).cloned().unwrap_or_default();
                self.fields.set_text(self.focus, &init_text);
                self.mode = Mode::Normal;
                self.status = StatusBar::info("Tab/Shift-Tab: move  Enter: edit  Ctrl-→/←: screen  Ctrl-S: save  ?: help");
                self.validate();
                return Action::None;
            }
            // Fall through — let edtui switch Insert→Normal
        }

        // Reject non-digit input for the port field before it reaches edtui
        if let KeyCode::Char(c) = key.code
            && self.focus.is_numeric() && !c.is_ascii_digit() { return Action::None; }

        // Route to edtui. Create the handler each time (it's a thin struct).
        let mut handler = match self.editing_mode {
            EditingMode::Vi => EditorEventHandler::vim_mode(),
            EditingMode::Emacs | EditingMode::Default => EditorEventHandler::emacs_mode(),
        };
        if let Some(editor) = self.fields.get_editor_mut(self.focus) {
            handler.on_key_event(key, editor);
            self.dirty = true;
            self.validate();
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
        use crate::config::{
            AccountEntry, AccountsConfig, DaemonConfig, EditingModeConfig,
            LoggingConfig, LogLevel, ServerConfig, TuiConfig,
        };
        let port = self.fields.text(FieldId::ServerPort).parse::<u32>().unwrap_or(1080);
        let log_level_str = self.fields.text(FieldId::LogLevel);
        let log_level = match log_level_str.as_str() {
            "trace" => LogLevel::Trace,
            "debug" => LogLevel::Debug,
            "warn"  => LogLevel::Warn,
            "error" => LogLevel::Error,
            _       => LogLevel::Info,
        };
        let editing_mode_cfg = match self.editing_mode {
            crate::config::EditingMode::Vi      => EditingModeConfig::Vi,
            crate::config::EditingMode::Emacs   => EditingModeConfig::Emacs,
            crate::config::EditingMode::Default => EditingModeConfig::Default,
        };
        let main_email   = self.fields.text(FieldId::MainEmail);
        let monitor_email = self.fields.text(FieldId::MonitorEmail);
        ConfigFile {
            schema_version: crate::config::CURRENT_SCHEMA_VERSION,
            accounts: AccountsConfig {
                main:    AccountEntry { email: Some(main_email).filter(|s| !s.is_empty()) },
                monitor: AccountEntry { email: Some(monitor_email).filter(|s| !s.is_empty()) },
            },
            server: ServerConfig {
                bind:  self.fields.text(FieldId::ServerBind),
                port,
                https: self.fields.server_https,
            },
            logging: LoggingConfig {
                level: log_level,
                file:  self.fields.text(FieldId::LogFile),
            },
            daemon: DaemonConfig {
                pidfile: self.fields.text(FieldId::PidFile),
            },
            tui: TuiConfig { editing_mode: editing_mode_cfg },
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
        assert_eq!(m.fields.text(FieldId::MainEmail), "rider@example.com");
        assert_eq!(m.fields.text(FieldId::MonitorEmail), "monitor@example.com");
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
        // Wipe existing text then type new value
        let len = m.fields.text(FieldId::MainEmail).len();
        for _ in 0..len {
            m.update(key(KeyCode::Backspace));
        }
        for c in "new@email.com".chars() {
            m.update(key(KeyCode::Char(c)));
        }
        assert_eq!(m.fields.text(FieldId::MainEmail), "new@email.com");
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
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Enter)); // editing
        m.update(key(KeyCode::Backspace));
        m.update(key(KeyCode::Backspace));
        m.update(key(KeyCode::Esc)); // revert (emacs/default mode exits immediately)
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.fields.text(FieldId::MainEmail), original);
    }

    #[test]
    fn numeric_field_rejects_non_digit_input() {
        let mut m = model_with_valid_config();
        m.update(ctrl(KeyCode::Right)); // → Server
        m.update(key(KeyCode::Tab)); // ServerBind → ServerPort
        m.update(key(KeyCode::Enter)); // edit
        let before = m.fields.text(FieldId::ServerPort);
        m.update(key(KeyCode::Char('x')));
        assert_eq!(m.fields.text(FieldId::ServerPort), before, "non-digit should be ignored");
        m.update(key(KeyCode::Char('9')));
        assert!(m.fields.text(FieldId::ServerPort).ends_with('9'));
    }

    #[test]
    fn email_field_validation_runs_on_every_update() {
        let mut m = model_with_valid_config();
        m.update(key(KeyCode::Enter)); // edit MainEmail
        // Clear field
        let len = m.fields.text(FieldId::MainEmail).len();
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
        let len = m.fields.text(FieldId::ServerPort).len();
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

    // -----------------------------------------------------------------------
    // vi mode tests
    // -----------------------------------------------------------------------

    fn model_vi() -> Model {
        let mut cfg = ConfigFile::default();
        cfg.accounts.main.email = Some("rider@example.com".to_string());
        cfg.tui.editing_mode = crate::config::EditingModeConfig::Vi;
        Model::new(cfg)
    }

    #[test]
    fn vi_mode_starts_in_normal_editor_state() {
        let m = model_vi();
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Normal,
            "vi mode should start with EditorState in Normal mode");
    }

    #[test]
    fn vi_enter_normal_mode_on_field_enter() {
        let mut m = model_vi();
        m.update(key(KeyCode::Enter)); // enter our Mode::Editing
        assert_eq!(m.mode, Mode::Editing);
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Normal,
            "vi should open in Normal mode");
    }

    #[test]
    fn vi_i_enters_insert_mode() {
        let mut m = model_vi();
        m.update(key(KeyCode::Enter)); // Mode::Editing
        m.update(key(KeyCode::Char('i'))); // edtui: Normal → Insert
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Insert,
            "pressing i in vi normal mode should enter Insert");
    }

    #[test]
    fn vi_esc_in_insert_transitions_to_normal_not_exits_editing() {
        let mut m = model_vi();
        m.update(key(KeyCode::Enter));          // Mode::Editing, editor Normal
        m.update(key(KeyCode::Char('i')));      // editor → Insert
        m.update(key(KeyCode::Esc));            // editor → Normal (NOT exit Mode::Editing)
        assert_eq!(m.mode, Mode::Editing,
            "first Esc in vi Insert should stay in Mode::Editing");
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Normal,
            "editor should now be in Normal mode");
    }

    #[test]
    fn vi_second_esc_exits_editing_mode() {
        let mut m = model_vi();
        m.update(key(KeyCode::Enter));          // Mode::Editing
        m.update(key(KeyCode::Char('i')));      // Insert
        m.update(key(KeyCode::Esc));            // → Normal (stay in Mode::Editing)
        m.update(key(KeyCode::Esc));            // editor already Normal → exit Mode::Editing
        assert_eq!(m.mode, Mode::Normal,
            "second Esc in vi Normal should exit Mode::Editing");
    }

    #[test]
    fn vi_second_esc_reverts_to_initial_text() {
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Enter));          // Mode::Editing
        m.update(key(KeyCode::Char('A')));      // append at end → Insert
        m.update(key(KeyCode::Char('X')));      // type X
        m.update(key(KeyCode::Esc));            // Insert → Normal
        m.update(key(KeyCode::Esc));            // exit and revert
        assert_eq!(m.fields.text(FieldId::MainEmail), original,
            "should revert to initial text on double-Esc in vi mode");
    }

    // -----------------------------------------------------------------------
    // emacs mode tests
    // -----------------------------------------------------------------------

    fn model_emacs() -> Model {
        let mut cfg = ConfigFile::default();
        cfg.accounts.main.email = Some("hello@example.com".to_string());
        cfg.tui.editing_mode = crate::config::EditingModeConfig::Emacs;
        Model::new(cfg)
    }

    #[test]
    fn emacs_mode_starts_in_insert_editor_state() {
        let mut m = model_emacs();
        m.update(key(KeyCode::Enter));
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Insert,
            "emacs mode should open in Insert mode");
    }

    #[test]
    fn emacs_esc_exits_editing_immediately() {
        let mut m = model_emacs();
        m.update(key(KeyCode::Enter));
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::Normal,
            "single Esc in emacs mode should immediately exit Mode::Editing");
    }

    #[test]
    fn emacs_ctrl_a_moves_to_start_of_line() {
        let mut m = model_emacs();
        m.update(key(KeyCode::Enter)); // in editing, cursor at end
        m.update(Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)));
        // Cursor should now be at col 0
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.cursor.col, 0, "Ctrl+A should move cursor to start");
    }

    #[test]
    fn emacs_ctrl_k_kills_to_end_of_line() {
        let mut m = model_emacs();
        m.update(key(KeyCode::Enter)); // cursor at end
        // Move to start first
        m.update(Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)));
        // Kill to end
        m.update(Event::Key(KeyEvent::new(KeyCode::Char('k'), KeyModifiers::CONTROL)));
        assert_eq!(m.fields.text(FieldId::MainEmail), "",
            "Ctrl+K from start should clear the line");
    }
}
