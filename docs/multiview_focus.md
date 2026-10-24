# Multiview: the selected medium (focus)

Status: **implemented** (0.4.1: focus model, clicks, marks, hint, rating keys; 0.4.2: every key through `MultiviewCommand`, Esc chain, DEL and zoom keys
in the tab view, refusals with reasons, marks hidden with the cursor and shown by a key; 0.4.3: panels follow the selection, "The selected video" sound
choice, the settings). The follow-up milestones below are open. Concept agreed on 2026-10-24, after RustView 0.4.0. Decisions were made with the user in four rounds; this file is the
reference for the implementation on branch `claude`. Paths are relative to `crates/rustview/` as in CLAUDE.md.

## Goal
A Multiview with two or more tiles gets ONE selected medium (the focus). Every key that acts on a medium acts on the selected one and on nothing else.
With nothing selected such a key does nothing and a short, centered hint says so. A Multiview of one tile needs none of this: its medium is always the
target and nothing is drawn.

## Why
Today the target of a key is decided by the pointer (hover), which is invisible (and gone while the idle cursor is hidden, and on Wayland where the
pointer is not polled), or the key acts on everything: Ctrl+0..5 rates EVERY medium of the view, F2/zoom keys/Enter/Ctrl+S/Ctrl+Z take the tile under
the pointer, DEL works only when exactly one medium is shown and not at all in the tab view. Inspector and Tag & Rate of a Multiview tab act on all of
its tiles. Rating or tagging one medium out of four by keyboard is impossible.

## Current behavior (inventory, 0.4.0)
Inputs come from four sources: the fullscreen window (Slint), the main window (tab view), the native mpv windows of video tiles (under X11 every key goes
to the window under the pointer, so a key over a video reaches mpv, see `--input-vo-keyboard` in the mpv manual) and the bar windows.

| Input | Fullscreen (`ui/multiview_fullscreen_window.slint`) | Tab view (`ui/main.slint` key scope, `src/ui/multiview/tab_host.rs`) | Over a video (mpv, `src/viewer/mpv_engine.rs`) |
|---|---|---|---|
| Esc | crop tools newest first (`instance_edit.rs` `cancel_newest_crop`), then close (pinned refuses) | crop tools, then ends a tab selection | `cycle fullscreen` -> crop tools, then fullscreen: close, **tab view: OPENS the fullscreen** (`route.rs`) |
| Enter | apply crop: hovered tile with a tool, else the newest tool (`apply_crop_target`) | same | mpv default (playlist-next) |
| Ctrl+S / Ctrl+Shift+S | hovered dirty tile, else the only dirty tile (`save_target`) | same | **mpv screenshot written to disk** |
| Ctrl+Z / Ctrl+Shift+Z / Ctrl+Y | hovered tile with edits, else the only one (`undo_target`) | same | nothing |
| Ctrl+0..5 (tab view also 0..5) | ALL media of the view (`rating_targets` = `member_ids`) | same | `script-message rustview-rate` -> all media |
| F2 | tile under the pointer, else last hovered, else first (`target_pick.rs`) | same (`tab_host::rename_target`) | `snap-window` hack -> that tile |
| DEL | only with exactly one medium (`delete_single`) | **nothing** (Browse only) | `rustview-delete` -> only one medium, only in the fullscreen (`router.rs`) |
| Ctrl+Backspace / Ctrl++ / Ctrl+- | image under the pointer, else the only image (`instance_zoom.rs` `zoom_key`) | **missing** | Ctrl++/- change mpv's audio delay |
| Arrows, Space, PgUp/PgDn, wheel | paging of a single Browse image only; free with 2+ tiles | Left/Right: Prev/Next of a one-medium tab | seek, pause, volume (mpv defaults) |
| Ctrl+W | close | close the tab | `window-minimized` hack -> close |
| Click | only double-click detection; the tile index is thrown away (`session.rs`, `tab_host.rs`) | same | `MBTN_LEFT ignore`: RustView never sees it |
| Double / middle click | close | open the tab's fullscreen | `cycle fullscreen` |
| Ctrl+double click | Browse origin: to a tab; tab origin: close | nothing | `window-maximized` hack |
| Ctrl+wheel, drag, minimap, crop drag | image under the pointer | same | mpv |
| F5 | - | refresh all media of the tab | - |

Media bar buttons always act on their own tile (`TileKey`); primary bar buttons on the whole view.

Found along the way (fixed by this milestone or the follow-up input milestone, see below): Ctrl+S over a video writes an mpv screenshot instead of
saving the edited picture; Ctrl++/- over a video changes the audio delay; `s`/`S` write screenshots; Ctrl+C is mpv's `quit 4` (probably ends the tile's
player, unverified); Esc and `f` over a video in the tab view open the fullscreen.

## Behavior

### Focus
- Exists only while the Multiview shows 2+ tiles. A single tile is always the target, without any marks.
- One focused tile at most (`Option<TileKey>` in `MultiviewInstance`; tiles are focused, so two tiles of one medium are told apart). No multiple
  selection, no Ctrl/Shift extension, no "act on all" by key.
- **Set by** a plain left click on the medium (an image: the click that is not a drag; a video: a new mpv binding, see "mpv in this milestone") and by
  any click on a button, menu item or slider of the tile's media bar. **Not set by** hover, wheel, Ctrl+wheel, dragging a zoomed picture, the minimap,
  dragging the crop rectangle, gutters, Ctrl+click (Ctrl+double click keeps its meaning).
- **Never cleared by the user**, only moved to another tile; a click on free space does nothing.
- Opening a Multiview of 2+ media: nothing focused. Merging tabs: nothing focused.
- Growing from one tile to two or more (Duplicate, Browse follow): the formerly lone tile is focused (now visibly).
- The focused tile leaves (Remove, Delete, Browse follow): nothing focused.
- Kept when the view moves between fullscreen and tab, when tabs are switched and in the tab grid. Not kept across restarts (tabs are not restored).
- No focus change while a dialog locks the view.

### Keys that need a target
Ctrl+0..5 (and 0..5 in the tab view), F2, DEL, Ctrl+Backspace, Ctrl++, Ctrl+-, Enter (apply crop), Ctrl+S, Ctrl+Shift+S, Ctrl+Z, Ctrl+Shift+Z / Ctrl+Y,
F5 (tab view). Each acts on the focused tile only, whichever window delivered the key (fullscreen, main window, or a video's window through the mpv
messages that exist: Ctrl+0..5, DEL, F2), wherever the pointer is.
- Nothing focused (2+ tiles): nothing happens and the hint "Select a medium first" appears.
- The key does not fit the focused tile: nothing happens and the hint names the reason, e.g. "Nothing to save", "Nothing to undo", "Nothing to redo",
  "No crop to apply", "Videos cannot be zoomed here", "Saving in progress".
- DEL opens the regular delete dialog for the focused medium, also in the tab view and from a video's window in the tab view.
- The zoom keys work in the tab view as in the fullscreen.

Unchanged and view-wide: Esc (see below), Ctrl+W, double and middle click, Ctrl+double click, the primary bar (Mute All, Play/Pause All, ...).

### Esc
1. A dialog is open: it comes to the front (as today).
2. The crop tool of the focused tile is cancelled.
3. The other open crop tools, newest first (LIFO).
4. Fullscreen: close (a pinned one refuses). Tab view: ends a tab selection, else nothing.
Esc in a video's window gets its own mpv message (instead of `cycle fullscreen`) and runs the same chain, so Esc over a video in the tab view no longer
opens the fullscreen. `f`, double and middle click keep `cycle fullscreen`.

### Tab view panels
Inspector, Tag & Rate (incl. its rating buttons), tag clicks, tag history and F5 of a Multiview tab act on the focused medium (`UiState::active_tab_selection`).
With nothing focused they show "Select a medium in the view" and do nothing. A tab of one medium is unchanged.

### Sound
New choice "The selected video" for Settings > Fullscreen > Sound > "Sound follows" (`fullscreen.sound.follows = "selected"`): focusing a video gives it
the sound and mutes the other videos the policy controls; focusing an image changes nothing. The default stays "first".

## Look
- **Area:** the empty part of the focused tile's column (`TileLayout::column`, full row height) is brightened with white: Off 0 %, Weak 4 %,
  Medium 7 % (default), Strong 11 %. Drawn by `multiview_canvas.slint` below the media, so it is only visible where the medium leaves room (also around videos).
- **Corner marks:** four L-shaped marks inside the corners of the medium, 2 logical px thick, `Theme.primary` (the color of the Browse selection),
  Short 16 / Medium 24 (default) / Long 32 logical px. Images: Slint, above the picture and below the crop overlay; while zoomed they sit in the corners of
  the area the picture is shown in. Videos: mpv `overlay-add` (8 strips, each a 1x1 premultiplied BGRA pixel scaled with `dw`/`dh`, overlay ids of their
  own next to the rating feedback's), re-sent on every layout change like the feedback; works on Wayland and over a paused video. Not drawn in the tab-grid miniature.
- **Hidden with the cursor:** while the resting mouse cursor is hidden (setting or primary bar button) and "Hide the selection marks too" is on, area and
  corners are hidden as well (at once, no fade) and come back with the cursor. A key that acts on the focused tile (also one refused with a reason) shows
  them for about one second. The focus itself stays.
- **Hint:** a circle like the rating/save feedback (dark surface, `Theme.primary` ring, Tabler `hand-click` icon) with one line below it, centered in the
  view; held 750 ms, faded 250 ms like `media_feedback`; a new hint restarts it. It must be visible over a video, so it is a small override-redirect
  window of its own like the bars (blended by the compositor; drawn in the canvas where that is not possible, e.g. Wayland). One single-shot timer, no polling.

## Settings (all additive with `#[serde(default)]`, no migration)
- Settings > Fullscreen, new section "Selected Medium": "Brighten its area" (Off / Weak / Medium / Strong, default Medium), "Corner marks"
  (Short / Medium / Long, default Medium). The marks cannot be switched off (the focus would be invisible).
- Settings > Fullscreen > Primary Action Bar > "Mouse cursor", below "Hide after": "Hide the selection marks too" (indented, enabled only while hiding
  the cursor is on, default on). Tooltip: "While the resting mouse cursor is hidden, the marks of the selected medium are hidden as well. They show again
  with the cursor; the keys still act on the selected medium." The tooltip of "Hide the mouse cursor when it rests" mentions it.
- Tooltip of the primary bar's cursor button (`primary_actions.rs`) while the switch is on: "The mouse cursor and the selection marks are hidden after
  N s without movement (click to keep them visible)" / "Hide the mouse cursor and the selection marks after N s without movement (they show again when it moves)".
- Settings > Fullscreen > Sound > "Sound follows": new choice "The selected video".

## mpv in this milestone (the minimum)
- `MBTN_LEFT` -> a `script-message` that tells RustView which tile was clicked (no pause; `MBTN_LEFT_DBL` must still work).
- `ESC` -> a `script-message` for the Esc chain (instead of `cycle fullscreen`).
- The existing messages (Ctrl+0..5, DEL, F2) are routed to the focus. Everything else of mpv's keyboard stays until the input milestone.

## Architecture
- `viewer/multiview/focus.rs` (pure, unit tested): the focus rules (set, move, tile set changes, growth from one tile, leaving tile).
- `viewer/multiview/targets.rs` (pure, unit tested): ONE resolver `command -> Target::{Tile(TileKey), NoFocus, Refused(reason)}` replacing the scattered
  hover rules (`target_pick`, `save_target`, `undo_target`, `apply_crop_target`, `zoom_key`, `rating_targets`, `delete_single`, `rename_hovered`).
- A `MultiviewCommand` enum (rate, rename, delete, zoom reset/in/out, apply crop, save, save as, undo, redo, refresh, escape) is the one entry point of
  every key source (fullscreen window, main window, mpv messages). The follow-up hotkey system only replaces the key -> command table.
- Hint: pure model (`hint.rs`: text, icon, timing, clock passed in) + its window (`ui/multiview_hint_window.slint`, listed in `ui/shutdown/windows.rs`).
- Logging: `[Multiview: Focus]` and `[Multiview: Hint]` messages with tile keys and media ids only.

## Stages (one testable patch version each, all on `claude`)
1. Focus model, click on images and videos, bar clicks focus, marks for images and videos, the hint component. **Done in 0.4.1**, together with the
   rating keys (Ctrl+0..5) acting on the focus; settings for the marks are still the defaults (Medium/Medium).
2. All keys through `MultiviewCommand` to the focus (fullscreen, tab view, mpv messages), the Esc chain incl. the mpv Esc binding, DEL and zoom keys in
   the tab view, refusals with reasons, marks revealed by a key. **Done in 0.4.2**; hiding the marks with the cursor already works (default on), its
   switch and tooltips follow in stage 3. F5 acts on the selected medium; the refresh button of the tab toolbar still refreshes the whole tab.
3. Tab view panels, "The selected video" sound choice, the settings section and the cursor-hiding switch with its tooltips, CLAUDE.md (Multiview chapter)
   and CHANGELOG. **Done in 0.4.3.** The Tag & Rate panel greys out and shows "Select a medium in the view" while nothing is selected; the refresh button
   of the tab toolbar keeps refreshing every medium of the tab (only F5 follows the selection).

Tests: unit tests for `focus`, `targets`, `hint`, the settings sanitizing and the sound rule; GUI checks with `rv-testenv session` (2, 3 and 4 tiles,
images and one video, fullscreen and tab view, keys with and without focus, cursor hiding).

## Follow-up milestones (each in a new, clean chat)
1. **Hotkey system** in the settings, modeled on RustCut (`rustcut/crates/rustcut/src/ui/keymap/{mod,action,chord}.rs` and the Hotkeys page
   `ui/settings/hotkeys.rs`: actions with their keys, filter, recording a key, a taken key moves on the second press), grouped by area (Browse,
   Multiview, Player in the Multiview, ...).
   **Done in 0.4.4** (`docs/hotkeys.md`).
2a. **Keyboard part of milestone 2 done in 0.4.6:** mpv takes no keys (`input-vo-keyboard=no`), RustView handles every key over a video (Space,
   M, jumps, frame steps, volume, speed through the keymap's Video Player area). The mouse over a video is RustView's since 0.4.7 (`docs/video_controls.md`).
2. **RustView controls the videos itself:** mpv gets no key bindings of its own any more; RustView forwards its commands (e.g. Space play/pause,
   M mute/unmute, Left/Right jump backward/forward). Removes the screenshot/audio-delay/quit leaks listed above.
3. **Own OSD** instead of mpv's OSC/OSD.
