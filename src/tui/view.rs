use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Tabs},
};

use super::model::{FieldId, Model, Mode, Screen};

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
        render_help_overlay(frame, area);
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
    let is_editing = is_focused && model.mode == Mode::Editing;
    let value = model.fields.get(field);
    let display = if field.is_password() {
        "*".repeat(value.len())
    } else {
        value.to_string()
    };

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

    let content = if is_editing {
        format!("{display}█") // cursor
    } else {
        display
    };

    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL).title(title).border_style(border_style));
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
    let dirty_mark = if model.dirty { " [*]" } else { "" };
    let text = format!("{}{dirty_mark}", model.status.message);
    let style = if model.status.is_error {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let paragraph = Paragraph::new(Span::styled(text, style));
    frame.render_widget(paragraph, area);
}

fn render_help_overlay(frame: &mut Frame, area: Rect) {
    let help_lines = vec![
        Line::from("Keybindings"),
        Line::from(""),
        Line::from("  Tab / Shift-Tab   Move focus"),
        Line::from("  Ctrl-→ / Ctrl-←   Next / previous screen"),
        Line::from("  Enter             Edit focused field"),
        Line::from("  Esc               Cancel edit / quit"),
        Line::from("  Ctrl-S            Save"),
        Line::from("  ?                 Toggle this help"),
    ];

    let w = 50u16.min(area.width);
    let h = (help_lines.len() as u16 + 2).min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    let popup_area = Rect::new(x, y, w, h);

    frame.render_widget(Clear, popup_area);
    let block = Block::default().borders(Borders::ALL).title("Help");
    let paragraph = Paragraph::new(help_lines).block(block);
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
        // tab to password field, enter editing, type a password
        m.update(crossterm::event::Event::Key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)));
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
        let len = m.fields.main_email.len();
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
