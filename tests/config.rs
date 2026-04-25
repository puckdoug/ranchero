use std::io::Write;

use ranchero::config::{self, ConfigError, atomic_write};
#[cfg(target_os = "macos")]
use ranchero::config::paths::create_xdg_symlink;

// ---------------------------------------------------------------------------
// atomic_write
// ---------------------------------------------------------------------------

#[test]
fn atomic_write_creates_file_with_content() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.toml");
    atomic_write(&p, b"hello = true\n").unwrap();
    let got = std::fs::read_to_string(&p).unwrap();
    assert_eq!(got, "hello = true\n");
}

#[test]
fn atomic_write_does_not_leave_tmp_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("out.toml");
    atomic_write(&p, b"x = 1\n").unwrap();
    assert!(!p.with_extension("toml.tmp").exists(), ".tmp file should be renamed away");
}

#[test]
fn atomic_write_creates_parent_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a/b/c/out.toml");
    atomic_write(&p, b"ok = true\n").unwrap();
    assert!(p.exists());
}

// ---------------------------------------------------------------------------
// config::load — filesystem-touching tests
// ---------------------------------------------------------------------------

#[test]
fn unknown_schema_version_errors() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("ranchero.toml");
    writeln!(std::fs::File::create(&p).unwrap(), "schema_version = 99").unwrap();
    let err = config::load(Some(&p)).unwrap_err();
    assert!(matches!(err, ConfigError::UnknownSchemaVersion(99)));
}

#[test]
fn malformed_toml_errors_with_path_in_message() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("ranchero.toml");
    writeln!(std::fs::File::create(&p).unwrap(), "[[[[bad toml").unwrap();
    let err = config::load(Some(&p)).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains(p.to_str().unwrap()),
        "error should mention the file path; got: {msg}"
    );
}

#[test]
fn config_path_flag_loads_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("alt.toml");
    writeln!(
        std::fs::File::create(&p).unwrap(),
        "schema_version = 1\n[server]\nport = 7777"
    )
    .unwrap();
    let cfg = config::load(Some(&p)).unwrap();
    assert_eq!(cfg.server.port, 7777);
}

#[test]
fn config_missing_at_explicit_path_errors() {
    let p = std::path::Path::new("/does/not/exist/ranchero.toml");
    let err = config::load(Some(p)).unwrap_err();
    assert!(matches!(err, ConfigError::MissingFile(_)));
}

#[test]
fn config_file_store_round_trips() {
    use ranchero::config::store::{ConfigStore, FileConfigStore};

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("ranchero.toml");
    let mut store = FileConfigStore::new(p.clone());

    // No file yet → returns None
    assert!(store.load().unwrap().is_none());

    // Save a config
    let mut cfg = ranchero::config::ConfigFile::default();
    cfg.server.port = 4242;
    store.save(&cfg).unwrap();

    // Load it back
    let loaded = store.load().unwrap().unwrap();
    assert_eq!(loaded.server.port, 4242);

    // Written atomically — no .tmp file left over
    assert!(!p.with_extension("toml.tmp").exists());
}

// ---------------------------------------------------------------------------
// XDG symlink (macOS only)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
#[test]
fn xdg_symlink_points_to_real_dir() {
    let root = tempfile::tempdir().unwrap();
    let real_dir = root.path().join("net.heroic.ranchero");
    let fake_home = root.path().join("home");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::create_dir_all(&fake_home).unwrap();

    create_xdg_symlink(&real_dir, &fake_home).unwrap();

    let link = fake_home.join(".config/ranchero");
    assert!(link.is_symlink(), ".config/ranchero should be a symlink");
    assert_eq!(
        std::fs::read_link(&link).unwrap().canonicalize().unwrap(),
        real_dir.canonicalize().unwrap(),
        "symlink target should be the real config dir"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn xdg_symlink_creation_is_idempotent() {
    let root = tempfile::tempdir().unwrap();
    let real_dir = root.path().join("net.heroic.ranchero");
    let fake_home = root.path().join("home");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::create_dir_all(&fake_home).unwrap();

    create_xdg_symlink(&real_dir, &fake_home).unwrap();
    // Second call must not error
    create_xdg_symlink(&real_dir, &fake_home).unwrap();

    let link = fake_home.join(".config/ranchero");
    assert!(link.is_symlink());
}

#[cfg(target_os = "macos")]
#[test]
fn xdg_symlink_not_created_when_real_dir_already_exists() {
    let root = tempfile::tempdir().unwrap();
    let real_dir = root.path().join("net.heroic.ranchero");
    let fake_home = root.path().join("home");
    std::fs::create_dir_all(&real_dir).unwrap();

    // Pre-create .config/ranchero as a real directory (e.g. pre-existing setup)
    let xdg_path = fake_home.join(".config/ranchero");
    std::fs::create_dir_all(&xdg_path).unwrap();

    // Should not error and should not replace the real directory
    create_xdg_symlink(&real_dir, &fake_home).unwrap();
    assert!(!xdg_path.is_symlink(), "real directory should not be replaced with a symlink");
    assert!(xdg_path.is_dir(), "real directory should still exist");
}

#[cfg(target_os = "macos")]
#[test]
fn xdg_dot_config_parent_created_if_missing() {
    let root = tempfile::tempdir().unwrap();
    let real_dir = root.path().join("net.heroic.ranchero");
    // fake_home has no .config subdir yet
    let fake_home = root.path().join("home");
    std::fs::create_dir_all(&real_dir).unwrap();
    std::fs::create_dir_all(&fake_home).unwrap();

    create_xdg_symlink(&real_dir, &fake_home).unwrap();

    assert!(fake_home.join(".config").is_dir(), ".config should be created");
    assert!(fake_home.join(".config/ranchero").is_symlink());
}
