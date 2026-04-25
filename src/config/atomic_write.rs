use std::io::Write;
use std::path::Path;

use super::ConfigError;

/// Write `contents` to `path` atomically: write to a sibling `.tmp` file, then rename.
/// If the write fails partway through, the original `path` is untouched.
pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_path = path.with_extension("toml.tmp");
    {
        let mut tmp = std::fs::File::create(&tmp_path)?;
        tmp.write_all(contents)?;
        tmp.sync_all()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}
