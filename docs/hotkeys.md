# Configurable hotkeys

Every key of RustView that triggers an action comes from ONE keymap that the user edits in Settings > Hotkeys, modeled on
RustCut's keymap (`rustcut/crates/rustcut/src/ui/keymap/`, page `ui/settings/hotkeys.rs`). The Multiview keys still run as
`MultiviewCommand`s on the selected medium (`docs/multiview_focus.md`); the keymap only replaced the key -> command tables.
User decisions: questionnaires `~/claude/feedback/2026-10-24-rustview-hotkeys` and `...-hotkeys-2` (2026-10-24).

## Areas
An action belongs to one area; the same key may mean different things in areas that are never active together.

| Area | Where it applies |
|---|---|
| General | the main window in every view (Browse, tab view, tab grid) |
| Browse | the Browse view (Rename also in the tab grid, where it acts on the last used panel) |
| Tab | a Multiview shown in a tab (acts on the selected medium) |
| Fullscreen | the Multiview fullscreen window (acts on the selected medium) |
| Video player | the selected video in a tab or the fullscreen; shown now, takes effect when RustView controls the videos (next milestone) |

Conflicts are checked between areas that can be active at the same time: General with Browse, Tab and Video player; Tab and
Fullscreen each with Video player. The one allowed overlap: a Video player key may also page in the Fullscreen (paging only
works for a single image, the player only for a video). Tab and Fullscreen are separate areas (user: they are not quite the
same, e.g. only the fullscreen of a Browse view pages or becomes a tab) with similar defaults.

## Rules (user decisions)
- One row per action and area (Rename in Browse and Rename in Tab are two rows).
- Browse refuses plain printable keys (letters, digits, Space): they belong to type-ahead find.
- Rating keys need Ctrl everywhere (the plain digits of the tab view are gone).
- Paging only in a Multiview opened from Browse (fullscreen): Left / Up / Page Up back, Right / Down / Page Down forward;
  Space never pages; a tab never pages (keys removed).
- Plain Backspace is a second zoom reset in the Multiview (Ctrl+0 rates).
- Keys over a video follow the keymap now: RustView binds the chords of its Tab/Fullscreen/General actions in mpv as
  `script-message rustview-key <chord>` (input section `rustview_keys`), the router resolves them for the host of that
  player. Chords that a Video player action has stay with mpv until the next milestone.
- Fixed keys (cannot be changed, shown greyed with a lock and a tooltip that says why): Esc (cancels, closes, clears the
  selection; it also cancels the recording of a key), the cursor keys of Browse (arrows, Page Up/Down, Home/End, Shift
  extends), type-ahead letters. Dialog keys (Enter, Esc) are not listed.
- Text fields: while one has the keyboard only actions marked "while typing" act (close tab, focus the filter, tab
  switching, settings, F1).

## Settings page "Hotkeys" (before "API")
- Sections per area, collapsible (collapsed sections, the filter text and "Only changed" persist).
- A filter (action, area or key) and an "Only changed" switch; no other change marker (the active reset button shows it).
- A row: title, its keys as chips (x removes one), "add a key" (then press it; Esc cancels), "back to the default"
  (tooltip names the default; enabled only when changed). Fixed rows have a lock icon in place of the reset button.
- A key another action of an overlapping area has is not taken at once: a note in the row names that action, pressing the
  same key again moves it. Browse refuses plain printable keys with a note.
- "Reset every key" is held (hold to confirm, `ui/hold.slint`, 1 s by default, Settings > General).
- Only actions whose keys differ from the defaults are stored (`hotkeys` in `settings.json`, additive, no migration);
  unknown ids and unreadable keys are dropped with a log note, a key bound twice stays with the configured action.

## Shown keys
Context menus, tooltips, status messages and setting descriptions name the current first key of an action
(`Keymap::first_label`), never a hard-coded key. The Rate Selection submenu shows the rating keys.

## New actions (user decisions)
F5 refreshes the selected files in Browse (the menu promised it), F11 opens the selection in the fullscreen, Ctrl+Enter
opens it in a background tab, F11 / F show a tab in the fullscreen and leave it, Ctrl+Tab / Ctrl+Shift+Tab /
Ctrl+Page Down / Ctrl+Page Up switch tabs, Alt+1 / Alt+2 show Browse / the tab grid, Ctrl+Comma opens the settings, F1 opens
them on the Hotkeys page, Ctrl+T switches Browse between thumbnails and the table. Mouse buttons stay as they are.

## Video zoom (confirmed idea)
mpv's builtin `positioning` script (loaded: `load-scripts=yes`, `load-positioning` defaults to yes) already zooms toward
the cursor with Ctrl+wheel over a video. The zoom keys of the Multiview send `add video-zoom` / reset `video-zoom`,
`video-pan-x/y` to the selected video's player; a new file in the same player starts unzoomed. No slider or minimap for
videos, and a video stays inside its tile.
