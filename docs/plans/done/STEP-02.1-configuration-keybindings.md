# Step 02.1 — Configuration TUI: vi (and emacs) key bindings

## Goal

Make every editable text field in `ranchero configure` respond to vi key
bindings (normal/insert/visual modal editing) as the primary enhancement, with
emacs key bindings as a bonus. The active editing mode is chosen by:

1. **Config file** `[tui] editing_mode = "vi" | "emacs" | "default"` — highest
   priority.
2. **`~/.editrc`** — if the file contains `bind -v`, default to vi; if it
   contains `bind -e`, default to emacs. Parsed at startup, before the TUI
   opens.
3. **Built-in default** — `"default"` (current behaviour: Emacs-style, since
   that is crossterm's natural key model).

## Library decision

### `edtui` (https://github.com/preiter93/edtui, v0.7.1)

`edtui` provides `EditorState` (text buffer + cursor + vi/emacs mode) and
`EditorView` (ratatui widget that renders the buffer). It supports the full
vi command set — Normal / Insert / Visual modes, motions, operators, undo/redo
— and also has an emacs mode. It is the right tool for this job.

**Ratatui compatibility:** edtui targets ratatui `0.30`. We are currently on
`0.29`. This step begins with a minor upgrade (`0.29 → 0.30`) before adding
edtui. The 0.29 → 0.30 bump is a patch-level API change; no breaking changes
are expected for the widget surface we use (Layout, Block, Paragraph, Tabs,
Spans, TestBackend). Verify with `cargo test` after the bump before proceeding.

### `editline` — not used

`editline` is a readline-style single-line editor for synchronous CLI loops.
It manages its own terminal raw mode and event loop, which conflicts directly
with ratatui's draw/event model. It provides no widget integration and no
ratatui compatibility. Discarded.

## Architecture changes

### Per-field `EditorState`

Replace the `Fields` struct's bare `String` values for text fields with
`EditorState` from edtui. Each text field owns its editing state:

```rust
// src/tui/model.rs
use edtui::{EditorState, EditorMode};

pub struct Fields {
    pub main_email:       EditorState,
    pub main_password:    EditorState,   // rendered as *** regardless of mode
    pub monitor_email:    EditorState,
    pub monitor_password: EditorState,
    pub server_bind:      EditorState,
    pub server_port:      EditorState,   // input filtered to digits only
    pub server_https:     bool,          // toggled, not text-edited
    pub log_level:        EditorState,
    pub log_file:         EditorState,
    pub pid_file:         EditorState,
}
```

`EditorState::get_text()` returns the current string value for serialisation
and validation.

### `EditingMode` and model-level mode control

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditingMode { #[default] Default, Vi, Emacs }
```

Added to `Model`:

```rust
pub struct Model {
    ...
    pub editing_mode: EditingMode,
}
```

When the model is constructed, `editing_mode` is set from the resolved config
(which already incorporates the `~/.editrc` default). Each `EditorState` is
initialised to the matching edtui mode:

```rust
fn make_editor(mode: EditingMode) -> EditorState {
    let mut s = EditorState::default();
    match mode {
        EditingMode::Vi    => s.set_mode(EditorMode::Normal),
        EditingMode::Emacs => s.set_mode(EditorMode::Insert),  // emacs is always-insert
        EditingMode::Default => s.set_mode(EditorMode::Insert),
    }
    s
}
```

### Event routing in `Model::update`

In `Mode::Editing`, instead of our hand-rolled char-push/backspace handler,
pass the crossterm `Event` to `edtui`'s event handler for the focused field:

```rust
Mode::Editing => {
    // Let edtui consume the event for the focused EditorState.
    // edtui returns whether it handled the event.
    let handled = self.fields.get_editor_mut(self.focus)
        .map(|ed| ed.on_event(&ev))
        .unwrap_or(false);

    // Intercept Esc at normal-mode boundary to leave Editing:
    // in Vi mode, the first Esc takes us from Insert → Normal (edtui handles);
    // a second Esc (already in Normal) or Esc in Emacs mode exits Editing.
    if !handled {
        if let Event::Key(KeyEvent { code: KeyCode::Esc, .. }) = ev {
            self.mode = Mode::Normal;
            self.validate();
        }
    }
    Action::None
}
```

For `Mode::Normal` (our screen-level normal mode, distinct from vi's normal
mode), existing navigation keys are unchanged. A Vi-mode field shows `[N]` or
`[I]` in its border title to indicate which vi mode the editor is in.

### Digit-only enforcement for numeric fields

`EditorState` will accept any character. For `ServerPort`, validate in
`Model::validate()` as before (port must be numeric), and additionally intercept
non-digit input in the event handler before forwarding to edtui.

### Password rendering

`EditorState` stores the plaintext; `view.rs` renders `"*".repeat(len)` and
never passes the actual buffer to the `EditorView` widget for password fields.
The `EditorView` is used for all other fields.

## `~/.editrc` parsing

New module `src/config/editrc.rs`:

```rust
pub enum EditrcMode { Vi, Emacs }

/// Parse ~/.editrc and return the global editing mode if specified.
/// Ignores program-scoped lines (e.g. `prog:bind -v`).
/// Returns None if the file is absent or contains no relevant directive.
pub fn detect_from_editrc(home: &Path) -> Option<EditrcMode>;
```

Parse rules (from the editrc(5) man page):
- Lines beginning with `#` are comments.
- A line of the form `bind -v` sets vi mode globally.
- A line of the form `bind -e` sets emacs mode globally.
- Program-scoped lines (`prog:bind -v`) are ignored — we are not `libedit`.
- The **last** matching global directive wins (consistent with libedit
  behaviour).

## Config schema extension

Add to `ranchero.toml`:

```toml
[tui]
editing_mode = "default"   # "vi" | "emacs" | "default"
```

`TuiConfig` struct in `src/config/mod.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct TuiConfig {
    #[serde(default)]
    pub editing_mode: EditingModeConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EditingModeConfig { #[default] Default, Vi, Emacs }
```

`ResolvedConfig` grows a `tui_editing_mode: EditingMode` field, resolved as:

```
CLI --editing-mode (future) > config file [tui].editing_mode > ~/.editrc detect > Default
```

## Ratatui upgrade notes

Before adding edtui:

1. Update `Cargo.toml`: `ratatui = "0.30"`.
2. Run `cargo test` — the 0.29 → 0.30 bump is minor; breakage is unlikely
   but verify all 79 existing tests pass before proceeding.
3. Re-run tests green, then add `edtui = "0.7"` and proceed.

## Tests first

### `src/config/editrc.rs` unit tests

1. `editrc_absent_returns_none` — no file → `None`.
2. `bind_v_detected_as_vi` — file contains `bind -v` → `Some(Vi)`.
3. `bind_e_detected_as_emacs` — file contains `bind -e` → `Some(Emacs)`.
4. `program_scoped_bind_ignored` — `prog:bind -v` → `None`.
5. `last_directive_wins` — file has `bind -e` then `bind -v` → `Some(Vi)`.
6. `comments_ignored` — `# bind -v` is a comment → `None`.
7. `mixed_file_respects_global_only` — mix of scoped and global → global wins.

### `src/config/mod.rs` unit tests (extend existing)

8. `editing_mode_config_vi_overrides_editrc_emacs` — config `[tui] editing_mode
   = "vi"` wins over `~/.editrc bind -e`.
9. `editing_mode_default_falls_back_to_editrc` — config default + `~/.editrc
   bind -v` → resolved mode is Vi.
10. `editing_mode_default_with_no_editrc` → resolved mode is Default.

### `src/tui/model.rs` unit tests (extend existing)

11. `vi_mode_starts_in_normal_state` — model built with `Vi` has `EditorMode::Normal`
    on each field's `EditorState`.
12. `emacs_mode_starts_in_insert_state` — model built with `Emacs` has
    `EditorMode::Insert`.
13. `vi_i_enters_insert_then_esc_returns_to_normal` — feed `i`, type chars,
    `Esc`: field is in normal mode with typed text saved.
14. `vi_dd_clears_field` — in normal mode, `dd` empties the field string.
15. `vi_A_appends_to_end` — `A` moves cursor to end and enters insert mode.
16. `emacs_ctrl_a_moves_to_start` — in emacs mode, `Ctrl-A` moves cursor to
    position 0.
17. `emacs_ctrl_k_kills_to_end_of_line` — text after cursor is deleted.
18. `esc_in_vi_normal_exits_editing_mode` — a second Esc (already in vi normal)
    returns our `Mode` to `Normal`.
19. `esc_in_emacs_mode_exits_editing_mode` — single Esc exits.
20. `numeric_field_ignores_non_digit_in_vi_insert` — typing `x` in server_port
    field (insert mode) is rejected before reaching edtui.

### `tests/tui.rs` integration tests (extend existing)

21. `configure_tui_reads_editing_mode_from_resolved_config` — model created
    from a `ResolvedConfig` with `Vi` uses vi mode on all editors.
22. `editrc_vi_mode_propagates_end_to_end` — fake home with `~/.editrc`
    containing `bind -v`; resolved config has no override → model uses Vi.

### `tests/config.rs` integration tests (extend existing)

23. `editrc_file_round_trip` — write `bind -v` to a tempfile, call
    `detect_from_editrc`, assert `Some(Vi)`.

## Acceptance criteria

- `cargo test` green, `cargo clippy -- -D warnings` clean.
- `ranchero configure` with no config and no `~/.editrc` behaves as before
  (cursor key editing, Enter to commit, Esc to cancel).
- With `bind -v` in `~/.editrc`, the same command opens with vi normal mode;
  `i` enters insert, `Esc` returns to normal, `dd` clears a field.
- With `[tui] editing_mode = "emacs"` in the config file, `Ctrl-A` / `Ctrl-E`
  / `Ctrl-K` work on all text fields.
- Both modes correctly exclude passwords from TOML output (existing invariant).
- Existing 79 tests continue to pass.

## Deferred

- `--editing-mode` CLI flag (would sit above config in the precedence chain).
- Visual-mode selection for log-level (a bounded enum field would be better
  served by an arrow-key selector widget than by text editing).
- Mouse cursor positioning within fields (edtui supports it; wire once stable).
- Syntax highlighting in the Review screen's TOML preview (edtui supports it
  but requires a grammar definition).
