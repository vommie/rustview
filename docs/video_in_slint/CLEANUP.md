# Cleanup findings (for the branch after the acceptance)

User decision 2026-10-26 (M0 questionnaire, question 4): a general cleanup and optimization pass of RustView comes as a
branch of its own **after** the acceptance of `video-in-slint`. Every milestone logs here what it notices outside the video
work (one line each: where, what, why it matters, a rough size). Nothing here is done on this branch. At the end the list was
presented to the user as a questionnaire; the outcome per row is at the end of this file.

| # | Found in | Where | What | Why it matters |
|---|---|---|---|---|
| 1 | M0 | `src/viewer/mpv_engine/events.rs` `spawn_event_observer_thread` | `time-pos`, `duration`, `pause`, `volume`, `eof-reached`, `idle-active`, `fullscreen`, `window-maximized` are read with `get_property` every 20 ms (100 ms hidden) per player instead of `observe_property` events | CPU and wake-ups per video while nothing changes |
| 2 | M0 | `src/ui/multiview/session.rs`, `tab_host.rs` (`MULTIVIEW_TICK_MS` = 50) | a 50 ms tick runs while a Multiview is shown (aspect probing, zoom polling, mark reveal); after M3 the pointer part is gone, the rest could be event-driven. Measured in M7: a tab with one paused video costs 5 % of a core on the UI thread, the Browse view 0.6 % | idle CPU, the "no polling" rule of CLAUDE.md |
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
| 14 | M6 | VM, `target/debug/incremental` of the worktree `~/claude/rustview-video` | the incremental cache grew to 94 GB in one day of milestones and filled the disk to 0 bytes (user noticed); deleting it is safe. `incremental = false` for the dev profile (it is `opt-level = 3` anyway) or `CARGO_INCREMENTAL=0` in `.cargo/config.toml` would stop it | a full disk stops every session on the machine |
| 15 | M6 | `src/ui/tabs/grid_bridge.rs` `sync_tab_grid_ui`, `handle_item_*` | `_library`/`settings` parameters partly unused, a mix of `clone_strong` captures in `tab_grid_callbacks.rs` (each callback keeps a strong handle of the main window), many "Requirement N" comments from an old spec | readability, a reference cycle of the window with its own callbacks |
| 16 | M7 | `src/viewer/multiview/instance_tiles.rs` `start_video`, `mpv_engine/events.rs` | a video mpv cannot open (`Corrupt/truncated.mp4`) shows an empty black tile: the error tile appears only when the player cannot be created, while `loadfile` fails later and asynchronously (`end-file` with an error is not watched). The same before `video-in-slint` | ACCEPTANCE P3 expects an error tile; a black tile looks like a hang |
| 17 | M7 | `src/ui/tabs/grid_bridge.rs` (the fullscreen launch shared by F11 in the tab view and the grid) | F11 in the tab view logs `[TabGrid: MiddleClick] Launching fullscreen ... from TabGrid (Requirement 3: auto-plays)` | a misleading log line when reading a user's log |
| 18 | M7 | `src/ui/multiview/session.rs` (702 lines) | still over the ~600-line limit (CLEANUP #5); M7 moved the fullscreen state into `fullscreen_mode.rs` but the file grew by a few lines | file size rule |
| 19 | M7 (W1) | native Wayland, `system/dnd/`, `viewer/x11/xdnd.rs` | no drag source to other programs on native Wayland (winit offers none; a Wayland data device of our own needs the serial of the pointer press) | user decision 2026-10-26: solve later, in the cleanup branch |
| 20 | M7 (W2) | native Wayland, `ui/dialog_parent.rs` | dialogs are ordinary windows (no `xdg_toplevel.set_parent`, no xdg-dialog in winit 0.30); a click on the locked main window closes them | user decision 2026-10-26: solve later |
| 21 | M7 (W3) | native Wayland, `ui/multiview/pointer_lock.rs` | the minimap drag has no pointer lock (winit's `CursorGrabMode::Locked` + relative motion would do, Slint does not forward device events) | user decision 2026-10-26: solve later |

## Outcome (questionnaire `2026-10-26-rustview-aufraeumen`, done on `claude` from 0.6.1)

| # | Decision | Outcome |
|---|---|---|
| 1 | do | Done. The observer thread observes `time-pos`, `duration`, `pause`, `volume`, `mute`, `eof-reached`, `idle-active` and `video-params/{dw,dh,rotate}` (`mpv_observe_property`) and sleeps in `mpv_wait_event(-1)`; nothing is polled any more (`viewer/mpv_engine/events.rs`). `fullscreen`/`window-maximized` were already gone. The video size is cached by the player and invalidated by `loadfile` until the new file starts, which replaced the tick-counting `StaleProbe` of recycled players. |
| 2 | do | Done. The 50 ms repeating timers of the fullscreen, the tab view and the tab grid are single-shot `WakeTimer`s armed for `MultiviewInstance::tick_deadline` (`instance_tick.rs`); pointer, key and player events (`PlayerEvent::{Playback, VideoParams}`), `modal-lock-changed` and `tab-grid-active-changed` of the main window update the hosts at once. Measured: a tab with one paused video 3 ticks of 100 Hz in 10 s = 0.3 % of a core (M7: 5 %). It still ticks every 50 ms while the bar of a playing video shows (its time moves) and, in the tab grid, while a video plays (its time badge). |
| 3 | later | `docs/BACKLOG.md` B1 (no upstream report; first look for a cause of our own). |
| 4 | do | Done. One listener per player (`MpvPlayer::on_notice`, `MpvNotice`) instead of the callback lists (the general `subscribe` list had no subscriber at all); the stale OSC/wheel docs are rewritten. |
| 5 | later | `docs/BACKLOG.md` B2 (`video_bar/window.rs` went in M2; `session.rs` stays over the limit for now). |
| 6 | do | Done. The options moved to `viewer/mpv_engine/options.rs`: `load-osd-console` (no mpv option) is gone, `script-opts` is no longer set twice nor overwritten (the A-B script's message switch goes through `change-list`), the OSC deadzone options are gone. The mpv defaults check of the same questionnaire is in its own questionnaire `2026-10-26-rustview-mpv-einstellungen`. |
| 7 | do | Done (`set_interactive` removed). |
| 8 | do | The second placement went with M6; `sync_tab_grid_ui` places each Multiview once and takes the instance lock once per tab instead of three times. |
| 9 | do | Already in CLAUDE.md's VM notes ("When the viewer of the VM ... ~1 frame/s"); nothing more to do. |
| 10 | do | Done: `crates/rustview/tests/tooltip_policy.rs` (every `TooltipHost` has an id and its file calls `on-hover-enter` and `on-hover-leave` of it). |
| 11 | do | Done in the test environment (outside the repository): `gen_ab_repeats.py --help` prints its usage and changes nothing; an unknown argument does the same with exit code 2. |
| 12 | do | Done. While the crop rectangle is the whole picture, a drag inside it draws a new rectangle (`CropTool::hit_test`, the overlay shows the crosshair there). |
| 13 | do | Done: `MultiviewBarMenu.translucent` became the color property `fill`. |
| 14 | do | Done: `incremental = false` in `[profile.dev]` (a `CARGO_INCREMENTAL` in `.cargo/config.toml` would not reach cargo itself). |
| 15 | do | Done: the unused library parameter of `sync_tab_grid_ui` is gone (all call sites), every tab grid callback holds the main window weakly, the "Requirement N" comments are rewritten. |
| 16 | do | Done. An `end-file` with an error (and an unexpected `shutdown` of mpv) reaches the tile as `PlayerEvent::Failed`; the tile shows the warning icon with "This video could not be played" and mpv's reason (its last error line for the file plus the error code's text) where the video would be; F5 tries again. Checked with `Corrupt/truncated.mp4`. A crash inside libmpv itself ends the whole process and cannot be caught. |
| 17 | do | Done: the shared handler is `BridgeTabGrid::open_tab_fullscreen` behind the main window callback `open-tab-fullscreen` (grid middle click, F11, the bar's button) and logs `[TabFullscreen: Open]`. |
| 18 | later | Same as #5 (`docs/BACKLOG.md` B2). |
| 19 | later | `docs/BACKLOG.md` B3. |
| 20 | later | `docs/BACKLOG.md` B4. |
| 21 | later | `docs/BACKLOG.md` B5. |

### The mpv check of the same questionnaire (wish), decided in `2026-10-26-rustview-mpv-einstellungen` and `2026-10-26-rustview-ab-meldungen`

Found: mpv never got keys or the pointer (no window of its own; `mpv_command` shows no OSD), the OSC was forced off, but mpv
could still draw its OSD into the video texture (script messages, and whatever a `mpv.conf` turns on, e.g. `osd-level=3`), RustView
switched mpv's built-in bindings and the user's `input.conf` on, and six key-driven built-in scripts plus the youtube-dl hook ran in
every player. Decided and done (0.6.2, `viewer/mpv_engine/options.rs`): no OSD at all (`osd-level=0`, also over `mpv.conf`, and
`ab_repeat-osd_messages=no`; the switch "Show the script's messages" is gone, the control bar shows what happens), no input of mpv's
own (no built-in or `input.conf` bindings, no keyboard, pointer, terminal or media keys), the user's `mpv.conf` stays loaded with
its cache and everything else; an `input-ipc-server` in it stays (the test environment uses it).
