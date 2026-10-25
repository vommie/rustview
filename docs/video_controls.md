# The video control bar

Every video of a Multiview (fullscreen and tab view, not the tab grid) has a control bar of its own at its bottom. RustView takes
the pointer over videos for it: mpv's on-screen controller (OSC) is off and the video windows let the pointer through to the host
window (`X11VideoSubwindow::set_input_passthrough`, applied by `MultiviewInstance::apply_pointer_passthrough` on every tick in the
fullscreen and the tab view). The tab grid still leaves the pointer to mpv (its bindings report clicks and the wheel).

## Decisions (questionnaire round 4, 2026-10-24, `~/claude/feedback/2026-10-24-rustview-video-bar`)

- Content, left to right: play/pause, the current time, the progress bar, the remaining time, the volume (a speaker button and a
  volume slider instead of a plain mute button).
- Style: a translucent bar over the whole width of the video, about 70 % opaque, or a floating pill (both are options under
  Settings > Fullscreen > Player, as is the opacity; default: the bar at 70 %).
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
  1.5 s at rest), an option. Also while paused only with the pointer. A short fade (0.15 s) is an option (default on; only with a
  compositing manager).
- Several videos: each has its own bar.
- Volume: mute is a state of its own, as in mpv. The slider sets the level and never touches mute (a muted video stays muted while
  its level goes up; the slider shows the level dimmed). The speaker shows "muted" when the video is muted or at level 0. A click
  unmutes a muted video, mutes an audible one, and brings a silent one back to its last audible level.
- From round 3: the mouse wheel over a video changes the volume (5 % per notch; Ctrl + wheel zooms the video with mpv), a right
  click plays or pauses, a click selects the tile, double and middle click work as over pictures (through the canvas). Video
  zoom, panning, the minimap and the zoom slider like pictures: built in 0.4.12 (see below).

## Code

- Pure (`src/viewer/multiview/video_bar/`): `prefs` (the settings as a global), `placement` (bar and window rectangles), `reveal`
  (when it shows, fade-out), `progress` (the span model), `time_text`, `volume` (the speaker logic), `state` (per tile: the
  millisecond switch, drags, the pending seek, the last audible level), `view` (the data between instance and window).
- Instance: `instance_video_bar.rs` (`video_bar_targets` on every tick of a host, `video_bar_input` for what the user did,
  `video_wheel`, `video_right_click`); the state lives in `VideoTile::bar`.
- Windows (`src/ui/multiview/video_bar/`): a pool of override-redirect windows (`ui/multiview_video_bar_window.slint`) like the
  action bars, solid and shaped without a compositing manager; they count as part of the Multiview for the pointer checks of the
  action bars (`bar_overlay::register_overlay_xid`). Hosts: `session_video_bar.rs` (fullscreen) and `tab_host.rs` (tab view).
- Settings: `FullscreenPlayerSettings` (`fullscreen.player`), card `ui/settings_card_player.slint` + `settings_dialog/player_bar.rs`.

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

Code: the video windows let the pointer through in every visible host mode (`apply_pointer_passthrough`), so the grid item's
own touch area handles the video like a picture. `ui/multiview/grid_bars.rs` is the grid's tick (started by
`BridgeTabGrid::sync_tab_grid_ui`, stops itself when the grid goes): it asks every Multiview in the grid for its small bars
(`instance_grid_bar.rs`: `grid_video_bar_targets`, reveal mode `Hover`, `placement::place_mini`, fixed 22 px, only inside the
visible part of the grid) and keeps the time badges (`sync_time_badges`: mpv's `osd-overlay` with ASS and
`osd-border-style=background-box`, `MpvPlayer::set_osd_text`; hidden where a small bar shows, removed when the Multiview leaves
the grid). The small bars use the window pool of `ui/multiview/video_bar` with owner `TabGrid` and the `mini` layout. Keys:
`Area::Grid` in the keymap, `main_window::dispatch` (view mode 2) -> `tab-grid-key` -> `ui/tabs/grid_keys.rs`; the mark is
`UiState::tab_grid_mark` / `TabGridItemModel::is_marked`, moved by the pure `ui/tabs/grid/mark.rs` (`neighbor` in the masonry
layout) and scrolled into view with `tab-grid-reveal`. mpv's grid bindings (clicks and the wheel reported through property
toggles) are gone.

## Video zoom (round 3, built in 0.4.12)

Videos zoom exactly like pictures: Ctrl + wheel toward the pointer, the zoom keys, dragging the picture, the minimap (with a
still of the video, refreshed every 2 s while it shows over a playing video; drag, middle click = reset, right click = the slider
bubble), the zoom slider of the media bar, and a zoomed video grows into the free room of its tile (its native window grows). The
zoom model is the pictures' (`ZoomState`, without the picture work); mpv draws the result (`video-zoom`, `video-pan-*`). The same
mpv properties would work with the render API (`docs/rendering_audit.md`), only the window growth would become a larger texture.
