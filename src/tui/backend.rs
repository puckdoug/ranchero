/// Thin abstraction over ratatui's Terminal so driver tests can inject a scripted
/// event stream without a real terminal.
///
/// In production, `RatatuiBackend` wraps `ratatui::Terminal<CrosstermBackend<Stdout>>`.
/// In tests, `ScriptedBackend` feeds a Vec<Event> and records draw calls.
use crossterm::event::Event;

pub trait TerminalBackend {
    /// Draw one frame, returning Ok(()) on success.
    fn draw<F>(&mut self, f: F) -> std::io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame);

    /// Poll for the next event. Returns None when the scripted event queue is
    /// exhausted (tests) or blocks indefinitely (production).
    fn next_event(&mut self) -> std::io::Result<Option<Event>>;
}
