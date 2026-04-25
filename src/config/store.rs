use std::path::PathBuf;

use super::{atomic_write, ConfigError, ConfigFile};

pub trait ConfigStore {
    fn load(&self) -> Result<Option<ConfigFile>, ConfigError>;
    fn save(&mut self, cfg: &ConfigFile) -> Result<(), ConfigError>;
    fn path(&self) -> &std::path::Path;
}

pub struct FileConfigStore {
    path: PathBuf,
}

impl FileConfigStore {
    pub fn new(path: PathBuf) -> Self { Self { path } }
}

impl ConfigStore for FileConfigStore {
    fn load(&self) -> Result<Option<ConfigFile>, ConfigError> {
        if !self.path.exists() {
            return Ok(None);
        }
        let cfg = super::load(Some(&self.path))?;
        Ok(Some(cfg))
    }

    fn save(&mut self, cfg: &ConfigFile) -> Result<(), ConfigError> {
        let contents = toml::to_string_pretty(cfg)
            .map_err(|e| ConfigError::ParseError {
                path: self.path.clone(),
                message: e.to_string(),
            })?;
        // atomic_write creates the parent directory; after that, wire the XDG symlink.
        atomic_write(&self.path, contents.as_bytes())?;
        if let Some(dir) = self.path.parent() {
            super::paths::ensure_xdg_symlink(dir);
        }
        Ok(())
    }

    fn path(&self) -> &std::path::Path { &self.path }
}
