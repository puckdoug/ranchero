use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditrcMode { Vi, Emacs }

/// Parse `home/.editrc` and return the global editing mode if one is set.
///
/// Follows editrc(5): lines are `[prog:]command [args]`. We only act on
/// *global* `bind -v` / `bind -e` directives (no leading `prog:` prefix).
/// The last matching global directive wins. Returns `None` if the file is
/// absent or contains no relevant directive.
pub fn detect_from_editrc(home: &Path) -> Option<EditrcMode> {
    let path = home.join(".editrc");
    let contents = std::fs::read_to_string(&path).ok()?;
    parse_editrc(&contents)
}

fn parse_editrc(contents: &str) -> Option<EditrcMode> {
    let mut result = None;
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }
        // Skip program-scoped lines: anything containing ':' before whitespace
        let first_token = trimmed.split_whitespace().next().unwrap_or("");
        if first_token.contains(':') {
            continue;
        }
        // Match global `bind -v` or `bind -e`
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        if let ["bind", flag] = tokens.as_slice() {
            match *flag {
                "-v" => result = Some(EditrcMode::Vi),
                "-e" => result = Some(EditrcMode::Emacs),
                _ => {}
            }
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_v_detected_as_vi() {
        assert_eq!(parse_editrc("bind -v\n"), Some(EditrcMode::Vi));
    }

    #[test]
    fn bind_e_detected_as_emacs() {
        assert_eq!(parse_editrc("bind -e\n"), Some(EditrcMode::Emacs));
    }

    #[test]
    fn absent_returns_none() {
        assert_eq!(parse_editrc(""), None);
    }

    #[test]
    fn comments_ignored() {
        assert_eq!(parse_editrc("# bind -v\n"), None);
    }

    #[test]
    fn program_scoped_bind_ignored() {
        assert_eq!(parse_editrc("prog:bind -v\n"), None);
    }

    #[test]
    fn last_directive_wins_vi_after_emacs() {
        assert_eq!(parse_editrc("bind -e\nbind -v\n"), Some(EditrcMode::Vi));
    }

    #[test]
    fn last_directive_wins_emacs_after_vi() {
        assert_eq!(parse_editrc("bind -v\nbind -e\n"), Some(EditrcMode::Emacs));
    }

    #[test]
    fn mixed_scoped_and_global_respects_global() {
        let content = "prog:bind -v\nbind -e\nother:bind -v\n";
        assert_eq!(parse_editrc(content), Some(EditrcMode::Emacs));
    }

    #[test]
    fn unrelated_bind_flags_ignored() {
        assert_eq!(parse_editrc("bind -s\n"), None);
    }
}
