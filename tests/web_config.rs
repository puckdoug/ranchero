// SPDX-License-Identifier: AGPL-3.0-only
//! 17.2-T and 17.3-T — configuration fields for the web server.
//!
//! 17.2-T: `[server] pages_root` parses from TOML, is overridden by
//!   `RANCHERO_PAGES_ROOT`, and defaults to `"pages"` when absent.
//!
//! 17.3-T: `[server] https_cert_dir` parses from TOML and defaults to
//!   `"https"` when absent.
//!
//! Both fail to compile until `server_pages_root` and
//! `server_https_cert_dir` exist on `ResolvedConfig`.
//!
//! See `docs/plans/STEP-17-web-server.md`, items 17.2-T and 17.3-T.

use std::collections::HashMap;
use std::path::PathBuf;

use ranchero::config::{ConfigFile, Env, ResolvedConfig};
use ranchero::credentials::InMemoryKeyringStore;
use ranchero::cli::GlobalOpts;

// ---------------------------------------------------------------------------
// Helpers (mirror the pattern in src/config/mod.rs unit tests)
// ---------------------------------------------------------------------------

struct MapEnv(HashMap<&'static str, &'static str>);
impl Env for MapEnv {
    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).map(|s| s.to_string())
    }
}

fn empty_env() -> MapEnv { MapEnv(HashMap::new()) }
fn empty_cli() -> GlobalOpts { GlobalOpts::default() }
fn empty_keyring() -> InMemoryKeyringStore { InMemoryKeyringStore::default() }

fn resolve(file: Option<ConfigFile>, env: MapEnv) -> ResolvedConfig {
    ResolvedConfig::resolve(&empty_cli(), &env, &empty_keyring(), file)
        .expect("resolve must succeed")
}

// ---------------------------------------------------------------------------
// 17.2-T — pages_root
// ---------------------------------------------------------------------------

/// The `[server] pages_root` TOML field surfaces as
/// `server_pages_root: PathBuf` on `ResolvedConfig`.
#[test]
fn pages_root_parses_from_toml() {
    let toml = "\
        schema_version = 1\n\
        [server]\n\
        pages_root = \"/srv/pages\"\n\
    ";
    let file: ConfigFile = toml::from_str(toml).expect("toml parse");
    let r = resolve(Some(file), empty_env());
    assert_eq!(r.server_pages_root, PathBuf::from("/srv/pages"));
}

/// `RANCHERO_PAGES_ROOT` overrides the TOML value, following the
/// standard CLI → env → file precedence for all RANCHERO_* variables.
#[test]
fn pages_root_env_overrides_file() {
    let toml = "\
        schema_version = 1\n\
        [server]\n\
        pages_root = \"/srv/pages\"\n\
    ";
    let file: ConfigFile = toml::from_str(toml).expect("toml parse");
    let env = MapEnv(HashMap::from([("RANCHERO_PAGES_ROOT", "/env/pages")]));
    let r = resolve(Some(file), env);
    assert_eq!(r.server_pages_root, PathBuf::from("/env/pages"));
}

/// When neither TOML nor env sets `pages_root`, the resolved value is
/// `"pages"` (a path relative to the binary's working directory).
#[test]
fn pages_root_default_is_pages() {
    let r = resolve(None, empty_env());
    assert_eq!(r.server_pages_root, PathBuf::from("pages"));
}

// ---------------------------------------------------------------------------
// 17.3-T — https_cert_dir
// ---------------------------------------------------------------------------

/// The `[server] https_cert_dir` TOML field surfaces as
/// `server_https_cert_dir: PathBuf` on `ResolvedConfig`.
#[test]
fn https_cert_dir_parses_from_toml() {
    let toml = "\
        schema_version = 1\n\
        [server]\n\
        https_cert_dir = \"/etc/certs\"\n\
    ";
    let file: ConfigFile = toml::from_str(toml).expect("toml parse");
    let r = resolve(Some(file), empty_env());
    assert_eq!(r.server_https_cert_dir, PathBuf::from("/etc/certs"));
}

/// When TOML does not set `https_cert_dir`, the resolved value is
/// `"https"` (a path relative to the binary's working directory).
#[test]
fn https_cert_dir_default_is_https() {
    let r = resolve(None, empty_env());
    assert_eq!(r.server_https_cert_dir, PathBuf::from("https"));
}
