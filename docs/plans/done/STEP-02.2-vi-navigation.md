# Step 02.2 — Vi-mode outer navigation

**Status:** ☑ Complete

## Goal

When `editing_mode = "vi"`, extend vi key bindings from the
field-editing layer (STEP 02.1) to the outer screen-navigation
layer, so that the entire `ranchero configure` workflow is
navigable using familiar vi key sequences:

- Tab / Shift-Tab to move between screens (the visible "tabs").
- `j`/`k` to move between fields within a screen.
- `h`/`l` (and Space as alias for `l`) to move the cursor within the focused field.
- `i`/`a`/`A` to enter edit mode; Esc to leave it.
- `dd`/`yy`/`p`/`P` for cut/yank/paste with a cross-field paste buffer.
- `dw`/`db`/`de` for word-wise deletes inside a field.
- `u`/`:u`/`:undo` for undo of model-level operations.
- `:wq`, `:q!`, `ZZ`, `ZQ` for save/quit.
- Status bar and help overlay show vi-aware hints.

Default (emacs-compatible) bindings remain available as aliases throughout.

---

## State machine additions

### `Mode` enum

```rust
pub enum Mode {
    Normal,
    Editing,
    ConfirmDiscard,
    Help,
    VimCommand { buffer: String },   // accumulates characters after ':'
}
```

`VimCommand` is reachable from any vi Normal context. Its `buffer`
is rendered in the status bar as the user types (`:wq_`).

### `Action` enum

```rust
pub enum Action {
    None,
    Save,              // save and close
    WriteOnly,         // save without closing; clear the dirty flag (`:w`)
    Cancel,
    DiscardConfirmed,
}
```

The driver loop treats `WriteOnly` identically to `Save` for the
storage step, but it continues the event loop rather than
returning.

### `Model` fields

```rust
pub struct Model {
    pub current_screen: Screen,
    pub focus: FieldId,
    pub fields: Fields,
    pub validation: ValidationReport,
    pub status: StatusBar,
    pub dirty: bool,
    pub mode: Mode,
    pub editing_mode: EditingMode,

    /// First character of a two-key vi sequence (`Z` awaiting
    /// `Z` or `Q`; `d` awaiting a motion or `d`; `y` awaiting a
    /// motion or `y`). Cleared on any non-continuing keypress.
    pending_key: Option<char>,

    /// Persistent edtui handler. Stored on the model so that
    /// multi-key sequences (`dd`, `dw`, `de`, and similar)
    /// accumulate state across individual keypresses. Reset by
    /// `enter_editing`.
    editor_handler: EditorEventHandler,

    /// Vi paste register. Populated by `dd` and `yy`, consumed by
    /// `p` and `P`. Persists across fields so that `ddjp`-style
    /// movement is possible.
    paste_buffer: String,

    /// Undo history for model-level operations (`dd`, `p`, `P`).
    /// Each entry stores the field's text and cursor position
    /// before the operation. `u`, `:u`, and `:undo` pop entries
    /// from the stack and restore the previous state.
    undo_stack: Vec<UndoEntry>,

    initial_texts: HashMap<FieldId, String>,
}

#[derive(Clone, Debug)]
struct UndoEntry {
    field: FieldId,
    text: String,
    cursor_col: usize,
}
```

---

## Key binding table

### Outer Normal mode (`Mode::Normal`, vi)

| Key | Action |
|---|---|
| `Tab` / `Shift-Tab` | Next / previous screen (the visible tabs) |
| `Ctrl-→` / `Ctrl-←` | Same as Tab / Shift-Tab (alias) |
| `j` / `k` | Next / previous field within the current screen |
| `↓` / `↑` | Same as `j`/`k` (non-vi alias, also active in default mode) |
| `h` / `l` | Move cursor left / right within the focused field |
| `Space` | Alias for `l` |
| `i` | Enter editing mode, cursor at **start** of field |
| `a` / `A` | Enter editing mode, cursor at **end** of field |
| `Enter` | Same as `a` (or toggle for boolean fields like HTTPS) |
| `dd` | Cut focused field to `paste_buffer`; clear field; push undo |
| `yy` | Yank focused field to `paste_buffer` (no clear, no dirty) |
| `p` / `P` | Paste `paste_buffer` at the cursor (the highlighted character shifts to the right); the cursor is positioned on the last pasted character |
| `u` | Undo most recent destructive operation |
| `:` | Enter `Mode::VimCommand` |
| `ZZ` | Save and close (`Action::Save`) |
| `ZQ` | Quit without saving (`Action::DiscardConfirmed`) |
| `?` | Toggle help overlay |
| `Esc`, `Ctrl-C` | No operation in vi mode; never raises ConfirmDiscard. The user must quit explicitly. |
| `Ctrl-S` | Save (alias, all modes) |

### Unified Normal — inside a field but in `EditorMode::Normal` (after Esc from Insert)

This is the same Normal mode as outer; navigation, paste, undo, and
quit commands all behave identically. Differences:

- Pressing `j`/`k`/`Tab`/`Shift-Tab` exits `Mode::Editing` first, then performs the navigation.
- All vi text-object motions (`h`, `l`, `b`, `w`, `e`, `0`, `$`, etc.) and operators
  (`x`, `X`, `D`, `dd`, `dw`, `db`, `de`) operate **within** the field via edtui.

### Editing — `Mode::Editing` + `EditorMode::Insert` (vi)

Edtui handles all character insertion, backspace, and similar
operations. The following keys are intercepted by this layer:

| Key | Effect |
|---|---|
| `Enter` | Commit and exit `Mode::Editing` (vi: cursor stays at end) |
| `Esc` | Edtui transitions Insert → Normal (stays in `Mode::Editing`) |

In emacs/default mode, `Esc` from Insert reverts the field and exits
editing immediately (cancel semantics).

### Editing — `Mode::Editing` + `EditorMode::Normal` (vi sub-mode)

| Key | Effect |
|---|---|
| `Esc` | Exit `Mode::Editing` to outer Normal. Does not revert: vi Normal-mode edits are permanent, so `dd`, `dw`, and similar operations remain in effect. |
| `i`/`a`/`A`/`I` | Re-enter Insert mode (edtui) |

### Command mode (`Mode::VimCommand`)

| Input | Action |
|---|---|
| `:w` Enter | Write — `Action::WriteOnly`, dirty cleared, status: "Saved." |
| `:wq` Enter, `:x` Enter | Write-quit — `Action::Save` |
| `:q` Enter | Quit — `Action::Cancel` if clean; status error if dirty |
| `:q!` Enter | Force-quit — `Action::DiscardConfirmed` |
| `:u` Enter, `:undo` Enter | Pop the model undo stack |
| Esc | Cancel — clear buffer, return to `Mode::Normal` |
| Backspace | Remove the last character from the buffer |
| Unknown Enter | Status error: `unknown command: <buf>` |

### Modifier normalisation

Most terminals send uppercase letters as `KeyCode::Char('Z')`
with `KeyModifiers::SHIFT` set. The match arms in this layer
expect a bare `Char(_)`. The `update()` entry strips `SHIFT` from
`Char(_)` events before dispatch, so that `ZZ`, `ZQ`, `A`, `P`,
`?`, and similar keys work uniformly. Non-character keys (for
example, `Shift+BackTab`) retain their modifiers.

---

## Edtui customisation

The default edtui vim handler binds `dd`, `D`, `x`, and `X`, but
not `dw`, `db`, or `de`. The `make_editor_handler` function
extends the handler:

```rust
let mut handler = KeyEventHandler::vim_mode();
handler.insert(KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('w')]),
               DeleteWordForward(1));
handler.insert(KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('b')]),
               DeleteWordBackward(1));
// `de` = visual select to end of word, delete selection, back to normal.
handler.insert(KeyEventRegister::n(vec![KeyInput::new('d'), KeyInput::new('e')]),
               SwitchMode(Visual)
                   .chain(MoveWordForwardToEndOfWord(1))
                   .chain(DeleteSelection)
                   .chain(SwitchMode(Normal)));
EditorEventHandler::new(handler)
```

The `arboard` feature is disabled in the `edtui` dependency
(`default-features = false` in `Cargo.toml`). When enabled,
edtui's paste reads from the operating-system clipboard, which
conflicts with the model's `paste_buffer` semantics and produces
unexpected results.

### `d` and `y` lookahead in unified Normal

The model intercepts the first `d` or `y` of every
operator-motion sequence:

1. First `d` → `pending_key = Some('d')`, with no immediate
   effect.
2. Second key:
   - `d` → the model's own `dd` (clears the field, populates
     `paste_buffer`, pushes an undo entry).
   - Any other key → the buffered `d` is forwarded to edtui, and
     the second key is forwarded normally. Edtui then completes
     `dw`, `db`, `de`, and similar operator-motion sequences.

This preserves `dd`'s cross-field semantics while allowing edtui
to handle `d{motion}` natively. The `yy` sequence is handled
symmetrically.

---

## Cursor rendering

In `render_field`:

- Field not focused → plain text, no cursor.
- Field focused, `Mode::Editing` + `EditorMode::Insert` → text rendered
  unchanged, terminal cursor positioned via `frame.set_cursor_position()`
  (the terminal draws its native blinking bar).
- Field focused, any other state → block cursor: the character
  at `editor.cursor.col` is rendered with `Modifier::REVERSED`.
  No characters are inserted into the rendered string; the
  surrounding text remains in place.

The cursor is therefore visible in **outer Normal mode** as well, so
`h`/`l`/`b`/`w` movement provides clear visual feedback and `p`/`P` paste
position is unambiguous.

---

## Status bar

Two-zone layout, single bottom row:

```
-- INSERT --                                                           [*]
left: mode indicator / command buffer                          right: dirty
```

| `editing_mode` | `mode` | `EditorMode` | Left-zone content |
|---|---|---|---|
| Default / Emacs | Normal | — | `Tab/Shift-Tab: screen  ↑/↓: field  Enter: edit  Ctrl-S: save  ?: help` |
| Vi | Normal | — | `Tab: screen  j/k: field  h/l: cursor  i/a: edit  :: command  ZZ: save  ?: help` |
| Vi | VimCommand | — | `:<buffer>` |
| Vi | Editing | Insert | `-- INSERT --` |
| Vi | Editing | Normal | *(empty; vi Normal is silent, matching standard vim behaviour)* |
| Vi | Editing | Visual | `-- VISUAL --` |
| Default / Emacs | Editing | — | `Editing — Enter: confirm  Esc: cancel` |
| Any | ConfirmDiscard | — | `Unsaved changes. y: discard  n: go back` |
| Any | Help | — | *(blank — overlay covers screen)* |

Errors override the left zone with red text from `model.status.message`.

`status_bar_content(mode, editor_mode, editing_mode) -> String`
is exposed as a pure function for unit testing. The view layer
derives the indicator on each frame rather than relying on the
model to push updates.

---

## Help overlay

`help_lines_for_mode(editing_mode) -> Vec<Line<'static>>` returns the
overlay content. Activated by `?` from any Normal mode.

**Vi mode:**
```
Keybindings (vi)

  Tab / Shift-Tab    Next / previous screen
  j / k              Next / previous field
  h / l              Cursor left / right within field
  i                  Edit field (insert at start)
  a / A              Edit field (append at end)
  dd                 Cut focused field to paste buffer
  yy                 Yank focused field to paste buffer
  p / P              Paste at cursor (shifts text right)
  u  :u  :undo       Undo last destructive change
  :w                 Save
  :wq   ZZ           Save and quit
  :q!   ZQ           Quit without saving
  :q                 Quit (fails if unsaved changes)
  Esc                Cancel / return to Normal
  ?                  Toggle this help
```

**Default / Emacs mode:**
```
Keybindings

  Tab / Shift-Tab    Next / previous screen
  ↑ / ↓              Next / previous field
  Enter              Edit focused field
  Esc                Cancel edit / quit
  Ctrl-S             Save
  ?                  Toggle this help
```

---

## Paste semantics

`vi_paste_at_cursor()` inserts `paste_buffer` **at** `editor.cursor.col`:

```
text  = "abc",    cursor.col = 1 (on 'b'),    paste_buffer = "XY"
result = "aXYbc",  cursor.col = 2 (on the 'Y' — the last character of the pasted region)
```

Both `p` and `P` are routed through this single helper, because
each form field is a single line; vim's distinction between
"after current line" and "at current line" does not apply. The
character under the cursor shifts to the right to make room. This
behaviour matches the user mental model of "insert at the
cursor"; `c{motion}` and `s` remain available for replace
semantics.

The buffer persists across field-to-field navigation, so that
`ddjp` moves data between fields. `p` and `P` invoked from inside
`Mode::Editing` (after `Esc` from Insert) also read from the
model's `paste_buffer` and never from the operating-system
clipboard.

---

## Undo

`push_undo(field)` captures a snapshot of the field's text and
cursor position before any destructive operation. `dd`, `p`, and
`P` all push entries. `pop_undo()`:

1. Restores the field's text.
2. Restores the cursor position.
3. Moves focus to the restored field, switching screens if
   necessary, so that the change is visible to the user.
4. Sets `dirty = true`, runs `validate()`.
5. Reports `Nothing to undo` (red) if the stack is empty.

---

## Architecture — `:w` mid-session save

`run_loop` (driver.rs) handles `Action::WriteOnly` by calling
`store.save(&cfg)` and `save_passwords(model, keyring)`, then setting
`model.dirty = false` and continuing the event loop. The status bar
shows "Saved." until the next non-error keypress in `Mode::Normal`.

## Architecture — driver event filtering

`run_loop` skips key events whose `kind != KeyEventKind::Press`.
Some terminals (the kitty keyboard protocol, and certain macOS
configurations) emit both Press and Release events; processing
both would cause every keypress to be processed twice. The filter
resides in the driver, not in the model, so that the model never
sees these synthetic events.

---

## Tests

Below is the final test set as implemented. Counts are cumulative
to the end of STEP 02.2.

### `src/tui/model.rs` — vi navigation and operators

- `vi_j_advances_focus`, `vi_k_moves_focus_backward`
- `vi_l_moves_cursor_right_within_field`, `vi_h_moves_cursor_left_within_field`
- `vi_l_clamps_cursor_to_last_char`
- `vi_space_acts_as_l_alias`
- `vi_tab_switches_to_next_screen`, `vi_shift_tab_switches_to_prev_screen`
- `vi_i_enters_editing_at_field_start`
- `vi_a_enters_editing_at_field_end`, `vi_capital_a_is_alias_for_a`
- `vi_capital_a_works_with_shift_modifier`
- `vi_z_alone_does_not_fire`
- `vi_zz_saves`, `vi_zq_quits_without_saving`
- `vi_zz_saves_when_shift_modifier_is_set`, `vi_zq_quits_when_shift_modifier_is_set`
- `vi_z_then_non_zq_clears_buffer_and_handles_key`
- `vi_default_bindings_still_work_in_vi_mode`
- `vi_dd_clears_field_and_leaves_valid_state`
- `vi_dd_keeps_clear_after_esc_exit`
- `vi_dd_works_on_numeric_port_field`
- `vi_dd_via_outer_a_entry`
- `vi_dd_from_outer_normal_clears_focused_field`
- `vi_dd_outer_works_after_navigation`
- `vi_dd_in_insert_mode_just_types_dd`
- `vi_dd_still_works_after_dw_change`
- `vi_dw_in_unified_normal_deletes_word`
- `vi_de_in_unified_normal_deletes_to_end_of_word`
- `vi_d_then_other_key_does_not_clear_field`
- `vi_dd_populates_paste_buffer`, `vi_yy_populates_paste_buffer_without_clearing`
- `vi_p_inserts_at_cursor_pushing_highlighted_char_right`
- `vi_capital_p_acts_same_as_p`
- `vi_p_does_not_replace_existing_content`
- `vi_p_pastes_into_empty_field_at_position_0`
- `vi_p_with_empty_buffer_is_safe`
- `vi_y_then_other_key_does_not_overwrite_buffer`
- `vi_ddjp_moves_data_between_fields`
- `vi_p_in_unified_normal_uses_model_paste_buffer`

### `src/tui/model.rs` — undo

- `vi_u_undoes_dd`, `vi_u_undoes_paste`
- `vi_u_with_empty_history_shows_error`
- `vi_multiple_undo_levels`
- `vi_colon_u_undoes_via_command_mode`, `vi_colon_undo_alias_works`
- `vi_undo_jumps_focus_to_undone_field`

### `src/tui/model.rs` — Esc semantics

- `vi_esc_in_outer_normal_is_safe_when_dirty`
- `vi_esc_clears_pending_key`
- `emacs_esc_still_prompts_when_dirty`
- `vi_second_esc_exits_keeping_edits`
- `vi_esc_in_insert_transitions_to_normal_not_exits_editing`
- `vi_second_esc_exits_editing_mode`
- `escape_in_editing_mode_reverts_field` (default mode)
- `vi_enter_opens_insert_mode_at_end`

### `src/tui/model.rs` — unified Normal navigation

- `vi_j_in_editor_normal_exits_field_and_navigates`
- `vi_k_in_editor_normal_navigates_backward`
- `vi_tab_in_editor_normal_changes_screen`
- `vi_colon_in_editor_normal_enters_command_mode`
- `vi_j_in_editor_normal_keeps_edits`

### `src/tui/model.rs` — VimCommand mode

- `colon_enters_vim_command_mode`, `colon_accumulates_chars`
- `backspace_removes_from_command_buffer`
- `esc_in_command_mode_cancels`
- `colon_w_enter_returns_write_only`, `colon_wq_enter_returns_save`
- `colon_x_is_alias_for_wq`
- `colon_q_when_clean_returns_cancel`, `colon_q_when_dirty_shows_error`
- `colon_q_bang_returns_discard_regardless_of_dirty`
- `unknown_command_shows_error`, `empty_command_cancels_silently`

### `src/tui/model.rs` — non-vi navigation aliases

- `down_arrow_advances_focus_within_screen`
- `up_arrow_moves_focus_backward`
- `tab_switches_to_next_screen`, `shift_tab_switches_to_previous_screen`
- `ctrl_right_advances_screen`, `ctrl_left_moves_screen_back`

### `src/tui/model.rs` — `status_bar_content` pure function

- `status_content_vi_normal_shows_nav_hint`
- `status_content_vi_insert_shows_insert_indicator`
- `status_content_vi_editor_normal_is_blank`
- `status_content_vi_visual_shows_visual_indicator`
- `status_content_vim_command_shows_colon_buffer`
- `status_content_default_normal_shows_tab_hint`
- `status_content_emacs_editing_shows_editing_hint`

### `src/tui/view.rs` — rendering

- `accounts_screen_shows_main_email`, `accounts_screen_shows_monitor_email`
- `password_field_renders_as_asterisks`
- `server_screen_shows_port_and_bind`
- `validation_error_shown_next_to_invalid_field`
- `dirty_indicator_in_status_bar`
- `help_overlay_lists_keybindings`
- `rendered_status_bar_shows_insert_in_vi_insert_mode`
- `rendered_status_bar_blank_in_vi_editor_normal_mode`
- `rendered_status_bar_shows_colon_buffer_in_command_mode`
- `rendered_dirty_flag_in_right_zone`, `rendered_dirty_flag_absent_when_clean`
- `help_overlay_vi_mode_shows_vi_bindings`
- `help_overlay_default_mode_shows_tab_and_ctrl_s`

### `tests/tui.rs` — integration

- `vi_save_via_colon_wq`
- `vi_force_quit_via_colon_q_bang`
- `vi_write_only_clears_dirty_and_stays_open`
- `vi_insert_indicator_disappears_on_esc_to_normal`

---

## Acceptance criteria

All met:

- ✅ All tests pass (192 total at completion of this step).
- ✅ `cargo clippy --all-targets -- -D warnings` reports no warnings.
- ✅ Tab/Shift-Tab switch screens; `j`/`k` navigate fields; `h`/`l` move cursor within field.
- ✅ `i` / `a` / `A` / Enter enter Editing; cursor placed correctly (start vs end).
- ✅ `dd` / `yy` populate `paste_buffer`; `p` / `P` insert at cursor (shifts text right).
- ✅ `ddjp` moves data between fields; works regardless of OS clipboard contents.
- ✅ `u` / `:u` / `:undo` undo destructive operations; focus jumps to restored field.
- ✅ `dw` / `db` / `de` delete word forward / backward / to-end-of-word inside a field.
- ✅ `:wq`, `:q!`, `ZZ`, `ZQ`, `:w` work; uppercase keys with `KeyModifiers::SHIFT` are recognised.
- ✅ Vi `Esc` is safe — never raises ConfirmDiscard; vi Normal-mode edits are permanent.
- ✅ Cursor visible on any focused field: block cursor in Normal/Visual, terminal bar in Insert.
- ✅ Status bar shows `-- INSERT --` / `-- VISUAL --` / `:command` / vi nav hints; dirty flag right-aligned.
- ✅ Help overlay (`?`) shows mode-aware bindings.
- ✅ Default / emacs mode behaviour unchanged.

---

## Deferred

- `gg` and `G` for first / last screen.
- `0`, `$`, and `^` for line motions in outer Normal mode (only
  `h` and `l` are implemented at this stage).
- Numeric prefix counts (`3j`, `5l`, `2dd`).
- `c{motion}` and `s` (change and substitute); at present, only
  edtui's own implementations apply within Insert mode.
- Cross-field paste from edtui's per-field clipboard. When the
  user invokes `dw` inside a field, the deleted word is placed in
  edtui's clipboard rather than the outer `paste_buffer`, and so
  is not available via the outer `p`.
- Mouse click to focus a field.
- Custom `:` commands beyond `:w`, `:wq`, `:x`, `:q`, `:q!`,
  `:u`, and `:undo`.
- Redo (`Ctrl-R` / `:redo`).
