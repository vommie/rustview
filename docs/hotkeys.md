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
| Video player | the selected video in a tab or the fullscreen (comes first while the Multiview holds a video) |

Conflicts are checked between areas that can be active at the same time: General with Browse, Tab and Video player; Tab and
Fullscreen each with Video player. The one allowed overlap: a Video player key may also page in the Fullscreen (paging only
works for a single image, the player only for a video). Tab and Fullscreen are separate areas (user: they are not quite the
same, e.g. only the fullscreen of a Browse view pages or becomes a tab) with similar defaults.

## Rules (user decisions)
- One row per action and area (Rename in Browse and Rename in Tab are two rows).
- Browse refuses plain printable keys (letters, digits, Space): they belong to type-ahead find.
- Rating keys need Ctrl everywhere (the plain digits of the tab view are gone).
- Paging only in a Multiview opened from Browse (fullscreen): Left / Up / Page Up back, Right / Down / Page Down forward;
  Space never pages; a tab never pages (keys and the toolbar's Prev/Next buttons removed, round 2).
- The buttons of the media bar and the primary bar are actions without a default key (round 2): `MultiviewCommand::Bar` runs a
  media bar button of the selected medium like a click (refused with "Not available for this medium" when it is not on offer),
  a primary bar key runs `perform_primary_action`.
- Plain Backspace is a second zoom reset in the Multiview (Ctrl+0 rates).
- Keys over a video (0.4.6, the "RustView controls the videos" milestone, keyboard part): mpv takes no keys
  (`input-vo-keyboard=no`); X11 then delivers a key pressed over a video to the RustView window under it, so every key over a
  video is the keymap's. The Video Player area comes first while the Multiview holds a video; its actions run `MpvPlayer`
  commands on the selected video (`MultiviewCommand::Player`: pause, mute, jumps of Settings > Viewer > Videos, 5 % volume
  steps, frame steps, speed x1.1); a picture as target shows "Select a video". (0.4.4 had bound the keymap's chords inside mpv as
  `script-message rustview-key`; that path is gone.) The mouse over a video still goes to mpv (see "Mouse over videos").
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

## Video zoom
mpv's builtin `positioning` script (loaded: `load-scripts=yes`, `load-positioning` defaults to yes) already zooms toward
the cursor with Ctrl+wheel over a video. The zoom keys of the Multiview zoom the selected video through its player
(`MpvPlayer::zoom_video_by`: `add video-zoom log2(1 + step)` with the image zoom's step; `reset_video_zoom` sets `video-zoom`,
`video-pan-x/y`, `video-align-x/y` and `panscan` to 0); `load_file` resets it, so a new file in the same player starts
unzoomed. No slider or minimap for videos, and a video stays inside its tile.

## Code
- `ui/keymap/`: `chord.rs` (ported from RustCut; Backtab = Shift+Tab), `action.rs` (the `actions!` table: id, area, title, default keys,
  flags FIXED/TYPING/REPEAT/LABELS/BESIDE_PLAYER), `area.rs` (`Area::overlaps`), `mpv_keys.rs` (chord -> mpv key names, keypad variants),
  `mod.rs` (`Keymap`: from/to the stored table with repairs, lookup per area list, conflicts, labels).
- `ui/hotkeys/`: the keymap in use (`apply` at startup and when the settings change: mpv bindings, media bar labels), `resolve`
  (typing, repeat), `main_window.rs` (Browse/tab view/tab grid), `ui/multiview/session_keys.rs` (fullscreen), the router's `video_key`.
- `ui/settings_dialog/hotkeys_page/` (`page.rs` pure: sections, filter, recording, move on the second press, refusals, reset) and
  `ui/settings_page_hotkeys.slint` (global `HotkeysUi`).
- `ui/hold.slint` + `ui/hold_confirm.rs`: hold to confirm (`ToolbarButton.hold`, `CtxMenuItem::with_hold`, tooltip hold badge).
