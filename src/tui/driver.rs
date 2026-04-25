use std::io::{self, Stdout};

use crossterm::{
    event,
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::config::ConfigError;
use crate::config::store::ConfigStore;
use crate::tui::keyring::KeyringStore;
use crate::tui::model::{Action, Model};
use crate::tui::view::render;

#[derive(Debug)]
pub enum ConfigureError {
    Config(ConfigError),
    Io(io::Error),
}

impl std::fmt::Display for ConfigureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigureError::Config(e) => write!(f, "config error: {e}"),
            ConfigureError::Io(e)     => write!(f, "I/O error: {e}"),
        }
    }
}
impl std::error::Error for ConfigureError {}
impl From<ConfigError> for ConfigureError {
    fn from(e: ConfigError) -> Self { ConfigureError::Config(e) }
}
impl From<io::Error> for ConfigureError {
    fn from(e: io::Error) -> Self { ConfigureError::Io(e) }
}

/// Run the interactive configuration TUI.
///
/// Reads the current config from `store`, starts the ratatui event loop,
/// and on save: writes the config file via `store` and passwords via `keyring`.
pub fn run_configure(
    store: &mut dyn ConfigStore,
    keyring: &mut dyn KeyringStore,
) -> Result<bool, ConfigureError> {
    let initial = store.load()?.unwrap_or_default();
    let mut model = Model::new(initial);

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let saved = run_loop(&mut terminal, &mut model, store, keyring);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    saved
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    model: &mut Model,
    store: &mut dyn ConfigStore,
    keyring: &mut dyn KeyringStore,
) -> Result<bool, ConfigureError> {
    loop {
        terminal.draw(|f| render(model, f))?;

        let ev = event::read()?;
        let action = model.update(ev);

        match action {
            Action::None => {}
            Action::Save => {
                let cfg = model.to_config_file();
                if let Err(e) = store.save(&cfg) {
                    model.status = crate::tui::model::StatusBar::error(format!("Save failed: {e}"));
                    continue;
                }
                save_passwords(model, keyring);
                return Ok(true);
            }
            Action::Cancel | Action::DiscardConfirmed => {
                return Ok(false);
            }
        }
    }
}

pub fn save_passwords(model: &Model, keyring: &mut dyn KeyringStore) {
    if !model.fields.main_email.is_empty() && !model.fields.main_password.is_empty() {
        let _ = keyring.set("main", &model.fields.main_email, &model.fields.main_password);
    }
    if !model.fields.monitor_email.is_empty() && !model.fields.monitor_password.is_empty() {
        let _ = keyring.set("monitor", &model.fields.monitor_email, &model.fields.monitor_password);
    }
}

