# Cleanup findings (for the branch after the acceptance)

User decision 2026-10-26 (M0 questionnaire, question 4): a general cleanup and optimization pass of RustView comes as a
branch of its own **after** the acceptance of `video-in-slint`. Every milestone logs here what it notices outside the video
work (one line each: where, what, why it matters, a rough size). Nothing here is done on this branch. At the end the list is
presented to the user as a questionnaire, and the user decides what is done.

| # | Found in | Where | What | Why it matters |
|---|---|---|---|---|
| 1 | M0 | `src/viewer/mpv_engine/events.rs` `spawn_event_observer_thread` | `time-pos`, `duration`, `pause`, `volume`, `eof-reached`, `idle-active`, `fullscreen`, `window-maximized` are read with `get_property` every 20 ms (100 ms hidden) per player instead of `observe_property` events | CPU and wake-ups per video while nothing changes |
| 2 | M0 | `src/ui/multiview/session.rs`, `tab_host.rs` (`MULTIVIEW_TICK_MS` = 50) | a 50 ms tick runs while a Multiview is shown (aspect probing, zoom polling, mark reveal); after M3 the pointer part is gone, the rest could be event-driven | idle CPU, the "no polling" rule of CLAUDE.md |
| 3 | M0 (lab) | Slint 1.18 on X11 | renders twice per isolated redraw request (`TimerBasedFrameThrottle`); affects every window | worth an upstream report; double GPU work per video frame once videos are drawn by Slint |
| 4 | M0 | `src/viewer/mpv_engine.rs` | stale docs ("native OSC", "unreserved mouse wheel pass-through") and the `MpvPlayer` struct with ten callback lists | readability |
| 5 | M0 | `src/ui/multiview/session.rs` (697 lines), `src/ui/multiview/video_bar/window.rs` (694) | over the ~600-line limit of CLAUDE.md (the second goes in M2 anyway) | file size rule |
| 6 | M1 | `src/viewer/mpv_engine.rs` `MpvPlayer::new` | `load-osd-console` is no mpv 0.41 option (it is `load-console`; mpv ignores the unknown name), `script-opts` is set twice, and the comments still speak of the OSC deadzone although the OSC is off | readability, an option that silently does nothing |
| 7 | M1 | `src/viewer/mpv_engine.rs` `MpvPlayer::set_interactive` | public, never called (a wrapper of `set_tab_grid_mode`) | dead code |
| 8 | M1 | `src/ui/tabs/grid_bridge.rs` `sync_tab_grid_ui` | places every Multiview twice per sync (once while building the items, once in `sync_video_subwindows`); the second pass also runs on every scroll step | needless work per scroll (M6 removes the per-scroll pass for videos; the double placement while syncing stays) |
| 9 | M1 | VM (`virt-manager`/Spice with GL) | when the viewer of the VM is not showing the guest, every GL swap in the guest is throttled to ~1 frame/s (glxgears 0.9 FPS), which blocks Slint's UI thread for a second per frame | not RustView's bug, but it makes every UI check in the VM useless while it lasts; worth a line in CLAUDE.md's VM notes |
| 10 | M2 | `ui/tooltips.slint` `TooltipHost` | shows a tooltip only when its owner calls `on-hover-enter`/`on-hover-leave`; nothing enforces that, so the A-B buttons and the menu's buttons had silent tooltips from 0.5.4 to 0.5.7 (fixed in M2). A policy test (every `TooltipHost` has a matching `on-hover-enter` in its file) or `TooltipArea` everywhere would stop it from coming back | tooltips that silently never show |
| 11 | M2 | test environment `bin/gen_ab_repeats.py` | has no `--help`: any call (also `--help`) rewrites the sample A-B repeats in `~/.config/mpv/ab_repeat.json` (it keeps a `.bak`) | outside RustView; surprising for a quick look at its options |
| 12 | M3 | crop tool (`rustview_core::image_edit::crop_tool`) | a fresh crop tool covers the whole picture, so a drag inside it only moves the rectangle (clamped: nothing happens); a new rectangle can only be made from a handle or the ratio button. Found while testing the Save drop-down | the first drag of a new crop does nothing, which looks broken |
| 13 | M3 | `ui/multiview_bar_menu.slint` `translucent` | the drop-downs are always translucent now; only the minimap's slider bubble sets it to false, so the property could become a plain fill color | a property whose name speaks of compositing that no longer matters |
