# Step 02.2 — Vi-mode outer navigation

## Goal

When `editing_mode = "vi"`, extend vi key bindings from the field-editing
layer (STEP 02.1) to the **outer screen-navigation layer** as well, so the
entire `ranchero configure` workflow is drivable with vi muscle memory:

- `j`/`k` to move between fields, `h`/`l` to move between screens.
- `i`/`a` to enter edit mode; Esc to leave it.
- `:wq`, `:q!`, `ZZ`, etc. for save/quit.
- Status bar and help overlay update to show vi bindings instead of the
  default Tab/Ctrl-S hints when vi mode is active.

Default (emacs-compatible) bindings are unchanged and remain available in
all modes as aliases.

---

## State machine additions

### New `Mode` variant

```rust
pub enum Mode {
    Normal,
    Editing,
    ConfirmDiscard,
    Help,
    VimCommand { buffer: String },   // NEW: accumulates chars after ':'
}
```

`VimCommand` renders the buffer as a command line in the status bar
(`:wq_`) and is only reachable when `editing_mode == Vi`.

### New `Model` field

```rust
pub struct Model {
    ...
    /// Buffers the first character of a two-key vi sequence (e.g. `Z` awaiting `Z`/`Q`).
    /// Cleared on every keypress that does not continue the sequence.
    pending_key: Option<char>,
}
```

`pending_key` handles `ZZ` (save-quit) and `ZQ` (quit-without-save) without
a timeout — the second key is awaited synchronously. Any other key flushes
the buffer and is re-processed normally.

---

## Key binding table

All bindings below are **vi-mode only** (active when
`model.editing_mode == EditingMode::Vi` and `model.mode == Mode::Normal`).
The existing default bindings (Tab, Ctrl-S, Ctrl-Left/Right, Esc) remain
active in all modes.

### Screen-level navigation (`Mode::Normal`)

| Key(s) | Action | Notes |
|---|---|---|
| `j` | Advance focus to next field | Same as Tab |
| `k` | Move focus to previous field | Same as Shift-Tab |
| `l` | Go to next screen (right) | Same as Ctrl-Right |
| `h` | Go to previous screen (left) | Same as Ctrl-Left |
| `i` | Enter edit mode, cursor at **start** of field | Unlike `a`, which goes to end |
| `a` | Enter edit mode, cursor at **end** of field | Mirrors existing Enter behaviour |
| `A` | Enter edit mode, cursor at **end** of field | Alias for `a` |
| `ZZ` | Save and close | Two-key sequence via `pending_key` |
| `ZQ` | Close without saving (force) | Two-key sequence; no confirm prompt |
| `:` | Enter `Mode::VimCommand` | Begins command buffer |
| `?` | Toggle help overlay | Unchanged |
| `q` | Quit (prompt if dirty) | Unchanged |

### Command mode (`Mode::VimCommand`)

| Input | Command | Action |
|---|---|---|
| `:w` Enter | Write | Save file; stay in configure |
| `:wq` Enter | Write-quit | Save and return `Action::Save` |
| `:x` Enter | Write-quit (alias) | Same as `:wq` |
| `:q` Enter | Quit | `Action::Cancel` if clean; error if dirty |
| `:q!` Enter | Force-quit | `Action::DiscardConfirmed` without prompt |
| Esc | Cancel | Clear buffer, return to `Mode::Normal` |
| Backspace | Delete last char | Removes one char from buffer |
| Unknown Enter | Error | Show "unknown command: `foo`" in status bar |

Note: `:w` (save without quit) saves to the store and clears dirty, but
keeps the TUI open. The driver loop must handle a new `Action::WriteOnly`
return (or equivalent) so saves can happen mid-session. See
**Architecture** below.

### Editing mode (`Mode::Editing`, vi sub-mode)

The behaviour here is already implemented in STEP 02.1 (edtui handles it).
For documentation completeness:

| Context | Key | Effect |
|---|---|---|
| `EditorMode::Normal` | `i` | → `EditorMode::Insert` at cursor |
| `EditorMode::Normal` | `a` | → `EditorMode::Insert` after cursor |
| `EditorMode::Normal` | `A` | → `EditorMode::Insert` at end of line |
| `EditorMode::Normal` | `I` | → `EditorMode::Insert` at start of line |
| `EditorMode::Insert` | Esc | → `EditorMode::Normal` (stay in `Mode::Editing`) |
| `EditorMode::Normal` | Esc | → Exit `Mode::Editing` (revert field) |
| Any | Enter | → Commit and exit `Mode::Editing` |

---

## `Action` enum extension

`:w` requires saving without closing the TUI. Add a new variant:

```rust
pub enum Action {
    None,
    Save,              // save + close
    WriteOnly,         // NEW: save, stay open, clear dirty flag
    Cancel,
    DiscardConfirmed,
}
```

The driver's `run_loop` handles `WriteOnly` by writing the config and
passwords to their stores, resetting `model.dirty = false`, and continuing
the event loop.

---

## Status bar

### Layout

The status bar is the single bottom row of the TUI. It is split into
**two zones**:

```
-- INSERT --                                                           [*]
^─── left: mode indicator or command buffer ──────────────────────────^── right: dirty flag
```

- **Left zone** — mode indicator (see table below), or empty.
- **Right zone** — `[*]` when `model.dirty`, otherwise empty.

The split is rendered with a left-aligned `Paragraph` and a right-aligned
`Paragraph` using a two-column `Layout`.

### Content table

Authentic vim behaviour: the mode indicator appears only when the mode
needs announcing. Normal mode is intentionally silent.

| `editing_mode` | `mode` | `EditorMode` | Left-zone content |
|---|---|---|---|
| Default / Emacs | Normal | — | `Tab/Shift-Tab: move  Enter: edit  Ctrl-→/←: screen  Ctrl-S: save  ?: help` |
| Vi | Normal | — | `h/l: screen  j/k: field  i/a: edit  :: command  ZZ: save  ?: help` |
| Vi | VimCommand | — | `:<buffer>` (live, e.g. `:wq`) |
| Vi | Editing | Insert | `-- INSERT --` |
| Vi | Editing | Normal | *(empty — vi Normal is silent)* |
| Vi | Editing | Visual | `-- VISUAL --` |
| Default / Emacs | Editing | — | `Editing — Enter: confirm  Esc: cancel` |
| Any | ConfirmDiscard | — | `Unsaved changes. y: discard  n: go back` |
| Any | Help | — | *(blank — help overlay covers the screen)* |

### Transitions

- Pressing `i` or `a` from screen-navigation Normal → `Mode::Editing` +
  `EditorMode::Insert` → `-- INSERT --` appears.
- Pressing Esc in Insert → `EditorMode::Normal` (stay in `Mode::Editing`)
  → `-- INSERT --` disappears, left zone is blank.
- Pressing Esc again in Normal → exit `Mode::Editing`, revert field →
  blank left zone (back to vi nav hint).
- Pressing `:` from screen-navigation Normal → `Mode::VimCommand` →
  `-- INSERT --` gone, left zone shows `:` cursor.
- Typing after `:` → `:wq` cursor updates live.
- Pressing Esc in VimCommand → `Mode::Normal`, left zone returns to vi nav hint.

### Implementation

`StatusBar::content(mode, editor_mode, editing_mode, command_buffer)` is
a pure function that returns the left-zone string. `editor_mode` is
`Option<EditorMode>` — `None` when not in `Mode::Editing`.

The existing `StatusBar` struct can remain; `message` holds the left-zone
text and `is_error` drives the error colour. The view layer constructs it
from the pure function rather than reading it from `model.status` directly,
so the two-zone layout and dirty indicator are always in sync without the
model needing to proactively update `status` on every keypress.

---

## Help overlay

The help overlay (activated by `?`) currently lists a fixed set of
bindings. It must show mode-appropriate content:

**Default / Emacs mode:**
```
Keybindings
  Tab / Shift-Tab    Move focus
  Ctrl-→ / Ctrl-←   Next / previous screen
  Enter              Edit focused field
  Esc                Cancel edit / quit
  Ctrl-S             Save
  ?                  Toggle this help
```

**Vi mode:**
```
Keybindings (vi)
  h / l              Previous / next screen
  j / k              Previous / next field
  i / a              Edit field (insert / append)
  :w                 Save
  :wq  ZZ            Save and quit
  :q!  ZQ            Quit without saving
  :q                 Quit (fails if unsaved changes)
  Esc                Cancel / return to Normal
  ?                  Toggle this help
```

The overlay content is provided by a pure function
`help_lines(editing_mode: EditingMode) -> Vec<Line>` tested separately.

---

## Architecture — `:w` mid-session save

The current `run_loop` (in `driver.rs`) only writes on `Action::Save` then
exits. With `Action::WriteOnly`, it must:

1. Call `store.save(&cfg)` and `save_passwords(model, keyring)`.
2. Set `model.dirty = false` and update the status bar to "Saved."
3. Continue the event loop.

This is a small addition to the match arm in `run_loop`.

---

## Tests first

All tests are written in the appropriate modules before any production code.

### `src/tui/model.rs` — vi navigation tests

1. `vi_j_advances_focus` — `j` in vi Normal mode moves focus to next field.
2. `vi_k_moves_focus_backward` — `k` moves to previous field.
3. `vi_l_goes_to_next_screen` — `l` advances to next screen.
4. `vi_h_goes_to_prev_screen` — `h` goes to previous screen.
5. `vi_i_enters_editing_at_cursor_start` — `i` opens `Mode::Editing` with
   `EditorMode::Insert`, cursor at column 0.
6. `vi_a_enters_editing_at_end` — `a` opens `Mode::Editing` with cursor at
   end of existing text.
7. `vi_A_is_alias_for_a` — `A` behaves identically to `a`.
8. `vi_z_alone_does_not_fire` — single `Z` press sets `pending_key` and
   produces no action.
9. `vi_ZZ_saves` — two consecutive `Z` presses return `Action::Save`.
10. `vi_ZQ_quits_without_save` — `Z` then `Q` returns `Action::DiscardConfirmed`.
11. `vi_Z_then_non_ZQ_clears_buffer` — `Z` then `j` clears `pending_key`,
    moves focus, no save action.
12. `vi_default_bindings_still_work` — `Ctrl-S` saves, Tab moves focus,
    even in vi mode (backward-compat aliases).

### `src/tui/model.rs` — command mode tests

13. `colon_enters_vim_command_mode` — `:` switches mode to
    `VimCommand { buffer: "" }`.
14. `colon_accumulates_chars` — typing `wq` into command mode sets
    `buffer == "wq"`.
15. `backspace_removes_from_command_buffer` — Backspace trims last char.
16. `esc_in_command_mode_cancels` — Esc returns to `Mode::Normal`, no action.
17. `colon_w_enter_returns_write_only` — `:w` Enter returns `Action::WriteOnly`.
18. `colon_wq_enter_returns_save` — `:wq` Enter returns `Action::Save`.
19. `colon_x_enter_returns_save` — `:x` is alias for `:wq`.
20. `colon_q_clean_returns_cancel` — `:q` with `dirty == false` returns
    `Action::Cancel`.
21. `colon_q_dirty_shows_error` — `:q` with `dirty == true` returns
    `Action::None` and sets `status.is_error`.
22. `colon_q_bang_returns_discard` — `:q!` returns `Action::DiscardConfirmed`
    regardless of dirty flag.
23. `unknown_command_shows_error` — `:xyz` Enter returns `Action::None`
    and status says `unknown command: xyz`.
24. `empty_command_cancels` — `:` then Enter (empty buffer) returns to Normal
    silently.

### `src/tui/model.rs` — `Action::WriteOnly` driver test

25. `write_only_action_clears_dirty` — after the driver handles
    `Action::WriteOnly`, `model.dirty == false`.

### `src/tui/model.rs` — `StatusBar::content` unit tests

These are pure-function tests; no rendering required.

26. `status_content_vi_normal_shows_nav_hint` — vi + `Mode::Normal` →
    left zone contains `h/l` and `j/k`.
27. `status_content_vi_insert_shows_insert_indicator` — vi + `Mode::Editing`
    + `EditorMode::Insert` → `"-- INSERT --"`.
28. `status_content_vi_editor_normal_is_blank` — vi + `Mode::Editing` +
    `EditorMode::Normal` → left zone is empty string.
29. `status_content_vi_visual_shows_visual_indicator` — vi + `Mode::Editing`
    + `EditorMode::Visual` → `"-- VISUAL --"`.
30. `status_content_vim_command_shows_colon_buffer` — vi + `Mode::VimCommand
    { buffer: "wq" }` → `":wq"`.
31. `status_content_default_normal_shows_tab_hint` — default + `Mode::Normal`
    → contains `Tab` and `Ctrl-S`.
32. `status_content_emacs_editing_shows_editing_hint` — emacs + `Mode::Editing`
    → `"Editing — Enter: confirm  Esc: cancel"`.

### `src/tui/view.rs` — rendered status bar tests (TestBackend)

33. `rendered_status_bar_shows_insert_in_vi_insert_mode` — render a vi model
    in `Mode::Editing` + `EditorMode::Insert`; bottom row of the
    `TestBackend` buffer contains `-- INSERT --`.
34. `rendered_status_bar_blank_in_vi_editor_normal_mode` — render a vi model
    in `Mode::Editing` + `EditorMode::Normal`; bottom row does not contain
    `INSERT` or `NORMAL` or `VISUAL`.
35. `rendered_status_bar_shows_colon_buffer_in_command_mode` — render a vi
    model in `Mode::VimCommand { buffer: "wq" }`; bottom row starts with `:wq`.
36. `rendered_dirty_flag_appears_in_right_zone` — render any dirty model;
    bottom row contains `[*]` near the right edge (column > 60 for an
    80-column terminal).
37. `rendered_dirty_flag_absent_when_clean` — clean model; bottom row does
    not contain `[*]`.

### `src/tui/view.rs` — help overlay tests

38. `help_overlay_vi_mode_shows_vi_bindings` — rendered buffer in Help mode
    with vi active contains `:wq` and `ZZ`.
39. `help_overlay_default_mode_shows_tab_ctrl_s` — rendered buffer in Help
    mode with default active contains `Tab` and `Ctrl-S`.

### `tests/tui.rs` — integration

40. `vi_save_via_colon_wq` — drive a model with vi mode through `:wq`
    sequence; assert `Action::Save` returned and store receives the config.
41. `vi_force_quit_via_colon_q_bang` — drive through `:q!`; assert
    `Action::DiscardConfirmed`, no store write.
42. `vi_write_only_clears_dirty_and_stays_open` — drive through `:w`; assert
    store gets a write, model remains open (no Save/Cancel action), dirty cleared.
43. `vi_insert_indicator_disappears_on_esc_to_normal` — render at each step
    of the `i` → type → Esc → Esc sequence; assert `-- INSERT --` present only
    while in `EditorMode::Insert`.

---

## Acceptance criteria

- All 43 new tests pass; existing 104 tests unchanged.
- `cargo clippy --all-targets -- -D warnings` clean.
- In vi mode: `j`/`k`/`h`/`l` navigate; `:wq` and `ZZ` save; `:q!` and `ZQ`
  quit without save; `:q` fails gracefully if dirty.
- In default/emacs mode: no behavioural change from today.
- `-- INSERT --` appears in the bottom-left when a vi-mode field is in
  `EditorMode::Insert`; the indicator is absent in Normal mode and replaced
  by `:command` in VimCommand mode.
- Help overlay shows vi bindings when in vi mode, default bindings otherwise.
- `i` opens editing with cursor at the start of the field; `a`/`A` open with
  cursor at the end.
- `:w` saves without closing the TUI; status bar briefly shows "Saved."
- The `[*]` dirty indicator always appears at the right edge of the status
  bar regardless of the left-zone content.

---

## Deferred

- `gg` / `G` for first/last screen navigation (requires a two-char
  sequence buffer, low priority).
- `0` / `$` for first/last field on screen.
- Numeric prefix commands (`3j` to jump 3 fields down).
- Mouse click to focus a field.
- Custom `:` commands (`:set`, `:help`, etc.).
