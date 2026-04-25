use std::path::{Path, PathBuf};

pub fn resolve_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(rest)
    } else if path == "~" {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home)
    } else {
        PathBuf::from(path)
    }
}

/// Create `<home>/.config/ranchero` as a symlink pointing to `real_dir`.
///
/// Idempotent: does nothing if the link (or any file/dir) already exists at
/// that path. Creates `<home>/.config` if it is absent.
///
/// Only meaningful on macOS, where the OS-native config directory lives under
/// `~/Library/Application Support/net.heroic.ranchero` and the XDG-style path
/// `~/.config/ranchero` is provided as a convenience alias.
#[cfg(target_os = "macos")]
pub fn create_xdg_symlink(real_dir: &Path, home: &Path) -> std::io::Result<()> {
    let xdg_link = home.join(".config/ranchero");

    // If something already exists at the link path (real dir, existing symlink,
    // or any other file), leave it untouched.
    if xdg_link.exists() || xdg_link.is_symlink() {
        return Ok(());
    }

    std::fs::create_dir_all(home.join(".config"))?;
    std::os::unix::fs::symlink(real_dir, &xdg_link)
}

/// Ensure the XDG convenience symlink exists, reading HOME from the environment.
/// No-op on non-macOS platforms.
pub fn ensure_xdg_symlink(real_dir: &Path) {
    #[cfg(target_os = "macos")]
    {
        let home = match std::env::var("HOME") {
            Ok(h) => PathBuf::from(h),
            Err(_) => return,
        };
        let _ = create_xdg_symlink(real_dir, &home);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = real_dir;
}
