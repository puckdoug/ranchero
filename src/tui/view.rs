use edtui::EditorMode;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
};

use super::model::{status_bar_content, FieldId, Model, Mode, Screen};
use crate::config::EditingMode;

const SCREEN_TITLES: [&str; 5] = ["Accounts", "Server", "Logging", "Daemon", "Review"];

pub fn render(model: &Model, frame: &mut Frame) {
    let area = frame.area();

    // Outer layout: tabs | content | status bar
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // tabs
            Constraint::Min(0),     // content
            Constraint::Length(1),  // status
        ])
        .split(area);

    render_tabs(model, frame, chunks[0]);
    render_screen(model, frame, chunks[1]);
    render_status(model, frame, chunks[2]);

    if model.mode == Mode::Help {
        render_help_overlay(model, frame, area);
    }
}

fn screen_index(screen: Screen) -> usize {
    Screen::ALL.iter().position(|s| *s == screen).unwrap_or(0)
}

fn render_tabs(model: &Model, frame: &mut Frame, area: Rect) {
    let titles: Vec<Line> = SCREEN_TITLES.iter().map(|t| Line::from(*t)).collect();
    let tabs = Tabs::new(titles)
        .block(Block::default().borders(Borders::BOTTOM))
        .select(screen_index(model.current_screen))
        .style(Style::default())
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, area);
}

fn render_screen(model: &Model, frame: &mut Frame, area: Rect) {
    match model.current_screen {
        Screen::Accounts => render_accounts(model, frame, area),
        Screen::Server   => render_server(model, frame, area),
        Screen::Logging  => render_logging(model, frame, area),
        Screen::Daemon   => render_daemon(model, frame, area),
        Screen::Review   => render_review(model, frame, area),
    }
}

fn render_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    field: FieldId,
    model: &Model,
) {
    let is_focused = model.focus == field;

    let border_style = if is_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let title = if let Some(err) = model.validation.error_for(field) {
        format!("{label} ⚠ {err}")
    } else {
        label.to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style);

    let value = model.fields.text(field);
    let display: Vec<char> = if field.is_password() {
        "*".repeat(value.chars().count()).chars().collect()
    } else {
        value.chars().collect()
    };

    // Show the cursor on any focused field — not just when in `Mode::Editing`.
    // Style depends on the editor's current mode:
    //   - Insert  → terminal bar cursor (frame.set_cursor_position)
    //   - else    → block cursor (reversed-colour character)
    let line = if is_focused {
        let col = model.fields.get_editor(field)
            .map(|ed| ed.cursor.col.min(display.len()))
            .unwrap_or(display.len());
        let editor_mode = model.fields.get_editor(field).map(|ed| ed.mode);
        let in_insert = model.mode == Mode::Editing
            && editor_mode == Some(EditorMode::Insert);

        if in_insert {
            // Insert mode: render text unchanged and position the terminal's
            // own cursor (a blinking vertical bar) at the insertion point.
            frame.set_cursor_position(Position::new(
                area.x + 1 + col as u16,
                area.y + 1,
            ));
            Line::raw(display.iter().collect::<String>())
        } else {
            // Block cursor: render the character at `col` with reversed colours.
            // No characters are inserted — text positions are preserved.
            let before: String = display[..col].iter().collect();
            let cursor_char = if col < display.len() {
                display[col].to_string()
            } else {
                " ".to_string() // block past end-of-text
            };
            let after: String = if col < display.len() {
                display[col + 1..].iter().collect()
            } else {
                String::new()
            };
            Line::from(vec![
                Span::raw(before),
                Span::styled(cursor_char, Style::default().add_modifier(Modifier::REVERSED)),
                Span::raw(after),
            ])
        }
    } else {
        Line::raw(display.iter().collect::<String>())
    };

    let paragraph = Paragraph::new(line).block(block);
    frame.render_widget(paragraph, area);
}

fn render_accounts(model: &Model, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    render_field(frame, chunks[0], "Main account email", FieldId::MainEmail, model);
    render_field(frame, chunks[1], "Main account password", FieldId::MainPassword, model);
    render_field(frame, chunks[2], "Monitor account email", FieldId::MonitorEmail, model);
    render_field(frame, chunks[3], "Monitor account password", FieldId::MonitorPassword, model);
}

fn render_server(model: &Model, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    render_field(frame, chunks[0], "Bind address", FieldId::ServerBind, model);
    render_field(frame, chunks[1], "Port", FieldId::ServerPort, model);
    render_field(frame, chunks[2], "HTTPS (Enter to toggle)", FieldId::ServerHttps, model);
}

fn render_logging(model: &Model, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_field(frame, chunks[0], "Log level (trace|debug|info|warn|error)", FieldId::LogLevel, model);
    render_field(frame, chunks[1], "Log file", FieldId::LogFile, model);
}

fn render_daemon(model: &Model, frame: &mut Frame, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    render_field(frame, chunks[0], "PID file", FieldId::PidFile, model);
}

fn render_review(model: &Model, frame: &mut Frame, area: Rect) {
    let cfg = model.to_config_file();
    let toml = toml::to_string_pretty(&cfg).unwrap_or_else(|_| "error".to_string());
    let block = Block::default().borders(Borders::ALL).title("Config preview (passwords omitted)");
    let paragraph = Paragraph::new(toml).block(block);
    frame.render_widget(paragraph, area);
}

fn render_status(model: &Model, frame: &mut Frame, area: Rect) {
    // Left zone: mode indicator (or error message), derived from model state.
    let focused_editor_mode = model.fields.get_editor(model.focus).map(|e| e.mode);
    let (left_text, is_error) = if model.status.is_error {
        (model.status.message.clone(), true)
    } else {
        (status_bar_content(&model.mode, focused_editor_mode, model.editing_mode), false)
    };

    let left_style = if is_error {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Right zone: dirty indicator.
    let right_text = if model.dirty { "[*]" } else { "" };

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(right_text.len() as u16)])
        .split(area);

    frame.render_widget(
        Paragraph::new(Span::styled(left_text, left_style)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(right_text, Style::default().fg(Color::DarkGray))),
        chunks[1],
    );
}

fn help_lines_for_mode(editing_mode: EditingMode) -> Vec<Line<'static>> {
    match editing_mode {
        EditingMode::Vi => vec![
            Line::from("Keybindings (vi)"),
            Line::from(""),
            Line::from("  Tab / Shift-Tab    Next / previous screen"),
            Line::from("  j / k              Next / previous field"),
            Line::from("  h / l              Cursor left / right within field"),
            Line::from("  i                  Edit field (insert at start)"),
            Line::from("  a / A              Edit field (append at end)"),
            Line::from("  dd                 Cut focused field to paste buffer"),
            Line::from("  yy                 Yank focused field to paste buffer"),
            Line::from("  p / P              Paste at cursor (shifts text right)"),
            Line::from("  u  :u  :undo       Undo last destructive change"),
            Line::from("  :w                 Save"),
            Line::from("  :wq   ZZ           Save and quit"),
            Line::from("  :q!   ZQ           Quit without saving"),
            Line::from("  :q                 Quit (fails if unsaved changes)"),
            Line::from("  Esc                Cancel / return to Normal"),
            Line::from("  ?                  Toggle this help"),
        ],
        _ => vec![
            Line::from("Keybindings"),
            Line::from(""),
            Line::from("  Tab / Shift-Tab    Next / previous screen"),
            Line::from("  \u{2191} / \u{2193}              Next / previous field"),
            Line::from("  Enter              Edit focused field"),
            Line::from("  Esc                Cancel edit / quit"),
            Line::from("  Ctrl-S             Save"),
            Line::from("  ?                  Toggle this help"),
        ],
    }
}

fn render_help_overlay(model: &Model, frame: &mut Frame, area: Rect) {
    let lines = help_lines_for_mode(model.editing_mode);
    let w = 55u16.min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, popup_area);
    let block = Block::default().borders(Borders::ALL).title("Help");
    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, popup_area);
}

// ---------------------------------------------------------------------------
// TestBackend-driven rendering tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};
    use super::*;
    use crate::config::ConfigFile;
    use crate::tui::model::Model;

    fn render_to_buffer(model: &Model, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(model, f)).unwrap();
        terminal.backend().buffer().clone()
    }

    fn model_accounts() -> Model {
        let mut cfg = ConfigFile::default();
        cfg.accounts.main.email = Some("r@example.com".to_string());
        cfg.accounts.monitor.email = Some("m@example.com".to_string());
        Model::new(cfg)
    }

    #[test]
    fn accounts_screen_shows_main_email() {
        let m = model_accounts();
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains("r@example.com"), "main email not found in rendered output:\n{content}");
    }

    #[test]
    fn accounts_screen_shows_monitor_email() {
        let m = model_accounts();
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains("m@example.com"), "monitor email not found:\n{content}");
    }

    #[test]
    fn password_field_renders_as_asterisks() {
        use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};
        let mut m = model_accounts();
        // Down to password field, enter editing, type a password
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        for c in "secret".chars() {
            m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
        }
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(!content.contains("secret"), "plaintext password must not be visible: {content}");
        assert!(content.contains("******"), "asterisks should appear for password: {content}");
    }

    #[test]
    fn server_screen_shows_port_and_bind() {
        use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};
        let mut m = model_accounts();
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)));
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains("1080") || content.contains("127.0.0.1"),
            "server screen should show port or bind: {content}");
    }

    #[test]
    fn validation_error_shown_next_to_invalid_field() {
        use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};
        let mut m = model_accounts();
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
        // Clear and type invalid email
        let len = m.fields.text(super::super::model::FieldId::MainEmail).len();
        for _ in 0..len {
            m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)));
        }
        for c in "notanemail".chars() {
            m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)));
        }
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains('⚠'), "validation error marker should appear: {content}");
    }

    #[test]
    fn dirty_indicator_in_status_bar() {
        let mut m = model_accounts();
        m.dirty = true;
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains("[*]"), "dirty indicator should appear: {content}");
    }

    #[test]
    fn help_overlay_lists_keybindings() {
        use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};
        let mut m = model_accounts();
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)));
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains("Tab") || content.contains("Keybindings"),
            "help overlay should list keybindings: {content}");
    }

    // -----------------------------------------------------------------------
    // Status bar rendering tests (STEP-02.2)
    // -----------------------------------------------------------------------

    fn model_vi_editing_insert() -> crate::tui::model::Model {
        use crate::config::{ConfigFile, EditingModeConfig};
        let mut cfg = ConfigFile::default();
        cfg.accounts.main.email = Some("r@example.com".to_string());
        cfg.tui.editing_mode = EditingModeConfig::Vi;
        let mut m = crate::tui::model::Model::new(cfg);
        // Enter editing via 'i' (Insert at start)
        m.update(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('i'), crossterm::event::KeyModifiers::NONE)
        ));
        m
    }

    fn model_vi_editing_editor_normal() -> crate::tui::model::Model {
        let mut m = model_vi_editing_insert();
        // Esc: Insert → Normal (stay in Mode::Editing)
        m.update(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Esc, crossterm::event::KeyModifiers::NONE)
        ));
        m
    }

    fn model_vi_command(buf: &str) -> crate::tui::model::Model {
        use crate::config::{ConfigFile, EditingModeConfig};
        let mut cfg = ConfigFile::default();
        cfg.tui.editing_mode = EditingModeConfig::Vi;
        let mut m = crate::tui::model::Model::new(cfg);
        m.update(crossterm::event::Event::Key(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char(':'), crossterm::event::KeyModifiers::NONE)
        ));
        for c in buf.chars() {
            m.update(crossterm::event::Event::Key(
                crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char(c), crossterm::event::KeyModifiers::NONE)
            ));
        }
        m
    }

    #[test]
    fn rendered_status_bar_shows_insert_in_vi_insert_mode() {
        let m = model_vi_editing_insert();
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains("-- INSERT --"),
            "status bar should show -- INSERT -- when vi field is in Insert mode:\n{content}");
    }

    #[test]
    fn rendered_status_bar_blank_in_vi_editor_normal_mode() {
        let m = model_vi_editing_editor_normal();
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        let last_line: &str = content_last_line(&content);
        assert!(!last_line.contains("INSERT"),
            "status bar should not show INSERT in vi Normal mode:\n{last_line}");
        assert!(!last_line.contains("NORMAL"),
            "status bar should not show NORMAL in vi Normal mode:\n{last_line}");
    }

    #[test]
    fn rendered_status_bar_shows_colon_buffer_in_command_mode() {
        let m = model_vi_command("wq");
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        let last_line = content_last_line(&content);
        assert!(last_line.contains(":wq"),
            "status bar should show :wq in command mode:\n{last_line}");
    }

    #[test]
    fn rendered_dirty_flag_in_right_zone() {
        let mut m = model_accounts();
        m.dirty = true;
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        let last_line = content_last_line(&content);
        // [*] should appear — not necessarily far right, but present
        assert!(last_line.contains("[*]"),
            "dirty indicator [*] should appear in status bar:\n{last_line}");
    }

    #[test]
    fn rendered_dirty_flag_absent_when_clean() {
        let m = model_accounts();
        assert!(!m.dirty);
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        let last_line = content_last_line(&content);
        assert!(!last_line.contains("[*]"),
            "dirty indicator should not appear when model is clean:\n{last_line}");
    }

    #[test]
    fn help_overlay_vi_mode_shows_vi_bindings() {
        use crate::config::{ConfigFile, EditingModeConfig};
        use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};
        let mut cfg = ConfigFile::default();
        cfg.tui.editing_mode = EditingModeConfig::Vi;
        let mut m = crate::tui::model::Model::new(cfg);
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)));
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains(":wq") || content.contains("ZZ"),
            "vi help overlay should mention :wq or ZZ:\n{content}");
    }

    #[test]
    fn help_overlay_default_mode_shows_tab_and_ctrl_s() {
        use crossterm::event::{KeyCode, KeyModifiers, KeyEvent};
        let mut m = model_accounts(); // default mode
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Char('?'), KeyModifiers::NONE)));
        let buf = render_to_buffer(&m, 80, 30);
        let content = buffer_to_string(&buf);
        assert!(content.contains("Tab") || content.contains("Ctrl-S"),
            "default help overlay should mention Tab or Ctrl-S:\n{content}");
    }

    fn content_last_line(content: &str) -> &str {
        content.lines().rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
    }

    fn buffer_to_string(buf: &ratatui::buffer::Buffer) -> String {
        let area = buf.area;
        let mut out = String::new();
        for y in area.top()..area.bottom() {
            for x in area.left()..area.right() {
                out.push_str(buf.cell((x, y)).map(|c| c.symbol()).unwrap_or(" "));
            }
            out.push('\n');
        }
        out
    }
}
