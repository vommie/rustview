# The video control bar

Every video of a Multiview (fullscreen and tab view; the tab grid has a small one) has a control bar of its own at its bottom.
Since `video-in-slint` (0.5.7-0.5.13, `docs/rendering.md`) a video is a texture drawn by the canvas like a picture, so the pointer
over a video is simply the window's: mpv's on-screen controller (OSC) is off, mpv takes neither keys nor the mouse, and the bar is
an element of the canvas.

## Decisions (questionnaire round 4, 2026-10-24, `~/claude/feedback/2026-10-24-rustview-video-bar`)

- Content, left to right: play/pause, the current time, the progress bar, the remaining time, the volume (a speaker button and a
  volume slider instead of a plain mute button).
- Style: a translucent bar over the whole width of the video, about 70 % opaque, or a floating pill (both are options under
  Settings > Multiview > Player, as is the opacity; default: the bar at 70 %).
- Height: freely adjustable (default 30 px); fonts, buttons, the progress bar and the slider grow with it.
- Progress bar: a wide block like mpv's, the played part in the primary blue. It is an open model: layers of spans, the played
  part is the first one, so later features (A-B repeat, sections) add spans without touching the bar.
- Seeking: click and drag, both exact by default (`seek <t> absolute+exact`); the bar shows the target until mpv is there. A
  bubble above the progress bar shows the time under the pointer. How it seeks is an option (user request, 2026-10-24): exact
  (default), like mpv's own controller (a click exactly, a drag by keyframes, nothing more on release), by keyframes, or as mpv is
  configured (`hr-seek`); `SeekMode::seek_for` maps each step of a drag to mpv's seek flags (`SeekPrecision`).
- Times: short form ("1:23", "-0:45"; hours only for videos of an hour or more), fixed width (measured from a template of the
  widest text), filled icons.
- Milliseconds: a click on either time switches both. Whether a new video starts with them is a setting (default: without); the
  click is not stored and ends with the video.
- When it shows: while the pointer is near the bottom of the video (default) or after any movement over the video (it goes after
  1.5 s at rest), an option. Also while paused only with the pointer. A short fade (0.15 s) is an option (default on; since 0.5.8
  also without a compositing manager, because the canvas draws the bar).
- Several videos: each has its own bar.
- Volume: mute is a state of its own, as in mpv. The slider sets the level and never touches mute (a muted video stays muted while
  its level goes up; the slider shows the level dimmed). The speaker shows "muted" when the video is muted or at level 0. A click
  unmutes a muted video, mutes an audible one, and brings a silent one back to its last audible level.
- From round 3: the mouse wheel over a video changes the volume (5 % per notch; Ctrl + wheel zooms the video), a right
  click plays or pauses, a click selects the tile, double and middle click work as over pictures (through the canvas). Video
  zoom, panning, the minimap and the zoom slider like pictures: built in 0.4.12 (see below).
- Volume feedback (0.5.2): when the wheel or a video player key (Louder/Quieter) changes the volume, a flat, semi-transparent volume bar
  (a speaker icon of the bar's height left of a filled bar, the level like the slider: 100 % and more = full, dimmed while muted) shows
  centered in the lower part of the video's picture, at most half its width, and holds and fades like the feedback pulse; a new change
  restarts it. The control bar's slider shows none. The mute key shows the centered speaker icon (crossed out when muted) like the rating
  icon; the mute buttons show none. Pure: `viewer/multiview/volume_feedback.rs` (inputs, pulses, geometry); drawn by the canvas
  from `TileView::volume` (`volume-*` of `MultiviewTileView`, since 0.5.10); `instance_volume_feedback.rs`; the host arms its
  feedback timer after `take_feedback_request`.

## Code

- Pure (`src/viewer/multiview/video_bar/`): `prefs` (the settings as a global), `placement` (the bar and the room above it),
  `reveal` (when it shows, fade-out), `progress` (the span model), `time_text`, `volume` (the speaker logic), `state` (per tile: the
  millisecond switch, drags, the pending seek, the last audible level), `view` (the data between instance and canvas, and
  `VideoBarInput::from_event`, which decodes the events of the canvas).
- Instance: `instance_video_bar.rs` (`video_bar_targets` whenever a host updates its bars: a pointer event, a player event, its tick (`instance_tick.rs`: every 50 ms only while the bar of a playing video shows, else at the next deadline), `video_bar_input` for what the user did,
  `video_wheel`, `video_right_click`); the state lives in `VideoTile::bar`.
- Drawn by the canvas (since 0.5.8, branch `video-in-slint` M2): `MultiviewCanvas.video-bars` holds one `MultiviewVideoBar`
  (`ui/multiview_video_bar.slint`) per bar, over its video and above every tile; its events go to the window's `VideoBarBus` global
  with the tile's key. `src/ui/multiview/video_bar/` keeps one `VideoBarModel` per host (rows updated in place by `TileKey`, the
  bar with an open popup last) and converts the targets (`convert.rs`). Hosts: `session_video_bar.rs` (fullscreen),
  `tab_video_bar.rs` (tab view; it also routes the main window's bar events) and `grid_bars.rs` (one model per Multiview of the
  tab grid, handed to its grid item).
- Settings: `FullscreenPlayerSettings` (`fullscreen.player`), card `ui/settings_card_player.slint` + `settings_dialog/player_bar.rs`.

## A narrow bar collapses (built in 0.6.10)

When several videos share the Multiview, a tile can be too narrow for the whole bar. The progress bar always keeps at least
three bar heights (`MULTIVIEW_VIDEO_BAR_SEEK_MIN_BAR_HEIGHTS`); the other parts give way step by step (`video_bar::collapse::BarFit`):

| Step | What changes |
|---|---|
| 0 `Full` | everything: play, current time, progress, remaining time, A-B group, speaker, volume slider |
| 1 `AudioButton` | the slider goes; a click on the speaker opens a vertical volume bar above it (percent, the bar, mute; RustCut's look), a right click mutes, the wheel changes the volume |
| 2 `NoRemaining` | the remaining time goes |
| 3 `AbButton` | the A-B group becomes one button (its icon in the loop's color); a click shows the group in a bubble above it, the next click hides it (the toggle is remembered in `view.video_bar_ab_bubble_open`, for every bar and the next start) |
| 4 `More` | the speaker and the A-B button make room for "...": its menu has "Volume N %" (opens the volume bar above "..."), Mute, and the submenu "A-B Repeat" (Set A, Set B, Loop A-B, Clear A and B, Save..., Load...) |
| 5 `NoTime` | the current time goes too; the menu starts with both times and a Milliseconds switch |

**Measured, not guessed.** Every part of the bar has a fixed width (the times are as wide as their templates), so
`ui/multiview_video_bar.slint` computes what each step needs besides the progress bar (`need(step)`) and reports the six widths
(event 26) when the bar is made and whenever one changes (bar height, the times' templates, the A-B group). Rust keeps them per
tile (`BarFitState`) and picks the fullest step that fits (`decide`); a step fuller than the one shown needs 16 px to spare
(`MULTIVIEW_VIDEO_BAR_COLLAPSE_HYSTERESIS_LOGICAL_PX`), so a width at a boundary never flickers. Until the first measurement a bar
shows the narrowest step (it fades in; the measured step follows a frame later). The parts are placed by explicit `x` (from the
left: play, time; from the right: slider, speaker or "...", the A-B group or button, remaining time); the progress bar takes the
rest, and the openers of every popup are known from the same properties.

**Popups.** The volume bar and the "..." menu (`video_bar::popups::BarPopup`) and the A-B save bubble and load menu are exclusive:
opening one closes the other. Each opens above its opener with its caret on the opener's center: Save and Load of the group (in
the bar or in the A-B bubble, above which they then open), else the A-B button, else "...". A popup whose opener leaves the bar
closes (`settle_popups`); a press anywhere else on the canvas closes it (event 30), Esc too (`escape_crop` asks
`close_video_bar_popups` first). The "..." menu is a `MultiviewBarMenu` with the rows of a context menu; its open submenu sits
beside it (left where the canvas has room), bottoms aligned, without caret. A new kind of entry (the video filters of milestone 2)
is one more submenu in `more_menu.rs`. The small bars of the tab grid have no steps.

**Code.** Pure: `video_bar/{collapse,popups,more_menu}.rs`; instance: `instance_video_bar.rs` (`bar_view` decides the step,
`more_action`); Slint: `ui/multiview_video_bar.slint` (placement, `need`, popups), `ui/multiview_video_bar_parts.slint` (buttons with
tooltips, times, the bubble and the volume popup), `ui/multiview_video_bar_ab_marks.slint` (the A-B strip, flags and labels),
`ui/multiview_video_bar_view.slint` and `ui/menu_item_types.slint` (the structs, in files of their own so `types.slint` can use
them without an import cycle).

The load menu has a refresh button left of "Delete all": the script reads its file again (`reload`) and publishes the list, in
this player and in every other player of the same file (`MpvPlayer::ab_command`). Nothing watches the file; it is read only on
this click and where the script reads it anyway (loading a file, every write).

## A-B repeats of the mpv script ab_repeat.lua (questionnaire 2026-10-25, built in 0.5.4)

While the user's mpv script ab_repeat.lua runs in a video's player (Settings > Multiview), the bar gets an A-B group in
a capsule between the remaining time and the sound: A, B, AB (a click switches the loop on and off, holding it clears A and B),
Save (a name bubble) and Load (the menu of saved repeats, with a badge of their number). Above the progress bar a tab grows out
of the bar as soon as it has something to show (a saved repeat, A or B): same color, curved flanks like a Chrome tab, a third of
the bar high and at least 12 px, ending exactly on the bar (an overlap of the two translucent fills showed as a darker seam).
In it: one thin row of the
saved repeats in translucent colors
(overlaps show stronger), the current A-B in yellow (blue while its loop is off), the flags of A and B with their times (labels
pushed apart when they would touch, dashed when the script set the point itself) and the name (and times) of the repeat under
the pointer. While a repeat loops, the progress bar outside A-B is darker. Everything the user does is a command of the script
(`docs/mpv_ab_repeat.md`); the bar only shows what the script publishes. Code: `video_bar/{ab_view,ab_state,ab_menu}.rs` (pure),
`instance_ab_repeat.rs`, `ui/multiview_video_bar_ab.slint` and the A-B part of `ui/multiview_video_bar.slint`. The Save bubble and
the Load menu open inside the window above their button, the caret at the button's center and touching the bar; the text field
takes the keyboard when it opens, a press anywhere else on the canvas (a backdrop under the bar) closes the popup, and the host
gives the keyboard back to its window when it closed (`VideoBarModel::take_focus_back`). They are plain elements of the canvas,
not Slint `PopupWindow`s: the tooltips of the menu's buttons are drawn by the window's tooltip layer, which a `PopupWindow` would
cover, and nothing has to tell Rust that Slint closed one.

A key that moves a video (the jump and frame keys, the A-B repeat keys) shows the bar for 1.5 s (`BarReveal::pulse`), like the
volume bar after a wheel step; the script's own jumps of a loop do not.

## Tab-Grid items (questionnaire 2026-10-24, `~/claude/feedback/2026-10-24-rustview-tab-grid`; built in 0.4.8)

- The mouse over a video item is RustView's too, like over a picture item: click opens the tab, Ctrl+click and Shift+click select,
  middle click opens the fullscreen, right click opens the tab's context menu, the wheel scrolls the grid smoothly, and the
  blue hover frame shows. mpv's grid bindings (clicks and the wheel reported through property toggles) go away.
- Videos stay paused in the grid (as today): the grid is an overview, paused videos cost nothing.
- A small control bar appears while the pointer is over a video item: play/pause and a progress bar (a click on it controls the
  video instead of opening the tab).
- A badge at the bottom right of the video shows the time, position and length ("0:03 / 0:08"); it hides while the small bar
  shows. (The pause badge of today lies under the video window and is never visible.)
- New hotkey area "Tab-Grid": the arrow keys move a mark from item to item (it looks like the hover frame), Enter opens the
  marked item, Del or Ctrl+W closes its tab, F11 opens it in the fullscreen.

Code: the grid item's own touch area handles a video like a picture (the video is a texture of the item's canvas). Every
Multiview of the grid is placed in grid content coordinates, so Slint scrolls and clips the items with their videos and a scroll
step re-places nothing (0.5.12). `ui/multiview/grid_bars.rs` is the grid's tick (started by `BridgeTabGrid::sync_tab_grid_ui`, runs only while a video of the grid plays or a
bar fades, stops when the grid goes, `grid_bars::active_changed`): it asks every Multiview in the grid for its small bars (`instance_grid_bar.rs`:
`grid_video_bar_targets`, reveal mode `Hover`, `placement::place_mini`, fixed 22 px, only inside the visible part of the grid),
gets the pointer from the main window's pointer events (`grid_bars::on_pointer`, moved into the content by `to_content`) and fills
the time badges (`TimeBadgeModel` per Multiview, `grid_time_badges`; a Slint badge of the grid item, hidden where a small bar
shows). The small bars are rows of a `VideoBarModel` per Multiview with the `mini` layout, drawn by the item's canvas. Keys:
`Area::Grid` in the keymap, `main_window::dispatch` (view mode 2) -> `tab-grid-key` -> `ui/tabs/grid_keys.rs`; the mark is
`UiState::tab_grid_mark` / `TabGridItemModel::is_marked`, moved by the pure `ui/tabs/grid/mark.rs` (`neighbor` in the masonry
layout) and scrolled into view with `tab-grid-reveal`. mpv's grid bindings (clicks and the wheel reported through property
toggles) are gone.

## Video zoom (round 3, built in 0.4.12)

Videos zoom exactly like pictures: Ctrl + wheel toward the pointer, the zoom keys, dragging the picture, the minimap (the video's
own texture in small, so it plays along; drag, middle click = reset, right click = the slider bubble), the zoom slider of the media
bar, and a zoomed video grows into the free room of its tile. The zoom model is the pictures' (`ZoomState::new_video`, without the
picture work). Since 0.5.11 mpv always renders the whole, fitted picture into the video's texture and the canvas places and scales
it where the zoom model puts the picture, clipped to the room (`video_placement.rs`, `instance_video_zoom.rs`), so zooming and
panning never wait for mpv.
