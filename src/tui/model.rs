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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Editing,
    ConfirmDiscard,
    Help,
    /// Vi `:command` mode — accumulates characters until Enter/Esc.
    VimCommand { buffer: String },
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Save,
    /// Save to disk but keep the TUI open (vi `:w`).
    WriteOnly,
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
    // Lines::from("") yields 0 rows (str::lines() on "" is empty).
    // An EditorState with 0 rows is invalid — inserts and motions can panic.
    // Use make_empty_editor() for the empty case.
    if text.is_empty() {
        return make_empty_editor();
    }
    let mut s = EditorState::new(Lines::from(text));
    s.set_single_line(true);
    s
}

/// Create a single-line EditorState with one empty row — the correct empty state.
fn make_empty_editor() -> EditorState {
    let mut s = EditorState::new(Lines::new(vec![vec![]]));
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
    /// First character of a two-key vi sequence (e.g. `Z` awaiting `Z`/`Q`,
    /// `d` awaiting `d`, `y` awaiting `y`).
    /// Cleared on any keypress that does not continue the sequence.
    pending_key: Option<char>,
    /// Persistent event handler for the active field editor.
    /// Must survive across key events so multi-key sequences (e.g. `dd`, `dw`)
    /// can accumulate state between the first and second keypress.
    editor_handler: EditorEventHandler,
    /// Vi paste register. Populated by `dd`/`yy`, consumed by `p`/`P`.
    /// Survives across fields so `ddjp` moves data between fields.
    paste_buffer: String,
    /// Undo history for model-level operations (dd, p, P). Each entry stores
    /// the field's text and cursor position before the operation.
    undo_stack: Vec<UndoEntry>,
    /// Text snapshots taken at construction time; used to revert on Esc.
    initial_texts: HashMap<FieldId, String>,
}

#[derive(Clone, Debug)]
struct UndoEntry {
    field: FieldId,
    text: String,
    cursor_col: usize,
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

impl Model {
    pub fn new(cfg: ConfigFile) -> Self {
        let editing_mode = match cfg.tui.editing_mode {
            crate::config::EditingModeConfig::Vi    => EditingMode::Vi,
            crate::config::EditingModeConfig::Emacs => EditingMode::Emacs,
            // When the config says "default", fall back to ~/.editrc detection.
            // This is the same precedence chain as ResolvedConfig but applied
            // directly so the TUI respects the user's global editrc preference.
            // Skip in tests so developer's own ~/.editrc doesn't change test behaviour.
            crate::config::EditingModeConfig::Default => {
                #[cfg(not(test))]
                {
                    let home = std::env::var("HOME")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"));
                    match crate::config::editrc::detect_from_editrc(&home) {
                        Some(crate::config::editrc::EditrcMode::Vi)    => EditingMode::Vi,
                        Some(crate::config::editrc::EditrcMode::Emacs) => EditingMode::Emacs,
                        None => EditingMode::Default,
                    }
                }
                #[cfg(test)]
                EditingMode::Default
            }
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
        let editor_handler = make_editor_handler(editing_mode);
        let mut m = Self {
            current_screen: Screen::Accounts,
            focus: FieldId::MainEmail,
            fields,
            validation: ValidationReport::default(),
            status: StatusBar::clear(),
            dirty: false,
            mode: Mode::Normal,
            editing_mode,
            pending_key: None,
            editor_handler,
            paste_buffer: String::new(),
            undo_stack: Vec::new(),
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

fn make_editor_handler(editing_mode: EditingMode) -> EditorEventHandler {
    match editing_mode {
        EditingMode::Vi => {
            // edtui's default vim handler binds dd / D / x / X but not dw / db / de.
            // Add the operator-motion combinations users expect from real vim.
            use edtui::EditorMode as EM;
            use edtui::actions::delete::{DeleteSelection, DeleteWordBackward, DeleteWordForward};
            use edtui::actions::motion::MoveWordForwardToEndOfWord;
            use edtui::actions::{Chainable, SwitchMode};
            use edtui::events::{KeyEventHandler, KeyEventRegister, KeyInput};

            let mut handler = KeyEventHandler::vim_mode();
            handler.insert(
                KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('w')]),
                DeleteWordForward(1),
            );
            handler.insert(
                KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('b')]),
                DeleteWordBackward(1),
            );
            // `de` (delete to end of word) is composed: enter Visual, select to
            // end-of-word, delete selection, return to Normal.
            handler.insert(
                KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('e')]),
                SwitchMode(EM::Visual)
                    .chain(MoveWordForwardToEndOfWord(1))
                    .chain(DeleteSelection)
                    .chain(SwitchMode(EM::Normal)),
            );
            EditorEventHandler::new(handler)
        }
        EditingMode::Emacs | EditingMode::Default => EditorEventHandler::emacs_mode(),
    }
}

// ---------------------------------------------------------------------------
// Status bar content — pure function, used by view layer
// ---------------------------------------------------------------------------

/// Compute the left-zone text for the status bar from model state.
/// The view calls this instead of reading `model.status.message` for
/// non-error content, so the mode indicator is always in sync.
pub fn status_bar_content(
    mode: &Mode,
    editor_mode: Option<EditorMode>,
    editing_mode: EditingMode,
) -> String {
    match mode {
        Mode::VimCommand { buffer } => format!(":{buffer}"),
        Mode::Editing => match editing_mode {
            EditingMode::Vi => match editor_mode {
                Some(EditorMode::Insert) => "-- INSERT --".to_string(),
                Some(EditorMode::Visual) => "-- VISUAL --".to_string(),
                _ => String::new(),
            },
            EditingMode::Emacs | EditingMode::Default =>
                "Editing \u{2014} Enter: confirm  Esc: cancel".to_string(),
        },
        Mode::Normal => match editing_mode {
            EditingMode::Vi =>
                "Tab: screen  j/k: field  h/l: cursor  i/a: edit  :: command  ZZ: save  ?: help".to_string(),
            _ =>
                "Tab/Shift-Tab: screen  \u{2191}/\u{2193}: field  Enter: edit  Ctrl-S: save  ?: help".to_string(),
        },
        Mode::ConfirmDiscard =>
            "Unsaved changes. y: discard  n: go back".to_string(),
        Mode::Help => String::new(),
    }
}

// ---------------------------------------------------------------------------
// update — pure event handler
// ---------------------------------------------------------------------------

impl Model {
    pub fn update(&mut self, ev: Event) -> Action {
        // Normalise SHIFT on character keys. Most terminals send uppercase
        // letters as `KeyCode::Char('Z')` *with* `KeyModifiers::SHIFT` set;
        // our handlers' modifier checks treat that as "modified" and reject
        // the key. Strip SHIFT for `Char(_)` so the case (which is already
        // baked into the char itself) is the sole signal.
        let ev = match ev {
            Event::Key(mut k) if matches!(k.code, KeyCode::Char(_)) => {
                k.modifiers -= KeyModifiers::SHIFT;
                Event::Key(k)
            }
            other => other,
        };

        match &self.mode {
            Mode::ConfirmDiscard => self.handle_confirm_discard(ev),
            Mode::Help => self.handle_help(ev),
            Mode::Editing => self.handle_editing(ev),
            Mode::Normal => self.handle_normal(ev),
            Mode::VimCommand { .. } => self.handle_vim_command(ev),
        }
    }

    fn handle_normal(&mut self, ev: Event) -> Action {
        // Clear any previous non-error status on each Normal keypress
        if !self.status.is_error { self.status = StatusBar::clear(); }

        let Event::Key(key) = ev else { return Action::None; };

        // Vi-specific outer navigation (checked first; falls through on None)
        if self.editing_mode == EditingMode::Vi
            && let Some(action) = self.handle_vi_outer_key(key) {
            return action;
        }

        match (key.modifiers, key.code) {
            // Tab / Shift-Tab: switch screens (the visible "tabs" at the top).
            (KeyModifiers::NONE, KeyCode::Tab) => {
                let next = self.current_screen.next();
                self.go_to_screen(next);
                Action::None
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                let prev = self.current_screen.prev();
                self.go_to_screen(prev);
                Action::None
            }
            // Ctrl-Right / Ctrl-Left: alias for screen navigation.
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
            // Down / Up: field navigation within a screen (for non-vi users).
            (KeyModifiers::NONE, KeyCode::Down) => {
                self.advance_focus(true);
                Action::None
            }
            (KeyModifiers::NONE, KeyCode::Up) => {
                self.advance_focus(false);
                Action::None
            }
            // Editing: Enter enters at end (append) in all modes
            (KeyModifiers::NONE, KeyCode::Enter) => {
                self.enter_editing(true);
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
            | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                // In vi mode, Esc is *always* safe — it never raises a
                // confirm-discard prompt (a concept that does not exist in vi).
                // The user explicitly quits with :q / :q! / ZZ / ZQ. Esc here
                // simply clears any pending two-key sequence (Z…, d…).
                if self.editing_mode == EditingMode::Vi {
                    self.pending_key = None;
                    return Action::None;
                }
                if self.dirty {
                    self.mode = Mode::ConfirmDiscard;
                    Action::None
                } else {
                    Action::Cancel
                }
            }
            // q — only quit in non-vi mode (vi mode uses :q / q! bindings)
            (KeyModifiers::NONE, KeyCode::Char('q'))
                if self.editing_mode != EditingMode::Vi =>
            {
                if self.dirty {
                    self.mode = Mode::ConfirmDiscard;
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

    /// Handle vi-specific outer navigation keys.
    /// Returns `Some(action)` if the key was consumed, `None` to fall through
    /// to the common bindings.
    fn handle_vi_outer_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Resolve pending two-key sequences (Z…, d…)
        if let Some('Z') = self.pending_key {
            self.pending_key = None;
            match key.code {
                KeyCode::Char('Z') => return Some(if self.validation.is_valid() {
                    Action::Save
                } else {
                    self.status = StatusBar::error("Fix validation errors before saving");
                    Action::None
                }),
                KeyCode::Char('Q') => return Some(Action::DiscardConfirmed),
                _ => return self.handle_vi_outer_key(key),
            }
        }
        if let Some('d') = self.pending_key {
            self.pending_key = None;
            match key.code {
                KeyCode::Char('d') => {
                    // dd from outer Normal: yank current text into the paste
                    // buffer, then clear the field. Booleans have no text.
                    if !self.focus.is_boolean() {
                        self.push_undo(self.focus);
                        self.paste_buffer = self.fields.text(self.focus);
                        self.fields.set_text(self.focus, "");
                        self.dirty = true;
                        self.validate();
                    }
                    return Some(Action::None);
                }
                _ => return self.handle_vi_outer_key(key),
            }
        }
        if let Some('y') = self.pending_key {
            self.pending_key = None;
            match key.code {
                KeyCode::Char('y') => {
                    // yy from outer Normal: copy current field text to the
                    // paste buffer (no clear, no dirty).
                    if !self.focus.is_boolean() {
                        self.paste_buffer = self.fields.text(self.focus);
                    }
                    return Some(Action::None);
                }
                _ => return self.handle_vi_outer_key(key),
            }
        }

        if key.modifiers != KeyModifiers::NONE { return None; }

        match key.code {
            KeyCode::Char('j') => { self.advance_focus(true);  Some(Action::None) }
            KeyCode::Char('k') => { self.advance_focus(false); Some(Action::None) }
            KeyCode::Char('l') | KeyCode::Char(' ') => {
                // Move cursor right within the focused field (vi Normal).
                // Space is the standard vi alias for `l`.
                let max = self.fields.text(self.focus).chars().count().saturating_sub(1);
                if let Some(ed) = self.fields.get_editor_mut(self.focus) {
                    ed.cursor.col = (ed.cursor.col + 1).min(max);
                }
                Some(Action::None)
            }
            KeyCode::Char('h') => {
                // Move cursor left within the focused field.
                if let Some(ed) = self.fields.get_editor_mut(self.focus) {
                    ed.cursor.col = ed.cursor.col.saturating_sub(1);
                }
                Some(Action::None)
            }
            KeyCode::Char('i') => { self.enter_editing(false); Some(Action::None) }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.enter_editing(true);
                Some(Action::None)
            }
            KeyCode::Char('Z') => {
                self.pending_key = Some('Z');
                Some(Action::None)
            }
            KeyCode::Char('d') => {
                // First half of `dd`. Buffer it and wait for the second `d`.
                self.pending_key = Some('d');
                Some(Action::None)
            }
            KeyCode::Char('y') => {
                // First half of `yy`. Buffer it and wait for the second `y`.
                self.pending_key = Some('y');
                Some(Action::None)
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                // Paste at the cursor — the highlighted char shifts right.
                self.vi_paste_at_cursor();
                Some(Action::None)
            }
            KeyCode::Char('u') => {
                // Undo the most recent destructive operation (dd, p, P).
                self.pop_undo();
                Some(Action::None)
            }
            KeyCode::Char(':') => {
                self.mode = Mode::VimCommand { buffer: String::new() };
                Some(Action::None)
            }
            _ => None,
        }
    }

    /// Snapshot the focused field's text and cursor before a destructive
    /// operation so the user can `u`/`:u` to roll it back.
    fn push_undo(&mut self, field: FieldId) {
        if field.is_boolean() { return; }
        let text = self.fields.text(field);
        let cursor_col = self.fields.get_editor(field)
            .map(|ed| ed.cursor.col)
            .unwrap_or(0);
        self.undo_stack.push(UndoEntry { field, text, cursor_col });
    }

    /// Pop the most recent undo entry (if any) and restore that field.
    /// Focus jumps to the restored field so the user sees the change.
    fn pop_undo(&mut self) {
        if let Some(entry) = self.undo_stack.pop() {
            // Move to the screen and field that owned the undone change.
            if let Some(screen) = Screen::ALL.iter()
                .find(|s| FieldId::for_screen(**s).contains(&entry.field))
                .copied()
            {
                self.current_screen = screen;
            }
            self.focus = entry.field;
            self.fields.set_text(entry.field, &entry.text);
            if let Some(ed) = self.fields.get_editor_mut(entry.field) {
                ed.cursor.col = entry.cursor_col.min(
                    entry.text.chars().count().saturating_sub(1)
                );
            }
            self.dirty = true;
            self.validate();
        } else {
            self.status = StatusBar::error("Nothing to undo");
        }
    }

    /// Paste the contents of the paste buffer into the focused field at the
    /// cursor position. The character currently under the cursor (highlighted
    /// by the block cursor) shifts right to make room for the pasted text.
    /// Cursor moves to the last character of the pasted content (vim convention).
    ///
    /// Both `p` and `P` use this in our single-line-field model — there is no
    /// distinct "after current line" vs "at current line" because each field
    /// is one line.
    fn vi_paste_at_cursor(&mut self) {
        if self.focus.is_boolean() || self.paste_buffer.is_empty() {
            return;
        }
        self.push_undo(self.focus);
        let current = self.fields.text(self.focus);
        let chars: Vec<char> = current.chars().collect();
        let cursor_col = self.fields.get_editor(self.focus)
            .map(|ed| ed.cursor.col.min(chars.len()))
            .unwrap_or(chars.len());

        // Insert AT the cursor: the highlighted char shifts right.
        let insert_at = cursor_col;

        let buffer_chars: Vec<char> = self.paste_buffer.chars().collect();
        let mut new_chars: Vec<char> = chars[..insert_at].to_vec();
        new_chars.extend_from_slice(&buffer_chars);
        new_chars.extend_from_slice(&chars[insert_at..]);
        let new_text: String = new_chars.iter().collect();

        self.fields.set_text(self.focus, &new_text);

        // Position cursor on the last character of the pasted region.
        if let Some(ed) = self.fields.get_editor_mut(self.focus) {
            let new_col = insert_at + buffer_chars.len().saturating_sub(1);
            ed.cursor.col = new_col.min(new_chars.len().saturating_sub(1));
        }
        self.dirty = true;
        self.validate();
    }

    /// Enter editing mode on the focused field.
    /// `cursor_at_end`: position cursor at end of existing text (`a`/`A`/Enter),
    /// or leave at position 0 (`i`).
    fn enter_editing(&mut self, cursor_at_end: bool) {
        // Reset the persistent handler so no pending multi-key state from a
        // previous field bleeds into this one.
        self.editor_handler = make_editor_handler(self.editing_mode);
        if self.focus.is_boolean() {
            self.fields.server_https = !self.fields.server_https;
            self.dirty = true;
            self.validate();
            return;
        }
        if let Some(ed) = self.fields.get_editor_mut(self.focus) {
            ed.mode = EditorMode::Insert;
            if cursor_at_end {
                let mut h = EditorEventHandler::emacs_mode();
                h.on_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL), ed);
            }
            // else cursor stays at col 0 (i = insert at start)
        }
        self.mode = Mode::Editing;
    }

    fn handle_vim_command(&mut self, ev: Event) -> Action {
        let Event::Key(key) = ev else { return Action::None; };

        let buffer = match &self.mode {
            Mode::VimCommand { buffer } => buffer.clone(),
            _ => return Action::None,
        };

        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                Action::None
            }
            KeyCode::Backspace => {
                let mut buf = buffer;
                buf.pop();
                self.mode = Mode::VimCommand { buffer: buf };
                Action::None
            }
            KeyCode::Enter => {
                let cmd = buffer.trim().to_string();
                self.mode = Mode::Normal;
                self.execute_vim_command(&cmd)
            }
            KeyCode::Char(c) => {
                let mut buf = buffer;
                buf.push(c);
                self.mode = Mode::VimCommand { buffer: buf };
                Action::None
            }
            _ => Action::None,
        }
    }

    fn execute_vim_command(&mut self, cmd: &str) -> Action {
        match cmd {
            "w" => {
                if !self.validation.is_valid() {
                    self.status = StatusBar::error("Fix validation errors before saving");
                    return Action::None;
                }
                self.dirty = false;
                self.status = StatusBar::info("Saved.");
                Action::WriteOnly
            }
            "wq" | "x" => {
                if !self.validation.is_valid() {
                    self.status = StatusBar::error("Fix validation errors before saving");
                    return Action::None;
                }
                Action::Save
            }
            "q" => {
                if self.dirty {
                    self.status = StatusBar::error(
                        "No write since last change (add ! to override)"
                    );
                    Action::None
                } else {
                    Action::Cancel
                }
            }
            "q!" => Action::DiscardConfirmed,
            "u" | "undo" => {
                self.pop_undo();
                Action::None
            }
            "" => Action::None,
            other => {
                self.status = StatusBar::error(format!("unknown command: {other}"));
                Action::None
            }
        }
    }

    fn handle_editing(&mut self, ev: Event) -> Action {
        let Event::Key(key) = ev else { return Action::None; };

        // Enter always commits and exits editing, regardless of mode
        if key.code == KeyCode::Enter {
            self.mode = Mode::Normal;
            self.status = StatusBar::clear();
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
                // Vi mode: Normal-mode edits (dd, dw, x, …) are permanent —
                // Esc exits the field keeping whatever state was reached.
                // Emacs/default mode: Esc is "cancel", so revert to the value
                // the field had when editing began.
                if matches!(self.editing_mode, EditingMode::Emacs | EditingMode::Default) {
                    let init_text = self.initial_texts.get(&self.focus).cloned().unwrap_or_default();
                    self.fields.set_text(self.focus, &init_text);
                }
                self.mode = Mode::Normal;
                self.status = StatusBar::clear();
                self.validate();
                return Action::None;
            }
            // Fall through — let edtui switch Insert→Normal
        }

        // In vi mode, when the field editor is in Normal mode, intercept the same
        // outer-navigation keys that work in Mode::Normal.  This unifies vi's
        // single Normal mode: the user doesn't need a second Esc to navigate.
        if self.editing_mode == EditingMode::Vi {
            let in_editor_normal = self.fields.get_editor(self.focus)
                .map(|ed| ed.mode == EditorMode::Normal)
                .unwrap_or(false);
            if in_editor_normal {
                // Resolve pending Z sequence first (ZZ / ZQ)
                if let Some('Z') = self.pending_key {
                    self.pending_key = None;
                    match key.code {
                        KeyCode::Char('Z') => {
                            self.mode = Mode::Normal;
                            return if self.validation.is_valid() {
                                Action::Save
                            } else {
                                self.status = StatusBar::error("Fix validation errors before saving");
                                Action::None
                            };
                        }
                        KeyCode::Char('Q') => {
                            self.mode = Mode::Normal;
                            return Action::DiscardConfirmed;
                        }
                        _ => {} // fall through — re-process key below
                    }
                }

                // Pending 'd':
                //   second key 'd' → our model-level dd (cross-field buffer)
                //   else            → forward the buffered 'd' to edtui so it
                //                     can build d{motion} like dw, db, de, dh, dl.
                if let Some('d') = self.pending_key {
                    self.pending_key = None;
                    if matches!(key.code, KeyCode::Char('d')) {
                        if !self.focus.is_boolean() {
                            self.push_undo(self.focus);
                            self.paste_buffer = self.fields.text(self.focus);
                            self.fields.set_text(self.focus, "");
                            self.dirty = true;
                            self.validate();
                        }
                        return Action::None;
                    }
                    // Forward the buffered 'd' to edtui, then fall through so
                    // the current key continues normal processing (it will be
                    // routed to edtui at the bottom of this function).
                    let field = self.focus;
                    if let Some(editor) = self.fields.get_editor_mut(field) {
                        let d_key = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE);
                        self.editor_handler.on_key_event(d_key, editor);
                    }
                }
                // Pending 'y': symmetrical handling for yy / y{motion}.
                if let Some('y') = self.pending_key {
                    self.pending_key = None;
                    if matches!(key.code, KeyCode::Char('y')) {
                        if !self.focus.is_boolean() {
                            self.paste_buffer = self.fields.text(self.focus);
                        }
                        return Action::None;
                    }
                    let field = self.focus;
                    if let Some(editor) = self.fields.get_editor_mut(field) {
                        let y_key = KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE);
                        self.editor_handler.on_key_event(y_key, editor);
                    }
                }

                // h / l are NOT intercepted here — edtui handles them as
                // cursor motion within the field, which is what vi expects.
                match (key.modifiers, key.code) {
                    (KeyModifiers::NONE, KeyCode::Char('j')) => {
                        self.mode = Mode::Normal;
                        self.advance_focus(true);
                        return Action::None;
                    }
                    (KeyModifiers::NONE, KeyCode::Char('k')) => {
                        self.mode = Mode::Normal;
                        self.advance_focus(false);
                        return Action::None;
                    }
                    // Tab / Shift-Tab: exit field, switch screen.
                    (KeyModifiers::NONE, KeyCode::Tab) => {
                        self.mode = Mode::Normal;
                        let s = self.current_screen.next();
                        self.go_to_screen(s);
                        return Action::None;
                    }
                    (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                        self.mode = Mode::Normal;
                        let s = self.current_screen.prev();
                        self.go_to_screen(s);
                        return Action::None;
                    }
                    (KeyModifiers::NONE, KeyCode::Char(':')) => {
                        self.mode = Mode::VimCommand { buffer: String::new() };
                        return Action::None;
                    }
                    (KeyModifiers::NONE, KeyCode::Char('Z')) => {
                        self.pending_key = Some('Z');
                        return Action::None;
                    }
                    // Buffer-aware ops — same semantics as outer Normal.
                    (KeyModifiers::NONE, KeyCode::Char('d')) => {
                        self.pending_key = Some('d');
                        return Action::None;
                    }
                    (KeyModifiers::NONE, KeyCode::Char('y')) => {
                        self.pending_key = Some('y');
                        return Action::None;
                    }
                    (KeyModifiers::NONE, KeyCode::Char('p'))
                    | (KeyModifiers::NONE, KeyCode::Char('P')) => {
                        self.vi_paste_at_cursor();
                        return Action::None;
                    }
                    (KeyModifiers::NONE, KeyCode::Char('u')) => {
                        self.pop_undo();
                        return Action::None;
                    }
                    (KeyModifiers::NONE, KeyCode::Char('?')) => {
                        self.mode = Mode::Help;
                        return Action::None;
                    }
                    _ => {}
                }
            }
        }

        // Reject non-digit characters for numeric fields, but only in Insert mode.
        // Normal-mode commands (dd, dw, b, w, x, …) must still reach edtui.
        let in_insert = self.fields.get_editor(self.focus)
            .map(|ed| ed.mode == EditorMode::Insert)
            .unwrap_or(false);
        if let KeyCode::Char(c) = key.code
            && self.focus.is_numeric() && in_insert && !c.is_ascii_digit() { return Action::None; }

        // Route to the persistent edtui handler (must survive between key events
        // so multi-key sequences like `dd` or `dw` can complete across calls).
        let field = self.focus;
        if let Some(editor) = self.fields.get_editor_mut(field) {
            self.editor_handler.on_key_event(key, editor);
            // `dd` on a single-line field uses edtui's DeleteLine which removes
            // the row entirely, leaving lines.len() == 0 — an invalid state that
            // breaks subsequent inserts. Reinstate a clean empty row instead.
            // `dd` on a single-line field removes the only row (lines.len() == 0).
            // Reinstate one valid empty row so subsequent inserts don't panic.
            if editor.lines.is_empty() {
                *editor = make_empty_editor();
            }
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
                self.status = StatusBar::clear();
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
    fn down_arrow_advances_focus_within_screen() {
        let mut m = model_with_valid_config();
        assert_eq!(m.focus, FieldId::MainEmail);
        m.update(key(KeyCode::Down));
        assert_eq!(m.focus, FieldId::MainPassword);
        m.update(key(KeyCode::Down));
        assert_eq!(m.focus, FieldId::MonitorEmail);
        m.update(key(KeyCode::Down));
        assert_eq!(m.focus, FieldId::MonitorPassword);
        // wraps around
        m.update(key(KeyCode::Down));
        assert_eq!(m.focus, FieldId::MainEmail);
    }

    #[test]
    fn up_arrow_moves_focus_backward() {
        let mut m = model_with_valid_config();
        assert_eq!(m.focus, FieldId::MainEmail);
        m.update(key(KeyCode::Up));
        // wraps to last field on screen
        assert_eq!(m.focus, FieldId::MonitorPassword);
    }

    #[test]
    fn tab_switches_to_next_screen() {
        let mut m = model_with_valid_config();
        assert_eq!(m.current_screen, Screen::Accounts);
        m.update(key(KeyCode::Tab));
        assert_eq!(m.current_screen, Screen::Server);
        m.update(key(KeyCode::Tab));
        assert_eq!(m.current_screen, Screen::Logging);
    }

    #[test]
    fn shift_tab_switches_to_previous_screen() {
        let mut m = model_with_valid_config();
        m.update(key(KeyCode::Tab)); // → Server
        m.update(shift_backtab());   // ← Accounts
        assert_eq!(m.current_screen, Screen::Accounts);
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
    fn numeric_field_rejects_non_digit_in_insert_mode() {
        let mut m = model_with_valid_config();
        m.update(ctrl(KeyCode::Right)); // → Server
        m.update(key(KeyCode::Down)); // ServerBind → ServerPort
        m.update(key(KeyCode::Enter)); // edit — Insert mode
        let before = m.fields.text(FieldId::ServerPort);
        // Non-digit blocked in Insert mode
        m.update(key(KeyCode::Char('x')));
        assert_eq!(m.fields.text(FieldId::ServerPort), before, "non-digit should be ignored in Insert");
        m.update(key(KeyCode::Char('9')));
        assert!(m.fields.text(FieldId::ServerPort).ends_with('9'));
    }

    #[test]
    fn vi_dd_works_on_numeric_port_field() {
        let mut m = model_vi();
        m.update(ctrl(KeyCode::Right));     // → Server screen
        m.update(key(KeyCode::Char('j')));  // → ServerPort
        assert_eq!(m.focus, FieldId::ServerPort);
        // Enter via 'a', Esc to Normal, then dd
        m.update(key(KeyCode::Char('a')));  // Insert at end
        m.update(key(KeyCode::Esc));        // Insert → Normal
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));  // dd
        assert_eq!(m.fields.text(FieldId::ServerPort), "",
            "dd should clear the port field");
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
        m.update(key(KeyCode::Down)); // focus MainPassword
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
        m.update(key(KeyCode::Down)); // → ServerPort
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
    fn vi_enter_opens_insert_mode_at_end() {
        // STEP 02.2: Enter in vi mode behaves like 'a' — Insert at end.
        let mut m = model_vi();
        m.update(key(KeyCode::Enter));
        assert_eq!(m.mode, Mode::Editing);
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Insert,
            "Enter in vi mode should open Insert mode (same as 'a')");
        let text_len = m.fields.text(FieldId::MainEmail).len();
        assert_eq!(ed.cursor.col, text_len, "cursor should be at end");
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
    fn vi_second_esc_exits_keeping_edits() {
        // Vi mode: Esc from Normal exits without reverting.
        // Normal-mode edits (dd, dw, x, …) are permanent — same as real vim.
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Enter));          // Mode::Editing (Insert)
        m.update(key(KeyCode::Char('A')));      // edtui: append-at-end → Insert
        m.update(key(KeyCode::Char('X')));      // insert 'X'
        m.update(key(KeyCode::Esc));            // Insert → Normal (stay in Mode::Editing)
        m.update(key(KeyCode::Esc));            // Normal → exit Mode::Editing, NO revert
        assert_eq!(m.mode, Mode::Normal);
        // The typed 'X' must still be present — vi Esc does NOT revert.
        let new_val = m.fields.text(FieldId::MainEmail);
        assert_ne!(new_val, original, "vi Esc should NOT revert normal-mode edits");
        assert!(new_val.ends_with('X'), "typed character should persist after Esc");
    }

    #[test]
    fn vi_esc_in_outer_normal_is_safe_when_dirty() {
        // Real-vi semantics: Esc never raises a confirm-discard dialog.
        let mut m = model_vi();
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));   // dd — field cleared, dirty=true
        assert!(m.dirty);
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::Normal,
            "Esc in vi outer Normal must NOT enter ConfirmDiscard");
        // And another, and another — always safe in vi.
        m.update(key(KeyCode::Esc));
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::Normal);
        // The cleared field stays cleared.
        assert_eq!(m.fields.text(FieldId::MainEmail), "");
    }

    #[test]
    fn vi_esc_clears_pending_key() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('d')));   // pending = Some('d')
        m.update(key(KeyCode::Esc));          // safe — clear pending
        // Next 'd' should be a fresh first half, not complete a dd.
        m.update(key(KeyCode::Char('d')));
        assert!(!m.fields.text(FieldId::MainEmail).is_empty(),
            "single d after Esc must not clear the field");
    }

    #[test]
    fn emacs_esc_still_prompts_when_dirty() {
        // Default/emacs mode keeps the original confirm-discard behaviour.
        let mut m = model_with_valid_config();
        m.dirty = true;
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::ConfirmDiscard,
            "Esc in default mode when dirty should still prompt");
    }

    #[test]
    fn vi_dd_from_outer_normal_clears_focused_field() {
        // The user-facing flow: open TUI (Mode::Normal) and press `dd` —
        // this should clear the currently-focused field, just like vim's
        // `dd` deletes the current line regardless of cursor position.
        let mut m = model_vi();
        assert_eq!(m.mode, Mode::Normal);
        assert!(!m.fields.text(FieldId::MainEmail).is_empty(),
            "precondition: field has loaded data");
        m.update(key(KeyCode::Char('d')));     // first 'd' — buffered
        m.update(key(KeyCode::Char('d')));     // second 'd' — fires
        assert_eq!(m.fields.text(FieldId::MainEmail), "",
            "dd from outer Normal should clear the focused field");
        assert!(m.dirty, "clearing the field should mark the form dirty");
    }

    #[test]
    fn vi_dd_outer_works_after_navigation() {
        // The exact sequence the user reported: dd, then j/k navigation,
        // then dd again on a different field. Verify each step.
        let mut m = model_vi();
        // dd #1
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "");
        assert_eq!(m.mode, Mode::Normal, "should remain in outer Normal after dd");
        // j must advance focus
        m.update(key(KeyCode::Char('j')));
        assert_eq!(m.focus, FieldId::MainPassword,
            "j after dd MUST advance focus");
        // k must move it back
        m.update(key(KeyCode::Char('k')));
        assert_eq!(m.focus, FieldId::MainEmail,
            "k after dd MUST move focus back");
        // Tab moves to Server screen
        m.update(key(KeyCode::Tab));
        assert_eq!(m.current_screen, Screen::Server);
        // dd #2 on the new field
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::ServerBind), "",
            "dd should clear ServerBind after navigating");
    }

    // -----------------------------------------------------------------------
    // Yank / delete / paste (paste_buffer)
    // -----------------------------------------------------------------------

    #[test]
    fn vi_dd_populates_paste_buffer() {
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        assert!(!original.is_empty());
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.paste_buffer, original,
            "dd should copy field text into paste buffer");
    }

    #[test]
    fn vi_yy_populates_paste_buffer_without_clearing() {
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('y')));
        m.update(key(KeyCode::Char('y')));
        assert_eq!(m.paste_buffer, original,
            "yy should copy field text to paste buffer");
        assert_eq!(m.fields.text(FieldId::MainEmail), original,
            "yy should NOT clear the field");
        assert!(!m.dirty, "yy should NOT mark the form dirty");
    }

    #[test]
    fn vi_p_pastes_into_empty_field_at_position_0() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('y')));
        m.update(key(KeyCode::Char('y')));
        let yanked = m.paste_buffer.clone();
        // Move to MonitorEmail (empty by default) via two j's
        m.update(key(KeyCode::Char('j')));
        m.update(key(KeyCode::Char('j')));
        assert_eq!(m.focus, FieldId::MonitorEmail);
        m.update(key(KeyCode::Char('p')));
        // Pasting into an empty field is the same as the buffer contents.
        assert_eq!(m.fields.text(FieldId::MonitorEmail), yanked);
    }

    #[test]
    fn vi_p_inserts_at_cursor_pushing_highlighted_char_right() {
        // Field has "abc"; cursor at col 0 (highlighting 'a'); buffer "XY"; press p.
        // The highlighted character shifts right to make room — result "XYabc".
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abc");
        m.paste_buffer = "XY".to_string();
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 0;
        }
        m.update(key(KeyCode::Char('p')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "XYabc",
            "p should insert AT the cursor, pushing the highlighted char right");
        let cur_col = m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col;
        assert_eq!(cur_col, 1, "cursor should be on the last char of pasted region ('Y')");
    }

    #[test]
    fn vi_capital_p_acts_same_as_p() {
        // For our single-line fields, p and P are aliases.
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abc");
        m.paste_buffer = "XY".to_string();
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 1; // on 'b'
        }
        m.update(key(KeyCode::Char('P')));
        // 'b' shifts right, 'XY' takes its place — "aXYbc".
        assert_eq!(m.fields.text(FieldId::MainEmail), "aXYbc");
    }

    #[test]
    fn vi_p_does_not_replace_existing_content() {
        // Regression for the user's complaint: paste must NOT overwrite.
        // Cursor on '@' in "doug@heroic.net"; paste "X"; @ shifts right.
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "doug@heroic.net");
        m.paste_buffer = "X".to_string();
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 4; // on '@'
        }
        m.update(key(KeyCode::Char('p')));
        // '@' shifts right; 'X' takes its place at col 4.
        assert_eq!(m.fields.text(FieldId::MainEmail), "dougX@heroic.net",
            "p must shift the highlighted char right, not replace the field");
    }

    // -----------------------------------------------------------------------
    // Undo (u, :u, :undo)
    // -----------------------------------------------------------------------

    #[test]
    fn vi_u_undoes_dd() {
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "");
        m.update(key(KeyCode::Char('u')));
        assert_eq!(m.fields.text(FieldId::MainEmail), original,
            "u should restore the field text deleted by dd");
    }

    #[test]
    fn vi_u_undoes_paste() {
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abc");
        m.paste_buffer = "X".to_string();
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 1;
        }
        m.update(key(KeyCode::Char('p')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "aXbc",
            "precondition: paste inserted at cursor");
        m.update(key(KeyCode::Char('u')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "abc",
            "u should restore the pre-paste text");
    }

    #[test]
    fn vi_u_with_empty_history_shows_error() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('u')));
        assert!(m.status.is_error,
            "u with no history should set an error status");
        assert!(m.status.message.to_lowercase().contains("undo"));
    }

    #[test]
    fn vi_multiple_undo_levels() {
        let mut m = model_vi();
        // Three successive dd operations on different fields.
        let mail_orig = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));        // dd #1: clear MainEmail
        m.update(key(KeyCode::Char('j')));         // → MainPassword (empty)
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));        // dd #2: no-op (already empty), but still pushes
        m.update(key(KeyCode::Char('k')));         // back to MainEmail (empty after #1)
        m.paste_buffer = "Z".to_string();          // override buffer
        m.update(key(KeyCode::Char('p')));         // paste 'Z' into MainEmail

        assert_eq!(m.fields.text(FieldId::MainEmail), "Z");

        // Three undos walk back through the operations.
        m.update(key(KeyCode::Char('u'))); // undo paste → ""
        assert_eq!(m.fields.text(FieldId::MainEmail), "");
        m.update(key(KeyCode::Char('u'))); // undo dd#2 → "" (still empty)
        m.update(key(KeyCode::Char('u'))); // undo dd#1 → mail_orig
        assert_eq!(m.fields.text(FieldId::MainEmail), mail_orig,
            "successive undos should walk the stack");
    }

    #[test]
    fn vi_colon_u_undoes_via_command_mode() {
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "");
        // :u Enter
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('u')));
        m.update(key(KeyCode::Enter));
        assert_eq!(m.fields.text(FieldId::MainEmail), original);
    }

    #[test]
    fn vi_colon_undo_alias_works() {
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char(':')));
        for c in "undo".chars() { m.update(key(KeyCode::Char(c))); }
        m.update(key(KeyCode::Enter));
        assert_eq!(m.fields.text(FieldId::MainEmail), original);
    }

    #[test]
    fn vi_dw_in_unified_normal_deletes_word() {
        // dw inside a field must still delete a word (handled by edtui).
        // Our 'd' interception buffers; if the next key isn't 'd', the buffered
        // 'd' is forwarded to edtui along with the second key.
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "hello world here");
        // Enter the field via 'a', Esc to Normal — cursor at end.
        m.update(key(KeyCode::Char('a')));
        m.update(key(KeyCode::Esc));
        // Move cursor to start of "world" via h/l (or 0 + l's). Use 0 if edtui
        // supports it; otherwise just go to start with multiple h then forward.
        // For the test, set cursor directly at the start of the second word.
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 6; // on 'w' of "world"
        }
        // dw deletes from cursor to start of next word (deletes "world ").
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('w')));
        let result = m.fields.text(FieldId::MainEmail);
        assert!(result.starts_with("hello "),
            "dw should preserve the prefix; got {result:?}");
        assert!(result.contains("here"),
            "dw should not delete the word after the next; got {result:?}");
        assert!(!result.contains("world"),
            "dw should have removed 'world'; got {result:?}");
    }

    #[test]
    fn vi_de_in_unified_normal_deletes_to_end_of_word() {
        // de deletes from cursor to end of current word (inclusive).
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "hello world here");
        m.update(key(KeyCode::Char('a')));
        m.update(key(KeyCode::Esc));
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 6; // on 'w' of "world"
        }
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('e')));
        let result = m.fields.text(FieldId::MainEmail);
        assert!(!result.contains("world"),
            "de should remove 'world'; got {result:?}");
        assert!(result.contains("hello"),
            "de should preserve 'hello'; got {result:?}");
        assert!(result.contains("here"),
            "de should leave 'here' intact; got {result:?}");
    }

    #[test]
    fn vi_dd_still_works_after_dw_change() {
        // Make sure adding the 'd' lookahead didn't break the dd path.
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abc");
        m.update(key(KeyCode::Char('a')));
        m.update(key(KeyCode::Esc));
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "");
        assert_eq!(m.paste_buffer, "abc");
    }

    #[test]
    fn vi_p_in_unified_normal_uses_model_paste_buffer() {
        // Regression: the user reported that after dd at outer Normal, then
        // navigating to a field, entering it via 'a', appending text, pressing
        // Esc to enter unified Normal, then pressing 'p', they got OS clipboard
        // content instead of the dd'd value. Our model paste_buffer must win.
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "doug@mhost.com");
        // 1. dd at outer Normal
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.paste_buffer, "doug@mhost.com");
        // 2. Tab to Server screen
        m.update(key(KeyCode::Tab));
        assert_eq!(m.current_screen, Screen::Server);
        assert_eq!(m.focus, FieldId::ServerBind);
        // 3. Append a space (enter Insert via 'a', type space, Esc to Normal)
        m.update(key(KeyCode::Char('a')));
        m.update(key(KeyCode::Char(' ')));
        m.update(key(KeyCode::Esc));
        // editor is now in EditorMode::Normal, mode is still Editing
        assert_eq!(m.mode, Mode::Editing);
        let bind_with_space = m.fields.text(FieldId::ServerBind);
        assert!(bind_with_space.ends_with(' '));
        // 4. Press p — must paste from OUR paste_buffer, not OS clipboard.
        m.update(key(KeyCode::Char('p')));
        let bind_after = m.fields.text(FieldId::ServerBind);
        assert!(bind_after.contains("doug@mhost.com"),
            "p in unified Normal must use the model paste_buffer (got: {bind_after})");
    }

    #[test]
    fn vi_undo_jumps_focus_to_undone_field() {
        // Even if the user has navigated away, undo focuses the restored field.
        let mut m = model_vi();
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));               // dd on MainEmail
        m.update(key(KeyCode::Tab));                     // → Server screen
        assert_eq!(m.current_screen, Screen::Server);
        m.update(key(KeyCode::Char('u')));
        assert_eq!(m.current_screen, Screen::Accounts,
            "undo should jump back to the screen owning the undone field");
        assert_eq!(m.focus, FieldId::MainEmail);
    }

    #[test]
    fn vi_space_acts_as_l_alias() {
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abcdef");
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 0;
        }
        m.update(key(KeyCode::Char(' ')));
        assert_eq!(m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col, 1,
            "space should advance cursor right (alias for l)");
        m.update(key(KeyCode::Char(' ')));
        assert_eq!(m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col, 2);
        // And screens should NOT have switched.
        assert_eq!(m.current_screen, Screen::Accounts);
    }

    #[test]
    fn vi_ddjp_moves_data_between_fields() {
        // The user's exact reported workflow: dd cuts, j moves down, p pastes.
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        assert!(!original.is_empty());
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "");
        m.update(key(KeyCode::Char('j')));
        assert_eq!(m.focus, FieldId::MainPassword);
        m.update(key(KeyCode::Char('p')));
        assert_eq!(m.fields.text(FieldId::MainPassword), original,
            "ddjp should move text from one field to the next");
    }

    #[test]
    fn vi_p_with_empty_buffer_is_safe() {
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        assert!(m.paste_buffer.is_empty());
        // Move to MonitorEmail (empty by default in model_vi)
        m.update(key(KeyCode::Char('j')));
        m.update(key(KeyCode::Char('j')));
        m.update(key(KeyCode::Char('p')));
        // Buffer was empty — current field should be unchanged.
        assert!(!m.dirty, "p with empty buffer should not dirty the form");
        // And the original field is untouched.
        assert_eq!(m.fields.text(FieldId::MainEmail), original);
    }

    #[test]
    fn vi_y_then_other_key_does_not_overwrite_buffer() {
        let mut m = model_vi();
        m.paste_buffer = "previous".to_string();
        m.update(key(KeyCode::Char('y')));
        m.update(key(KeyCode::Char('j'))); // not the second 'y'
        assert_eq!(m.paste_buffer, "previous",
            "buffer should be unchanged when 'y' is followed by another key");
        assert_eq!(m.focus, FieldId::MainPassword, "j should still navigate");
    }

    #[test]
    fn vi_d_then_other_key_does_not_clear_field() {
        // Pressing 'd' followed by something other than 'd' should NOT
        // clear the field — the pending key is discarded.
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('j')));     // not the second 'd'
        assert_eq!(m.fields.text(FieldId::MainEmail), original,
            "field should remain intact when 'd' is followed by another key");
        // The 'j' should still navigate normally
        assert_eq!(m.focus, FieldId::MainPassword);
    }

    #[test]
    fn vi_dd_in_insert_mode_just_types_dd() {
        // If the user forgets the Esc and presses dd while still in Insert,
        // the chars are inserted as text — this is the symptom of "dd doesn't work".
        let mut m = model_vi();
        let original = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('a')));   // Insert at end (NO Esc)
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), format!("{original}dd"),
            "in Insert mode, dd just types two letters");
    }

    #[test]
    fn vi_dd_after_typing_in_insert_then_esc_clears() {
        // Edit text in Insert, then Esc to Normal, then dd: should clear everything.
        let mut m = model_vi();
        m.update(key(KeyCode::Char('a')));   // Insert at end
        m.update(key(KeyCode::Char('Z')));   // type Z (modify the field)
        m.update(key(KeyCode::Esc));         // Insert → Normal
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "",
            "dd should clear the entire field, including just-typed chars");
    }

    #[test]
    fn vi_dd_keeps_clear_after_esc_exit() {
        // The core regression: dd on a pre-existing value, then Esc to exit,
        // must not restore the original value.
        let mut m = model_vi();
        assert!(!m.fields.text(FieldId::MainEmail).is_empty(), "precondition: field has data");
        m.update(key(KeyCode::Char('a')));      // enter field (Insert, cursor at end)
        m.update(key(KeyCode::Esc));            // Insert → Normal
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));      // dd: clear field
        assert_eq!(m.fields.text(FieldId::MainEmail), "", "dd should have cleared the field");
        // Exit via Esc — must NOT revert
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::Normal, "should have exited editing");
        assert_eq!(m.fields.text(FieldId::MainEmail), "",
            "Esc after dd must not restore original value");
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

    #[test]
    fn vi_dd_via_outer_a_entry() {
        // Mirror the exact user workflow: open TUI, press 'a' to enter
        // the first field, press Esc (Insert → Normal), then 'dd'.
        let mut m = model_vi();
        assert_eq!(m.focus, FieldId::MainEmail);
        // 'a' from outer Normal → enter_editing(true) → Insert mode
        m.update(key(KeyCode::Char('a')));
        assert_eq!(m.mode, Mode::Editing);
        {
            let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
            assert_eq!(ed.mode, EditorMode::Insert, "after 'a' should be Insert");
        }
        // Esc: Insert → Normal (stay in Mode::Editing)
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::Editing, "still editing after first Esc");
        {
            let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
            assert_eq!(ed.mode, EditorMode::Normal, "after Esc should be Normal");
        }
        // dd should clear the field
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "",
            "dd via 'a' entry should clear the field");
    }

    #[test]
    fn vi_dd_clears_field_and_leaves_valid_state() {
        let mut m = model_vi();
        // Enter the field in Normal mode (via Enter → Insert, then Esc → Normal)
        m.update(key(KeyCode::Enter));             // Insert mode
        m.update(key(KeyCode::Esc));               // edtui Insert → Normal
        // dd should clear the field
        m.update(key(KeyCode::Char('d')));
        m.update(key(KeyCode::Char('d')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "",
            "dd should clear the field");
        // Field should still be editable — pressing 'i' then typing works
        m.update(key(KeyCode::Char('i')));         // back to Insert
        m.update(key(KeyCode::Char('a')));
        assert_eq!(m.fields.text(FieldId::MainEmail), "a",
            "field should accept input after dd");
    }

    // -----------------------------------------------------------------------
    // vi outer navigation (STEP-02.2)
    // -----------------------------------------------------------------------

    // -----------------------------------------------------------------------
    // Unified Normal mode — navigation while inside a field (STEP-02.2 fix)
    // -----------------------------------------------------------------------

    #[test]
    fn vi_j_in_editor_normal_exits_field_and_navigates() {
        // Reproduces the user-reported bug: after one Esc (Insert→Normal),
        // j should navigate without requiring a second Esc.
        let mut m = model_vi();
        m.update(key(KeyCode::Char('a')));  // enter field in Insert mode
        m.update(key(KeyCode::Esc));        // Insert → Normal (stay in Mode::Editing)
        assert_eq!(m.mode, Mode::Editing);
        m.update(key(KeyCode::Char('j')));  // should navigate, not require 2nd Esc
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.focus, FieldId::MainPassword, "j should move to next field");
    }

    #[test]
    fn vi_k_in_editor_normal_navigates_backward() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('j')));  // → MainPassword
        m.update(key(KeyCode::Char('a')));  // enter Insert
        m.update(key(KeyCode::Esc));        // → Normal within field
        m.update(key(KeyCode::Char('k')));
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.focus, FieldId::MainEmail);
    }

    #[test]
    fn vi_tab_in_editor_normal_changes_screen() {
        // Tab from unified Normal exits the field and switches to next screen.
        let mut m = model_vi();
        m.update(key(KeyCode::Char('a')));
        m.update(key(KeyCode::Esc));
        m.update(key(KeyCode::Tab));
        assert_eq!(m.mode, Mode::Normal);
        assert_eq!(m.current_screen, Screen::Server);
    }

    #[test]
    fn vi_colon_in_editor_normal_enters_command_mode() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('a')));
        m.update(key(KeyCode::Esc));
        m.update(key(KeyCode::Char(':')));
        assert!(matches!(m.mode, Mode::VimCommand { .. }));
    }

    #[test]
    fn vi_j_in_editor_normal_keeps_edits() {
        // Navigating away via j should not discard changes made in Insert mode.
        let mut m = model_vi();
        m.update(key(KeyCode::Char('a')));  // Insert at end
        m.update(key(KeyCode::Char('X')));  // type X
        m.update(key(KeyCode::Esc));        // → Normal
        let value_before_nav = m.fields.text(FieldId::MainEmail);
        m.update(key(KeyCode::Char('j')));  // navigate
        assert!(value_before_nav.ends_with('X'), "edits must persist after navigating with j");
    }

    #[test]
    fn vi_j_advances_focus() {
        let mut m = model_vi();
        assert_eq!(m.focus, FieldId::MainEmail);
        m.update(key(KeyCode::Char('j')));
        assert_eq!(m.focus, FieldId::MainPassword);
    }

    #[test]
    fn vi_k_moves_focus_backward() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('j'))); // → MainPassword
        assert_eq!(m.focus, FieldId::MainPassword);
        m.update(key(KeyCode::Char('k')));
        assert_eq!(m.focus, FieldId::MainEmail);
    }

    #[test]
    fn vi_l_moves_cursor_right_within_field() {
        // h/l now move the cursor inside the focused field rather than
        // switching screens (Tab/Shift-Tab handle screens).
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abcdef");
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 0;
        }
        m.update(key(KeyCode::Char('l')));
        assert_eq!(m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col, 1);
        m.update(key(KeyCode::Char('l')));
        assert_eq!(m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col, 2);
        // Screen should NOT have changed.
        assert_eq!(m.current_screen, Screen::Accounts);
    }

    #[test]
    fn vi_h_moves_cursor_left_within_field() {
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abcdef");
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 3;
        }
        m.update(key(KeyCode::Char('h')));
        assert_eq!(m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col, 2);
        // Boundary: cursor cannot go below 0.
        for _ in 0..10 {
            m.update(key(KeyCode::Char('h')));
        }
        assert_eq!(m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col, 0);
    }

    #[test]
    fn vi_l_clamps_cursor_to_last_char() {
        let mut m = model_vi();
        m.fields.set_text(FieldId::MainEmail, "abc");
        if let Some(ed) = m.fields.get_editor_mut(FieldId::MainEmail) {
            ed.cursor.col = 0;
        }
        for _ in 0..20 {
            m.update(key(KeyCode::Char('l')));
        }
        // Normal-mode cursor stops at the last character (len - 1).
        assert_eq!(m.fields.get_editor(FieldId::MainEmail).unwrap().cursor.col, 2);
    }

    #[test]
    fn vi_tab_switches_to_next_screen() {
        let mut m = model_vi();
        assert_eq!(m.current_screen, Screen::Accounts);
        m.update(key(KeyCode::Tab));
        assert_eq!(m.current_screen, Screen::Server);
    }

    #[test]
    fn vi_shift_tab_switches_to_prev_screen() {
        let mut m = model_vi();
        m.update(key(KeyCode::Tab));        // → Server
        m.update(shift_backtab());          // ← Accounts
        assert_eq!(m.current_screen, Screen::Accounts);
    }

    #[test]
    fn vi_i_enters_editing_at_field_start() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('i')));
        assert_eq!(m.mode, Mode::Editing);
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Insert);
        assert_eq!(ed.cursor.col, 0, "i should position cursor at start");
    }

    #[test]
    fn vi_a_enters_editing_at_field_end() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('a')));
        assert_eq!(m.mode, Mode::Editing);
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Insert);
        let text_len = m.fields.text(FieldId::MainEmail).len();
        assert_eq!(ed.cursor.col, text_len, "a should position cursor at end");
    }

    #[test]
    fn vi_capital_a_is_alias_for_a() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('A')));
        assert_eq!(m.mode, Mode::Editing);
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Insert);
        let text_len = m.fields.text(FieldId::MainEmail).len();
        assert_eq!(ed.cursor.col, text_len);
    }

    #[test]
    fn vi_z_alone_does_not_fire() {
        let mut m = model_vi();
        let action = m.update(key(KeyCode::Char('Z')));
        assert!(matches!(action, Action::None), "single Z should not fire");
        assert_eq!(m.mode, Mode::Normal, "single Z should not leave Normal mode");
    }

    fn shift_char(c: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT))
    }

    #[test]
    fn vi_zz_saves_when_shift_modifier_is_set() {
        // Most terminals send uppercase letters with SHIFT as a modifier.
        // Our match arms expect plain Char('Z'); the update() pipeline must
        // strip SHIFT from char keys so this works in real-world terminals.
        let mut m = model_vi();
        m.dirty = true;
        m.update(shift_char('Z'));
        let action = m.update(shift_char('Z'));
        assert!(matches!(action, Action::Save),
            "ZZ with SHIFT modifier must still save");
    }

    #[test]
    fn vi_zq_quits_when_shift_modifier_is_set() {
        let mut m = model_vi();
        m.dirty = true;
        m.update(shift_char('Z'));
        let action = m.update(shift_char('Q'));
        assert!(matches!(action, Action::DiscardConfirmed),
            "ZQ with SHIFT modifier must still quit-without-saving");
    }

    #[test]
    fn vi_capital_a_works_with_shift_modifier() {
        // Same fix should make `A` (append at end) work even when SHIFT is set.
        let mut m = model_vi();
        let action = m.update(shift_char('A'));
        assert!(matches!(action, Action::None));
        assert_eq!(m.mode, Mode::Editing,
            "Shift+A should enter editing mode");
        let ed = m.fields.get_editor(FieldId::MainEmail).unwrap();
        assert_eq!(ed.mode, EditorMode::Insert);
    }

    #[test]
    fn vi_zz_saves() {
        let mut m = model_vi();
        m.dirty = true;
        m.update(key(KeyCode::Char('Z')));
        let action = m.update(key(KeyCode::Char('Z')));
        assert!(matches!(action, Action::Save));
    }

    #[test]
    fn vi_zq_quits_without_saving() {
        let mut m = model_vi();
        m.dirty = true;
        m.update(key(KeyCode::Char('Z')));
        let action = m.update(key(KeyCode::Char('Q')));
        assert!(matches!(action, Action::DiscardConfirmed));
    }

    #[test]
    fn vi_z_then_non_zq_clears_buffer_and_handles_key() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char('Z')));
        // 'j' after Z should clear the pending key and advance focus
        m.update(key(KeyCode::Char('j')));
        assert_eq!(m.focus, FieldId::MainPassword, "j should still navigate");
        assert_eq!(m.mode, Mode::Normal, "should remain in Normal mode");
    }

    #[test]
    fn vi_default_bindings_still_work_in_vi_mode() {
        let mut m = model_vi();
        // Down arrow still advances focus (non-vi field-nav alias)
        m.update(key(KeyCode::Down));
        assert_eq!(m.focus, FieldId::MainPassword);
        // Ctrl+S still saves
        m.dirty = true;
        let action = m.update(ctrl(KeyCode::Char('s')));
        assert!(matches!(action, Action::Save));
    }

    // -----------------------------------------------------------------------
    // VimCommand mode (STEP-02.2)
    // -----------------------------------------------------------------------

    #[test]
    fn colon_enters_vim_command_mode() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        assert!(matches!(m.mode, Mode::VimCommand { .. }));
    }

    #[test]
    fn colon_accumulates_chars() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('w')));
        m.update(key(KeyCode::Char('q')));
        assert!(matches!(&m.mode, Mode::VimCommand { buffer } if buffer == "wq"));
    }

    #[test]
    fn backspace_removes_from_command_buffer() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('w')));
        m.update(key(KeyCode::Char('q')));
        m.update(key(KeyCode::Backspace));
        assert!(matches!(&m.mode, Mode::VimCommand { buffer } if buffer == "w"));
    }

    #[test]
    fn esc_in_command_mode_cancels() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('w')));
        m.update(key(KeyCode::Esc));
        assert_eq!(m.mode, Mode::Normal);
    }

    #[test]
    fn colon_w_enter_returns_write_only() {
        let mut m = model_vi();
        m.dirty = true;
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('w')));
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::WriteOnly));
        assert!(!m.dirty, ":w should clear dirty flag");
    }

    #[test]
    fn colon_wq_enter_returns_save() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('w')));
        m.update(key(KeyCode::Char('q')));
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::Save));
    }

    #[test]
    fn colon_x_is_alias_for_wq() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('x')));
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::Save));
    }

    #[test]
    fn colon_q_when_clean_returns_cancel() {
        let mut m = model_vi();
        assert!(!m.dirty);
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('q')));
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::Cancel));
    }

    #[test]
    fn colon_q_when_dirty_shows_error() {
        let mut m = model_vi();
        m.dirty = true;
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('q')));
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::None));
        assert!(m.status.is_error, ":q when dirty should set error status");
    }

    #[test]
    fn colon_q_bang_returns_discard_regardless_of_dirty() {
        let mut m = model_vi();
        m.dirty = true;
        m.update(key(KeyCode::Char(':')));
        m.update(key(KeyCode::Char('q')));
        m.update(key(KeyCode::Char('!')));
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::DiscardConfirmed));
    }

    #[test]
    fn unknown_command_shows_error() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        for c in "xyz".chars() { m.update(key(KeyCode::Char(c))); }
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::None));
        assert!(m.status.is_error);
        assert!(m.status.message.contains("xyz"));
    }

    #[test]
    fn empty_command_cancels_silently() {
        let mut m = model_vi();
        m.update(key(KeyCode::Char(':')));
        let action = m.update(key(KeyCode::Enter));
        assert!(matches!(action, Action::None));
        assert_eq!(m.mode, Mode::Normal);
    }

    // -----------------------------------------------------------------------
    // status_bar_content pure function (STEP-02.2)
    // -----------------------------------------------------------------------

    #[test]
    fn status_content_vi_normal_shows_nav_hint() {
        let text = status_bar_content(&Mode::Normal, None, EditingMode::Vi);
        assert!(text.contains("h/l"), "vi normal hint should mention h/l: got {text}");
        assert!(text.contains("j/k"), "vi normal hint should mention j/k: got {text}");
        assert!(text.contains(':'), "vi normal hint should mention :command: got {text}");
    }

    #[test]
    fn status_content_vi_insert_shows_insert_indicator() {
        let text = status_bar_content(&Mode::Editing, Some(EditorMode::Insert), EditingMode::Vi);
        assert_eq!(text, "-- INSERT --");
    }

    #[test]
    fn status_content_vi_editor_normal_is_blank() {
        let text = status_bar_content(&Mode::Editing, Some(EditorMode::Normal), EditingMode::Vi);
        assert!(text.is_empty(), "vi editor Normal should produce empty status, got: {text}");
    }

    #[test]
    fn status_content_vi_visual_shows_visual_indicator() {
        let text = status_bar_content(&Mode::Editing, Some(EditorMode::Visual), EditingMode::Vi);
        assert_eq!(text, "-- VISUAL --");
    }

    #[test]
    fn status_content_vim_command_shows_colon_buffer() {
        let text = status_bar_content(
            &Mode::VimCommand { buffer: "wq".to_string() },
            None,
            EditingMode::Vi,
        );
        assert_eq!(text, ":wq");
    }

    #[test]
    fn status_content_default_normal_shows_tab_hint() {
        let text = status_bar_content(&Mode::Normal, None, EditingMode::Default);
        assert!(text.contains("Tab"), "default hint should mention Tab: got {text}");
        assert!(text.contains("Ctrl-S") || text.contains("Ctrl"), "default hint should mention save key: got {text}");
    }

    #[test]
    fn status_content_emacs_editing_shows_editing_hint() {
        let text = status_bar_content(&Mode::Editing, None, EditingMode::Emacs);
        assert!(text.contains("Enter"), "emacs editing hint should mention Enter: got {text}");
        assert!(text.contains("Esc"), "emacs editing hint should mention Esc: got {text}");
    }
}
