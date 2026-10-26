# Video in Slint – inventory of the native-window workarounds (M0)

Every place in `crates/rustview` (`src/`, `ui/`) that exists only because a video is a native mpv window (`wid` + `vo=gpu`
in an `X11VideoSubwindow`), with the milestone of `HANDOFF.md` that replaces or deletes it, or **stays** with the reason.
Paths are relative to `crates/rustview/`. Made on 2026-10-26 from `video-in-slint` at 321be78 (= `claude` b23564b + the plan).

## How to check that the list is complete

The marker search (run from `crates/rustview/`; `\b` matters, so keep the quotes):

```bash
grep -rlE '\bwid\b|attach_to_window|X11VideoSubwindow|subwindow|overlay-add|overlay-remove|osd-overlay|override.redirect|set_input_passthrough|pointer_passthrough|cutout|register_overlay_xid|is_overlay_xid|bar_overlay|overlay_server|VideoBarHost|VideoBarWindow|video_bar/window|Away(Badge|Panel)|away-tab-id|fullscreen-tab-id|gpu-sw|gpu_context|gpu-context|minimap_cut|screenshot-raw|VideoStill|apply_shape|_NET_ACTIVE_WINDOW|host_active|place_video_window|video_window_rect|self_test|rustview-click|CLICK_MESSAGE|MBTN|input-vo-keyboard|osd_text|set_osd_text|native (mpv |video )?window|native mpv|PointerProbe|install_nonfatal_error_handler|reparent_windows|\.reparent\(|fn reparent\b|park_below|fn park\b|\.park\(|hint::show|MultiviewHintWindow|MultiviewBarWindow|MultiviewVideoBarWindow' src ui build build.rs tests | sort
```

At 321be78 it lists **92 files**; every one of them is in a table below (the false positives are in the last section). Words that
were tried and dropped because nearly every hit is unrelated: `reparent` (tag and folder "reparent" menus), `away` ("fades
away"), `park` (`parking_lot`), `xid` alone (the X11 code that stays: window state, dialogs, drag and drop), `video_bar::` (the
pure bar model, which stays). At the end of M7 the same search must list only the files marked **stays** and the false positives.

## Legend and rules

- **M1 … M7**: the milestone of `HANDOFF.md` that deletes or rewrites the entry. "M1 → M5" = first touched in M1 (made to work
  with the texture), finished in M5.
- **stays**: kept on purpose, with the reason. "stays, text M7": the code stays, only comments/docs that name native windows are
  rewritten in M7.
- Tests: unit tests of a module that is deleted go with it (the behavior they guard no longer exists; the commit names them);
  replacement code gets its own tests. The policy tests in `tests/` stay and are adapted (`window_shutdown_policy.rs` loses the
  bar/hint windows from its list when the windows go).

## 1. The video output: mpv draws into a native X11 window (M1)

| File / function | What it does today | Milestone |
|---|---|---|
| `src/viewer/x11/subwindow.rs` (`X11VideoSubwindow`: `new`, `xid`, `parent_xid`, `place_video`, `update_geometry_and_clip`, `reparent`, `clip_cutout_rectangles`, `reset_shape`/`reset_clip_shape`, `set_input_passthrough`, `set_cursor_hidden`, `show`/`hide`, `Drop`; `SubwindowGeometry`, `ViewportBounds`, `extent`) | container + inner X11 window per video below the Slint window; clip shape, holes for menus, empty input shape, invisible cursor, reparenting between windows | **M1** delete |
| `src/viewer/x11/mod.rs` (`mod subwindow`, re-exports of `X11VideoSubwindow`, `SubwindowGeometry`, `ViewportBounds`) | exports the above | **M1** (remove the exports) |
| `src/viewer/mod.rs` (re-exports `X11VideoSubwindow`, `PointerProbe`, `install_nonfatal_error_handler`, …) | viewer's public X11 surface | **M1** (subwindow), rest with its entries below |
| `src/viewer/error.rs` (`ViewerError::Subwindow` "X11 subwindow error") | error variant of the subwindow | **M1** delete or rename to the render surface's error |
| `src/viewer/mpv_engine.rs` `attach_to_window(wid)` | sets `wid`, limits `gpu-context`, switches `vo=gpu`; `attached_wid` field | **M1** replace by `attach_surface(...)` (`vo=libmpv` + render context on the video's render thread) |
| `src/viewer/mpv_engine.rs` `use_x11_gpu_contexts` + `mod gpu_context` | only mpv's X11 GPU contexts may draw into RustView's X11 window (else mpv opens Wayland windows) | **M1** delete (the render API has no GPU context of its own) |
| `src/viewer/mpv_engine/gpu_context.rs` (`is_x11_context`, `x11_gpu_contexts`, its tests) | pure filter of the `gpu-context` list | **M1** delete (with its tests) |
| `src/viewer/mpv_engine.rs` `new`: `gpu-sw=yes` | mpv's `vo=gpu` refuses llvmpipe without it | **M1** delete for the render path (the context is Slint's; with VirGL there is no llvmpipe); re-check the software-GL fallback note in CLAUDE.md |
| `src/viewer/mpv_engine.rs` `new`: `force-window=no`, `window-maximized`, `ontop`, `border` | keep mpv's own window state neutral | **M1** delete (no mpv window) |
| `src/viewer/mpv_engine.rs` `CLICK_MESSAGE` / `bind_click` (`MBTN_LEFT` → `script-message rustview-click`), `keybind MBTN_LEFT_DBL`/`MBTN_MID` → `cycle fullscreen`, the ten `Ctrl+MBTN_LEFT*` → `cycle window-maximized` | mouse on mpv's window reported as fake property toggles / script messages | **M1** delete (the video is a Slint `Image`; the canvas's own `tile-clicked`, double/middle/Ctrl clicks handle it like an image) |
| `src/viewer/mpv_engine/events.rs` `spawn_event_observer_thread`: `ClientMessage(CLICK_MESSAGE)`, the `fullscreen` and `window-maximized` interception, `current-gpu-context` record | turns those fakes into callbacks; reads mpv's GPU context for Settings > Debug | **M1** delete the click/fullscreen/maximize part; the GPU-context record becomes "render API on Slint's GL context" (see `graphics.rs`) |
| `src/viewer/mpv_engine/events.rs` `on_toggle_fullscreen`, `on_clicked`, `on_convert_to_background_tab` (+ `MpvClickCallback`, `MpvFullscreenCallback`, `MpvConvertToTabCallback` in `mpv_engine.rs`) | subscriptions for the above | **M1** delete |
| `src/viewer/multiview/tile.rs` `connect_callbacks` (the three subscriptions above), `PlayerEvent::{ToggleFullscreen, Clicked, CtrlClick}` in `src/viewer/multiview/events.rs`, their arms in `src/ui/multiview/router.rs` `route_event` ("Slint never sees clicks on the native window") | route mpv's mouse to the Multiview | **M1** delete the player events; `RoutedEvent`/`route.rs` stay for the Slint paths |
| `src/viewer/mpv_engine.rs` `new`: `input-vo-keyboard=no` (+ comment in `src/ui/hotkeys/mod.rs`) | keeps keys away from mpv's window | **M1** delete for players (there is no VO window); `mpv_engine/ab_repeat.rs` `headless` keeps it (harmless for a headless probe) — **stays** there |
| `src/viewer/multiview/tile.rs` (module doc, `VideoTile::window`, `create_video`: `X11VideoSubwindow::new(parent_xid, …)`, `attach_to_window`, `install_nonfatal_error_handler` after a failed start) | creates window + player per video tile | **M1** rewrite: the tile owns a video surface (render thread, textures) instead of a window |
| `src/viewer/multiview/teardown.rs` (`teardown_video`, `teardown_video_deferred`, `teardown_all`) | player shut down **before** its X11 window is destroyed (libmpv's Xlib handler kills the process otherwise) | **M1** rewrite: the order becomes "free the render context on its thread → `mpv_terminate_destroy` → drop the textures"; the deferred thread and its wait stay |
| `src/viewer/x11/error_handler.rs` `install_nonfatal_error_handler` (+ calls in `tile.rs`, `teardown.rs`, `self_test.rs`) | resets libmpv's process-killing Xlib error handler | **M1** keep while any mpv code path opens Xlib (hwdec `vaapi`/GLX interop may); **M7** delete if the self-check shows libmpv never installs it any more |
| `src/viewer/mpv_engine/self_test.rs` (`start`, `run`, `SelfTestOutcome`: `gpu-sw`, X11 GPU contexts, a small mpv window of its own) | Settings > Debug video self-test with the options of a real player | **M1** rewrite: render the test picture through the same video surface (offscreen FBO or a small Slint window), report the render API instead of the GPU context |
| `src/ui/settings_dialog/debug_page/actions.rs` (`install`, `self_test_checks`, `run_self_test`, test), `src/ui/settings_dialog/debug_page/mod.rs` (`Shared`, `refresh`), `src/ui/settings_dialog/debug_page/views.rs` (`self_test_rows`) | the self-test's buttons and rows | **M1** adapt to the new outcome (rows "render API", "GL renderer", "hwdec") |
| `src/system/diagnostics/graphics.rs` (`record_mpv_context`, `mpv_context`, `gl_info` "current-gpu-context") | Settings > Debug shows mpv's GPU context | **M1** replace by the render path's facts (Slint's GL renderer/version, desktop GL vs GLES, hwdec interop) |
| `src/main.rs` `main` (backend selection) | no `require_opengl()` today | **M1** add `BackendSelector::require_opengl()` (mpv drew black on GLES 2.0 in the lab); renderer per decision 1 (skia after the AMD measurement unless something speaks against it) |
| `src/viewer/multiview/instance_host.rs` `HostSurface::parent_xid` (+ `fullscreen`, `tab_view`, `tab_grid`, `same_geometry`, `HostState::hidden`) | the X window the video windows are children of | **M1** replace by the host's Slint window (which GL share group renders the textures) |
| `src/viewer/multiview/instance_host.rs` `reparent_windows`, `set_host` (calls it), `place_windows` (show/hide/`update_geometry_and_clip`), `MULTIVIEW_MIN_VIDEO_WINDOW_PX` (core `settings/schema/constants.rs`) | moves/places/clips the native windows for every layout change | **M1** delete; the texture rect comes from the `TileView` like an image's. M6 handles a host in another window |
| `src/viewer/multiview/instance_host.rs` `deactivate`, `stop_and_hide`, `apply_player_flags` (the `window.hide()` part) | hides windows of a hidden tab | **M1** drop the window part; pausing a hidden tab's players **stays** (policy) |
| `src/viewer/multiview/instance_host.rs` `apply_video_background` / `set_video_background` | mpv's background color behind a letterboxed video | **stays** (mpv still letterboxes into the texture); M1 checks it is still needed when the canvas draws the background |
| `src/viewer/tab_native.rs` (`deactivate_native`, `set_grid_view_active`, `stop_native`) | "native windows of a tab behind one interface" | **M1** rename/trim (only pause/stop remain) |
| `src/viewer/manager.rs` `hide_all_video_subwindows`, `src/viewer/manager/cutouts.rs` `hide_video_subwindows_except`, `set_all_videos_view_active` | hide every tab's video windows / pause | **M1** rename (pause only), keep the pause logic |
| `src/ui/tabs/navigation.rs` `sync_video_embedding`, `scroll_active_tab_into_view` (comment) | puts the active tab's windows into the viewer area, hides others | **M1** trim to "show the active tab's Multiview, pause the others" |
| `src/ui/multiview/cursor.rs` (module doc; `set_cursor_hidden` on the video windows through `MultiviewInstance::set_cursor_hidden`) | hides the resting cursor also over the native windows (invisible X11 cursor) | **M1** drop the video-window part; hiding through winit on the Slint window **stays** |
| `src/viewer/multiview/instance_host.rs` `set_menu_open`, `set_modal_locked`, `apply_pointer_passthrough`, `HostState::{menu_open, modal_locked}`; `src/viewer/multiview/instance_hover.rs` `tick` (calls passthrough every tick) | empty input shape on the video windows so the host (canvas, lock overlay, menu backdrop) gets the pointer | **M1** delete the passthrough; `modal_locked`/`menu_open` **stay** as behavior inputs (no bars while a dialog/menu is open), fed by the presenters instead of the cutout list |
| `ui/multiview_types.slint` `MultiviewTileView.kind` ("1 = video (native window)"), `MultiviewBarButtonView` comment | tile kinds; video tiles are holes in the canvas | **M1** video tiles get an `image` (the texture) like image tiles |
| `ui/multiview_canvas.slint` (module doc, `gutter-swap`, the focus/feedback comments "a video's native window covers this") | draws everything but videos | **M1** draws the video `Image`; the comments go with M4 |
| `ui/multiview_fullscreen_window.slint` (module doc, the lock overlay comment `size-changed` L165) | "video tiles are native mpv windows placed on top" | **M1** (doc), lock overlay **stays** |
| `ui/main.slint` `multiview-viewport-changed`, `refresh-medium` comment (L383-386: "Rust places the native video windows") | the viewer area's geometry for Rust | **stays** (the layout still needs the area), text **M1** |
| `src/system/window_system.rs` (doc), `src/system/window_system/choice.rs` (`Choice` doc: "native video windows … degrade") | the window-system choice explains XWayland by the native windows | text **M1**, policy **M7** (decision 3) |

## 2. In-window Slint UI over a video: cutouts (M1)

With the video as a texture, Slint's own context menus, tooltips and popups are drawn over it; every hole goes.

| File / function | What it does today | Milestone |
|---|---|---|
| `src/ui/context_menu/cutouts.rs` (`calculate_all_active_menu_cutouts`, `make_rounded_cutout_rectangles`), `src/ui/context_menu/mod.rs` (`mod cutouts`) | menu rectangles (real height, rounded) to cut out of the video windows | **M1** delete |
| `src/ui/context_menu/registration.rs` `register_popup_events`, `src/ui/context_menu/show.rs` `open_custom_menu`, `src/ui/context_menu/dispatcher.rs` `register_action_dispatcher` (`clear_menu_cutouts`) | set/clear the cutouts when a menu opens/closes | **M1** delete the cutout calls (keep the menu-open flag, see §1) |
| `src/ui/bridge/subsystem_callbacks.rs` `register_tooltip_overlay_callbacks` (`global-tooltip-state-changed` → `set_tooltip_cutouts`) | holes for tooltips | **M1** delete (and the Slint callback if nothing else uses it) |
| `src/viewer/manager/cutouts.rs` (`set_menu_cutouts`, `clear_menu_cutouts`, `apply_menu_open_to_all_subwindows`, `set_tooltip_cutouts`, `clear_tooltip_cutouts`, `get_combined_cutouts`, `reapply_active_cutouts_to_subwindow`, `apply_combined_cutouts_to_all_subwindows`), `src/viewer/manager.rs` (`mod cutouts`, `active_menu_cutouts`/tooltip fields, `new`) | the manager's cutout state, applied to every tab | **M1** delete (the two hide/pause helpers move, see §1) |
| `src/viewer/tab_native.rs` `apply_overlay_cutouts`, `set_menu_open` | per tab | **M1** delete / keep the flag |
| `src/viewer/multiview/instance_host.rs` `HostSurface::overlay_cutouts`, `set_overlay_cutouts`, `apply_cutouts`, `apply_all_cutouts`, `reproject` (calls), `set_host` (cutout-only change), `park` (`reset_clip_shape`) | per video window | **M1** delete |
| `src/ui/multiview/grid_host.rs` (`GridHost` cutout field, `new`, `place`), `src/ui/multiview/tab_host.rs` `run`, `sync` (cutouts into `HostSurface`) | hand the cutouts to the instance | **M1** delete the cutout part |
| `ui/tab_grid_item.slint` `context-menu-requested` (L240: "XShape cutouts punch through the MPV window … opaque background"), module doc ("solid video subwindow placeholders") | opaque black area under the video window | **M1** the item shows the texture; text M1 |
| `ui/smooth_scroll_view.slint` (doc: "frame-synchronized viewport positioning for embedded native subwindows") | scroll sync for native windows | **stays** (smooth scrolling), text **M6** |

## 3. Video control bar incl. the A-B group: windows of their own (M2)

**Done in M2 (0.5.8):** every row of this section. The bar is `MultiviewVideoBar` (`ui/multiview_video_bar.slint`) drawn by the
canvas from one `VideoBarModel` per host (`ui/multiview/video_bar/`, hosts `session_video_bar.rs`, `tab_video_bar.rs`,
`grid_bars.rs`); `window.rs`, the pool, `close_ab_popups`, `register_overlay_xid`/`is_overlay_xid`, `effective_prefs` (the compositor
no longer matters) and the shutdown row are gone; `placement` keeps the bar's rectangle (relative to the canvas now) without the popup
room. The tab grid's small bars moved too (they used the same pool), so their row in §7 is done as far as the bar goes; the pointer
probe of `grid_bars` stays for M6. The popups are elements of the canvas, not `PopupWindow`s (see `docs/video_controls.md`).

| File / function | What it does today | Milestone |
|---|---|---|
| `src/ui/multiview/video_bar/window.rs` (`VideoBarWindow`, `XState`, `apply_shape`, `takes_pointer_at`, `new` (override-redirect), `show` (`host_state`, `in_front`), `follow_popup` (window resized while a popup is open: the flicker, gap and caret of the saved menu), `hide`, `sync_prefs`, `sync_view`, `sync_ab`, `place`, `adopt_native_window` (`register_overlay_xid`), `host_state` (`is_overlay_xid`), `refresh_glass`, `connect`, `connect_ab`, `logical_rect`, its test) | one override-redirect ARGB window per shown bar, shaped without a compositor, keyboard grab for popups | **M2** delete; the bar is a component in `multiview_canvas.slint` |
| `src/ui/multiview/video_bar/mod.rs` (`VideoBarHost`, `effective_prefs`, `update`, `hide`, `hide_except`, `shutdown`, `window_for`, `all`; the window pool) | pool of bar windows, one per tile by `TileKey` | **M2** delete the pool; `effective_prefs` moves to the presenter |
| `ui/multiview_video_bar_window.slint` (`MultiviewVideoBarWindow`, `AbRegionView`, `clicked` comment "native mpv window") | the bar window: bar, time bubble, A-B tab, flags, label, Save bubble, Load menu | **M2** becomes `MultiviewVideoBar` (a component); the Save bubble and Load menu become `PopupWindow`s anchored to their button |
| `ui/multiview_video_bar_ab.slint` | the A-B group's parts | **M2** reused inside the component (no window marker any more) |
| `src/ui/multiview/session_video_bar.rs` (`update_video_bars`, `on_video_bar_input`), `src/ui/multiview/tab_host.rs` `update_video_bars`, `video_bar_after`, `suspend`, `on_tick` (bar part) | hosts feed the windows every tick | **M2** feed the canvas model instead |
| `src/ui/multiview/session.rs` `Session::video_bar_input` (`InputHandler`), `shutdown` (`video_bar::hide`) | input handler of the windows | **M2** the canvas' callbacks |
| `src/viewer/multiview/instance_video_bar.rs` `collect_bar_targets` (`video_window_rect`), `video_bar_targets` | where each bar goes (screen rects for windows) | **M2** rects relative to the canvas; the pure decisions **stay** |
| `src/viewer/multiview/video_bar/mod.rs` (doc: "a window of its own above the native mpv window"), `video_bar/placement.rs` (window rectangle around the bar) | pure placement incl. the window margin | **stays**, window margin part **M2** |
| `src/viewer/multiview/instance_ab_repeat.rs` `close_ab_popups` (called by every click on the Multiview because "the 50 ms pointer poll misses short clicks") | closes A-B popups from outside | **M2** delete (in-window popups close themselves) |
| `src/ui/bridge/mod.rs` `new`, `src/ui/bridge/settings_subscriber.rs` `register_settings_subscriber` (`video_bar::` / `bar_overlay::set_tooltip_settings`) | push settings into the windows | **M2/M3** push into the canvas model |
| `src/ui/shutdown/windows.rs` `WINDOWS` (`MultiviewVideoBarWindow`, `MultiviewBarWindow`, `MultiviewHintWindow`) + `tests/window_shutdown_policy.rs` | shutdown closes the overlay windows | **M2** (video bar), **M3** (bars, hint) remove the rows |
| Known problem: A-B "saved" menu flickers, has a gap, caret in the middle | `follow_popup` resizes/re-shapes the ARGB window | **M2** fixed by construction; check all three (ACCEPTANCE C) |

## 4. Media bar, primary bar, menus, zoom bubble, hint: windows of their own (M3)

**Done in M3 (0.5.9):** every row of this section. The bars are `MultiviewActionBar` (`ui/multiview_action_bar.slint`) drawn by the
canvas from one `ActionBarModel` per host (`ui/multiview/action_bar/`, events through the `ActionBarBus` global), the drop-downs and the
zoom slider are elements of the canvas below their bar (a backdrop closes a drop-down), the hint is `MultiviewHint` in the canvas
(`ui/multiview/hint.rs` only sets its text and ends its hold). Deleted: `bar_overlay` (+ `overlay.rs`), `bar_hosting`, `bar_menu`,
`ui/multiview_bar_window.slint`, `ui/multiview_hint_window.slint` (now `multiview_hint.slint`, a component), the override-redirect hook
in `main.rs`, `HintAnchor`, `near_bar_corner`/`xrectangle`/`rounded_region` and the overlay window constants, `viewer/x11/rectangle.rs`,
the `state-changed` callback of `GlobalTooltipLayer`, the shutdown rows of both windows. `overlay_server.rs` became `pointer_warp.rs`
(`PointerWarp`: only what `pointer_lock` needs). The fullscreen and the tab view get the pointer from their window's events
(`ui/multiview/pointer_events.rs`, winit `CursorMoved`/`CursorLeft`); `PointerProbe` is left for the tab grid (M6). The bar sizes come
from `MULTIVIEW_BAR_*` constants that Slint gets through `MultiviewBarMetrics` (`bar_placement::bar_size_logical`).

| File / function | What it does today | Milestone |
|---|---|---|
| `src/main.rs` `main`: `with_winit_window_attributes_hook(bar_overlay::window_attributes_hook)` | the only place that makes Slint windows override-redirect | **M3** delete (and with it the reason the hook exists); keep the `BackendSelector` for decision 1 |
| `src/ui/multiview/bar_overlay.rs` (`OVERLAYS`, `OVERLAY_XIDS`, `LAST_CLICKED`, `NEXT_IS_OVERLAY`, `TOOLTIPS`, `MENU_TIMER` + `MENU_POLL_MS`, `window_attributes_hook`, `create_override_redirect`, `register_overlay_xid`, `is_overlay_xid`, `set_tooltip_settings`, `prepare`, `prepare_media_bars`, `add_overlay`, `overlay_for`, `show`, `open_menu`, `poll_menu`, `service_menus`, `open_menu_keys`, `sync_zooms`, `close_menu`, `close_clicked_menu`, `hide`, `hide_kind`, `hide_kind_except`, `shutdown`, `all_overlays`, `overlays_of`), `src/ui/multiview/mod.rs` (`mod bar_overlay`) | pool of media bar windows + primary bar window; menu pointer poll every 16 ms; tooltip settings per window | **M3** delete |
| `src/ui/multiview/bar_overlay/overlay.rs` (`Overlay`, `XState::apply_shape`, `clear_menu`, `new`, `apply_tooltip_settings`, `assign`, `show` (`in_front` / `host_active`), `menu_zone_in_host`, `pointer_state`, `sync_zoom`, `host_active` (`_NET_ACTIVE_WINDOW`), `host_in_use`, `service_menu` (`bar_menu::OutsideClick`), `close_menu`, `host_state`, `sync_content`, `place`, `refresh_glass`, `hide`) | one bar window: shape, glass, placement, "only over its host / only while the host is active", menu servicing | **M3** delete |
| `src/ui/multiview/bar_hosting.rs` (`BarHost`, `of_instance`, `update`) | each tick: ask the instance, show/hide bar windows | **M3** becomes the canvas model sync (or goes into the presenters) |
| `ui/multiview_bar_window.slint` (`MultiviewBarWindow`; `action-clicked`, `menu-item-clicked`, `menu-changed`), `ui/multiview_bar.slint` (`arrow-activated` comment), `ui/multiview_bar_menu.slint` (the speech-bubble drop-down) | the bar window and its drop-down | **M3** the bar becomes a canvas component, the drop-down a `PopupWindow` (caret at the opener) |
| `ui/multiview_zoom_slider.slint` (used inside the bar window's bubble) | zoom slider bubble below the bar | **M3** the same component inside the canvas/popup |
| `src/viewer/multiview/bar_placement.rs` (doc "a native video window …", `place_bar`, `place_bar_in`, `near_bar_corner`, `in_reveal_zone`, `xrectangle`, `rounded_region`) | pure: bar rect, reveal zone, window margin, the X shape of rounded corners | **M3** keep `place_bar*`/`near_bar_corner`/`in_reveal_zone` (canvas coordinates), delete `xrectangle`/`rounded_region` |
| `src/viewer/multiview/bar_menu.rs` (`OutsideClick::{arm, poll}`) | "the menu lives in an override-redirect window that cannot take a grab": detects a press outside from polled pointer states | **M3** delete |
| `src/ui/multiview/handle.rs` `toggle_open_with`, `shutdown` (`bar_overlay::shutdown`, `video_bar::shutdown`); `src/ui/multiview/handle_edit.rs` `toggle_save_menu` | open the Open-With / Save drop-downs in the bar windows | **M3** open the canvas popups |
| `src/ui/multiview/session_input.rs` `update_bar_overlay`, `on_tick`, `on_click` (`bar_overlay::close_menu`), `pointer_in_window`, `host_surface`; `src/ui/multiview/session.rs` `prepare_window` (`bar_overlay::prepare`), `refresh`, `shutdown`, `launch`/`new` (comments) | the fullscreen's bar part | **M3** |
| `src/ui/multiview/tab_host.rs` `update_bar_overlay`, `on_click`, `suspend`, `refresh`, `on_tick` | the tab view's bar part | **M3** |
| `src/ui/multiview/router.rs` `route_event` (`bar_overlay::close_menu` on a video click) | a click on a video closes the bar menu (the window cannot see it) | **M3** delete (the popup closes itself) |
| `src/ui/multiview/hint.rs` (`HintWindow`, `create` (override-redirect), `shape`, `show`, `apply_x`, `retry_x`, `end_hold`, `end_fade`, `shutdown`), `ui/multiview_hint_window.slint` (`MultiviewHintWindow`) | the "Select a medium first" hint as a window of its own, shaped/solid without compositor, input passthrough | **M3** becomes a canvas element (same timing, `viewer/multiview/hint.rs` stays pure) |
| `src/ui/multiview/commands.rs` `refused` (`hint::show(anchor, kind)`) | shows the hint | **M3** sets the canvas' hint instead |
| `src/viewer/multiview/hint.rs` `HintAnchor` (screen anchor for the window) | where the hint window goes | **M3** canvas coordinates; the rest **stays** |
| `src/viewer/x11/overlay_server.rs` (`OverlayServer`: `compositor_running`, `window_depth`, `origin_in_root`, `pointer_and_buttons`, `top_level_at_pointer`, `window_size`, `active_window`, `top_level_of`, `raise`, `focus`, `set_input_region`, `set_bounding_region`, `combine`) | X side of all floating windows | **M3** delete everything but what the pointer lock needs (`warp_pointer`, `hide_cursor`, `show_cursor`): those move to `pointer_lock`'s own X helper or **stay** under a new name |
| `src/viewer/x11/pointer.rs` (`PointerProbe::{position, position_in}`), `src/viewer/mod.rs` export; users: `src/ui/multiview/session.rs` (`Session::probe`, `populate`), `session_input.rs` `pointer_in_window`, `tab_host.rs` (`TabHost::probe`, `sync`, `on_tick`), `grid_bars.rs` (`GridBars`, `ensure_running`, `tick`, `on_input`) | the 50 ms tick asks the X server where the pointer is, because mpv swallows pointer events and the bars are other windows | **M3** delete for hover/bars (Slint's `has-hover`/`changed mouse-x`); the grid's part **M6**. The tick itself **stays** for aspect probing, zoom polling and the mark reveal; "hide the resting cursor" uses Slint's pointer events |
| `src/viewer/multiview/instance_hover.rs` (doc: "mpv swallows the pointer events … asking the X server"), `tick`, `poll_pointer`, `tile_at_host`, `canvas_tile_at`, `media_bar_targets` | hover from the polled pointer | **M3** the pointer comes from the canvas' events; the pure tile/room tests **stay** |
| Settings "Show tooltips / delay" pushed into each bar window (`bar_overlay::set_tooltip_settings`, `Overlay::apply_tooltip_settings`) | Slint globals are per window | **M3** delete (the canvas is in the host window, whose tooltip layer already has the settings) |

## 5. What mpv draws over the picture (M4)

**Done in M4 (0.5.10):** every row of this section. The feedback pulse (rating, save, the speaker of the mute key), the corner marks
and the volume bar (`TileView::volume`, `volume-*` of `MultiviewTileView`) are drawn by `ui/multiview_canvas.slint` over videos as over
pictures; the grid's time badge is a row of a `TimeBadgeModel` per Multiview (`ui/multiview/time_badges.rs`, filled by the grid tick
from `grid_time_badges`) drawn by the canvas of the grid item (done in M4, not with M6). Deleted: `mpv_engine/overlay.rs`, `rating.rs`,
`osd_text.rs`, `feedback_overlay.rs`, `focus_overlay.rs`, `volume_overlay.rs`, the overlay sync functions and the `VideoTile` fields
`feedback_overlay`, `focus_overlay`, `volume_overlay`, `time_badge`. mpv draws nothing over the picture any more.

| File / function | What it does today | Milestone |
|---|---|---|
| `src/viewer/mpv_engine/overlay.rs` (`show_overlay`, `remove_overlay`, the id table incl. `VOLUME_BAR_OVERLAY_ID`, focus corner ids) | `overlay-add` / `overlay-remove` of premultiplied BGRA bitmaps | **M4** delete |
| `src/viewer/mpv_engine/rating.rs` | rating/save icon through `overlay-add` | **M4** delete |
| `src/viewer/multiview/feedback_overlay.rs` (`OverlayState`, `icon_pixels`, …), `src/viewer/multiview/instance_feedback.rs` (doc + the video branch of `show_feedback`/`sync_feedback_overlays`), `VideoTile::feedback_overlay` | the feedback pulse on a video, rasterized and faded for mpv | **M4** delete; the canvas draws `TileView::feedback` for videos like for images |
| `src/viewer/multiview/focus_overlay.rs` (`WindowMarks`, the marks cut to `video_window_rect`), `instance_focus.rs` `sync_focus_overlays` (+ calls in `reproject`, `tick`), `VideoTile::focus_overlay`, `ui/multiview_focus_marks.slint` (doc) | the part of the corner marks that lies over a video | **M4** delete; `MultiviewFocusCorners` draws all four corners above the video |
| `src/viewer/multiview/volume_overlay.rs` (`VolumeOverlayState`, `show`, `hide`, the bitmap), `instance_volume_feedback.rs` (video part), `VideoTile::volume_overlay` | the volume bar drawn by mpv | **M4** a canvas element; `volume_feedback.rs` (pure geometry/timing) **stays** |
| `src/viewer/mpv_engine/osd_text.rs` (`OsdText`, `set_osd_text`: `osd-overlay` ASS + `osd-border-style=background-box`), `mod osd_text` in `mpv_engine.rs`, `VideoTile::time_badge`, `src/viewer/multiview/instance_grid_bar.rs` `sync_time_badges` | the grid's time badge drawn by mpv (and the side effect: the script's OSD texts get a box too) | **M4** a Slint badge in the grid item (with M6's grid work if simpler) |
| `src/viewer/multiview/view.rs` `TileView` (doc), `ui/multiview_canvas.slint` comments | "a video tile gets the icon from mpv itself" | **M4** text |

## 6. Video zoom and minimap (M5)

| File / function | What it does today | Milestone |
|---|---|---|
| `src/viewer/multiview/video_placement.rs` (`VideoPlacement`: mpv's frame = the whole room, the container window clips to the medium's place) | avoids the stale stretched frame when the native window is resized at 0 ↔ zoomed | **M1 → M5**: M1 keeps the model (FBO = frame size, the `Image` clipped to the visible part), M5 replaces it by a texture of the room's size |
| `src/viewer/multiview/instance_video_zoom.rs` (doc, `video_window_rect`, `video_placement`, `sync_video_zoom`, `place_video_window`, `note_placement`, `video_cutouts`) | turns the zoom model into mpv's view + window geometry + minimap cutouts | **M1 → M5**: the window calls go in M1, the "window grows into the room" logic in M5; `zoom/mpv_view.rs` (`video-zoom`/`video-pan-*`) **stays** |
| `VideoTile::minimap_cut`, `src/viewer/multiview/instance_zoom.rs` `minimap_px` ("for the cutouts of a video window"), `src/viewer/multiview/instance_tiles.rs` `reload_video` (clears `minimap_cut`), `instance_host.rs` `apply_cutouts` | the minimap and its bubble cut out of the video window (the canvas draws them below) | **M1** delete (the canvas draws the minimap above the texture) |
| `src/viewer/mpv_engine/still.rs` (`still`, `raw_frame`, `to_rgb`: `screenshot-raw` through `libmpv2-sys`), `PlayerEvent::VideoStill`, `instance_video_zoom.rs` `request_still`, `on_video_still`, `VideoTile::{still_pending, still_at}`, `router.rs` arm | minimap stills on a worker thread every 2 s while it shows | **M5** draw the minimap from the video's own texture (no CPU copy); delete `still.rs` if nothing else needs it |
| `src/viewer/multiview/instance_video_bar.rs` `collect_bar_targets` (`video_window_rect`) | the bar follows the visible part of the native window | **M2** (canvas rect), **M5** (zoomed room) |

**Done in M5 (0.5.11):** every row of this section, with one change of plan. A texture of the room's size with mpv's own zoom
(`video-zoom`/`video-pan-*`) cannot feed the minimap: it only holds the zoomed part. So mpv now always renders the whole, fitted picture
into a texture of the picture's size (at most the video's own size) and the canvas places and scales it like a picture
(`VideoPlacement::{plain, zoomed}`: frame = where the whole picture is drawn, clip = the medium's place or the room; `texture_size`,
`keeps_texture` while a zoom animates; `size_video_textures`). The minimap is the same texture in small (`MinimapPicture::Video`,
`mm-video-row`). Deleted with it: `zoom/mpv_view.rs`, `MpvPlayer::{set_view, set_subtitle_margins}`, `VideoTile::{zoom_view, placement}`,
`still.rs` and everything of the stills. `zoom_video_by` stays as the fallback before mpv reported the size (undone when the zoom model
arrives). The control bar lies at the bottom of the clip (the place, a lone video's is the whole view, or the room while zoomed).

## 7. Tab grid and the handover fullscreen ↔ tab (M6)

| File / function | What it does today | Milestone |
|---|---|---|
| `src/ui/tabs/grid_bridge.rs` (`sync_tab_grid_ui`, `sync_video_subwindows`, `handle_item_left_clicked`, `handle_item_close_clicked`, `handle_scrolled`) | places every tab's video windows on its masonry tile, clipped to the viewport, re-placed on scroll | **M1** draw the textures in the grid items (minimal); **M6** delete the per-scroll re-placement (Slint clips and scrolls) |
| `src/ui/tabs/grid/canvas.rs` (doc: "one definition for the native video windows and the Multiview") | the canvas rect of a grid item | **stays**, text M6 |
| `src/ui/multiview/grid_host.rs` (`GridHost::{new, main_xid, place}`) | host surface of a grid item (parent xid, clip, cutouts) | **M1** (xid/cutouts), **M6** (clip) |
| `src/ui/multiview/grid_bars.rs` (`GridBars`, `ensure_running`, `mini_prefs`, `tick` (pointer probe), `update`, `on_input`, `stop`) | the grid's small video bars as bar windows, the grid tick | **M6** small bars as grid-item components, tick only for badges/aspects if still needed |
| `src/viewer/multiview/instance_grid_bar.rs` `grid_video_bar_targets` (doc: "lets the pointer through like the others") | pure targets of the small bars | **M6** canvas coordinates; **stays** as logic |
| `ui/fullscreen_away.slint` (`AwayBadge`, `AwayPanel`), `ui/tab_grid_item.slint` (`away`, `AwayBadge` on video tiles), `ui/tab_grid_view.slint` (`away-tab-id`), `ui/main.slint` (`AwayPanel`, `fullscreen-tab-id`, `sidebar-panel-tab-drag-cancelled` reset), `src/ui/multiview/handle.rs` `notify_closed`, `session.rs` `launch`/`finish_close` | stand-ins while a tab's videos play in the fullscreen window ("a native mpv window cannot be in two places") | **stays** (decision 2: the fullscreen stays a separate window, and a texture cannot be in two GL share groups either); M6 checks they show only for a video that plays in the fullscreen window |
| `src/ui/multiview/session_convert.rs` (`adopt`, `park_below_main_window`), `src/viewer/multiview/instance_host.rs` `park` (`.reparent(...)`), `src/viewer/manager.rs` `get_viewport_rect`/`set_main_viewport_info` (where parked windows go), `session.rs` `finish_close` (`park_below`) | moves the running video windows below the main window BEFORE the fullscreen window is hidden (hiding destroys children) | **M1** minimal: re-create the render context in the other window; **M6** the last frame as a still meanwhile; delete the X reparenting (decision 2: the fullscreen stays a separate window, so the handover stays) |
| `ui/smooth_scroll_view.slint` doc | see §2 | text **M6** |

**Done in M6 (0.5.12):** the tab grid places every Multiview in grid content coordinates (`canvas_rect` without scroll, `GridHost`
without xid and visible rectangle), so a scroll step re-places nothing (`sync_video_subwindows` and the cached-rect recompute of
`handle_scrolled` are gone; it only tells `grid_bars` that the content moved). The small bars take the pointer from the main window's
pointer filter (`grid_bars::on_pointer`, forwarded by `tab_host`) and move it and the visible part into the content (`to_content`);
`PointerProbe` (`x11/pointer.rs`) is deleted, the grid tick stays for the time badges and the bars' fade. `HostSurface` lost
`parent_xid` and `clip`, `park` its xid and rectangle, the tab manager its parked-viewport cache. Handover: the render thread reads the
newest picture back before it leaves a window (`keep_still`, `read_still`, `VideoTarget::read_into`), `VideoSurface::attach` waits for
it (at most 150 ms) and the new window shows it until mpv reports a video frame in the new render context (`Bound::has_frame`; mpv's
first renders after the track is selected again only clear to black). `AwayBadge`/`AwayPanel` stay (decision 2). Not changed: the
tab grid still builds every item's model on a sync (CLEANUP #8, the double placement is gone with `sync_video_subwindows`).

## 8. Window system and docs (M7)

| File / function | What it does today | Milestone |
|---|---|---|
| `src/system/window_system.rs` (`decide`, `select_backend`, doc), `src/system/window_system/choice.rs` | forces XWayland in a Wayland session "because native mpv windows exist only on X11" | **M7** native Wayland becomes the default (decision 3), switched together with the user in a Plasma Wayland session; XWayland stays selectable |
| core `diagnostics/dependencies.rs`, `health.rs` (`XWayland` rows) | Settings > Debug explains the choice | **M7** text for native Wayland as the default |
| `docs/rendering.md`, `docs/video_controls.md`, `docs/mpv_ab_repeat.md` (OSD/`osd-border-style` remark), `docs/multiview_focus.md` (mpv marks), CLAUDE.md (Multiview chapter, VM graphics note on `gpu-sw`, the black-video quirk) | describe the native-window design | **M7** rewrite |
| `src/viewer/x11/error_handler.rs` | see §1 | **M7** decide |

**Done in M7 (0.5.13):** the docs (`docs/rendering.md` rewritten, `docs/video_controls.md`, `docs/multiview_focus.md`,
`docs/mpv_ab_repeat.md`, `docs/hotkeys.md`, `docs/debug_page.md`, README "Platform", CLAUDE.md: the VM graphics note, the testing
quirk, "Platform", the Multiview, shutdown, feedback and dialog chapters) describe videos as textures of the canvas. The window-system
texts (`window_system.rs` doc and log line, Settings > Debug's "Native Wayland" row) no longer say that videos are missing on native
Wayland. The switch of the default itself waits for the joint test with the user (decision 3, `WAYLAND.md`); on the way the Multiview
fullscreen was made to open on native Wayland at all (`ui/multiview/fullscreen_mode.rs`; it waited for an X11 window id). `error_handler.rs`
**stays**: libmpv may still install its process-killing Xlib handler when a hardware decoder's interop used Xlib (VAAPI over GLX/X11
EGL), and the VM has no VAAPI to prove that it never does; the call after a teardown costs nothing.

## 9. X11 code that stays (not a video workaround)

| File | Why it stays |
|---|---|
| `src/viewer/x11/wm_state.rs` | EWMH maximize/fullscreen/monitor of real top-level windows (startup without resize, fullscreen on a monitor) |
| `src/viewer/x11/windows.rs` (`get_raw_window_xid`, `get_slint_window_xid*`) | the X window of a Slint window for `wm_state`, dialogs and XDND |
| `src/viewer/x11/dialog_hints.rs`, `src/ui/dialog_parent.rs` | `WM_TRANSIENT_FOR` / modal dialogs on X11 |
| `src/viewer/x11/xdnd.rs`, `src/viewer/x11/ghost.rs` (override-redirect drag ghost) | dragging files to other programs; the ghost must follow the pointer over other applications |
| `src/ui/multiview/pointer_lock.rs` (+ the warp/cursor part of `overlay_server.rs`) | minimap drag: hide, hold and warp the pointer (Blender-style continuous grab); Wayland falls back already |
| `src/viewer/x11/session.rs`, `server_facts.rs` | private x11rb connection, compositor facts for Settings > Debug |
| `src/viewer/multiview/instance_hover.rs` `probe_aspects` | corrects aspect ratios from mpv's `video-params` (needed with any output) |
| `src/viewer/mpv_engine/ab_repeat.rs` `headless` (`self_test::user_config_dir`, `input-vo-keyboard`) | headless mpv for "Check again"; no window involved |

## 10. False positives of the marker search (and the result at the end of M7)

| File | Hit | Why it is not a workaround |
|---|---|---|
| `src/viewer/multiview/view.rs` | "native window" in a doc line | text only, see §5 (M4) |
| `src/viewer/multiview/video_bar/mod.rs` | doc "above the native mpv window" | pure model, text M2 |
| `src/ui/hotkeys/mod.rs` | `input-vo-keyboard` in the doc | text, M1 |
| `src/ui/image_edit/save_flow.rs` | `fn park(waiting)` | parks a save continuation, unrelated |
| `src/viewer/mpv_engine/ab_repeat.rs` | `self_test`, `input-vo-keyboard` | headless probe, stays (§9) |
| `src/system/diagnostics/graphics.rs` | `current-gpu-context` | listed in §1 (M1) |

**Result at the end of M7 (0.5.13): 18 files** (from 92), each kept on purpose:

| File | Hit | Why it stays |
|---|---|---|
| `src/ui/image_edit/save_flow.rs` | `fn park` | parks a save continuation, unrelated |
| `src/ui/multiview/commands.rs` | `hint::show` | sets the canvas hint (`MultiviewHint`), no window |
| `src/ui/multiview/session_convert.rs`, `src/viewer/multiview/instance_host.rs` | `.park(` / `fn park` | hands a Multiview from the fullscreen back to the main window (decision 2: the fullscreen stays a separate window); no X reparenting any more |
| `src/ui/settings_dialog/debug_page/{actions,mod,views}.rs`, `src/viewer/mpv_engine.rs`, `src/viewer/mpv_engine/ab_repeat.rs` | `self_test`, `input-vo-keyboard` | the render-API self-test of Settings > Debug; the headless mpv of `ab_repeat` (§9) |
| `src/viewer/mod.rs`, `src/viewer/x11/mod.rs`, `src/viewer/x11/error_handler.rs`, `src/viewer/multiview/teardown.rs` | `install_nonfatal_error_handler` | decided in M7: stays (§8) |
| `src/viewer/x11/ghost.rs` | `override_redirect` | the drag ghost window (§9) |
| `ui/fullscreen_away.slint`, `ui/main.slint`, `ui/tab_grid_item.slint`, `ui/tab_grid_view.slint` | `AwayBadge`, `AwayPanel`, `fullscreen-tab-id`, `away-tab-id` | stand-ins while a tab's videos are in the fullscreen window (decision 2, §7) |

Numbers for the record (`wc -l` of the whole files): bar, video bar and hint windows with their X side ~3 400 lines, subwindow +
cutouts + GPU contexts ~870, mpv-drawn overlays ~950, zoom placement and stills ~520, plus the parts spread over hosts and
presenters; the lab
estimated −5 000 to −7 000 and +1 000 for the whole port.
