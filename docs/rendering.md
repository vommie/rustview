# Drawing videos and the window system

Every video is drawn by Slint: mpv renders each frame through its OpenGL render API (`vo=libmpv`) into a texture, and the tile
shows that texture as an ordinary Slint `Image` (branch `video-in-slint`, 0.5.7-0.5.13; plan, decisions and log in
`docs/video_in_slint/HANDOFF.md`). Everything over a video (context menus, tooltips, the action bars and their drop-downs, the
video control bar with the A-B group and its popups, the hint, the feedback pulse, the corner marks, the volume bar, the minimap)
is a plain element of the window that shows the video. There are no video windows, no windows of their own for UI over a video,
no cutouts, no pointer polling and no `overlay-add`/`osd-overlay`; mpv draws nothing over the picture. mpv stays mpv otherwise
(options, the user's `mpv.conf`, scripts such as `ab_repeat.lua`, properties).

## How a video is drawn (`viewer/video_surface/`, `viewer/mpv_engine/render.rs`)

- **One render thread per video** (`VideoSurface`, owned by its video tile; `render_thread.rs`). It has a GL context of its own in
  the share group of the GL context of the Slint window that shows the video (`share_group.rs`: GLX on X11 with femtovg, EGL with
  skia and on Wayland), and mpv's render context (`MpvRenderContext`, through `libmpv2-sys`, which `libmpv2` lacks) lives on it.
  mpv's update callback wakes the thread; it renders into one of **three textures** (`target.rs`) and fences it; the window
  waits on the fence before it draws it (`exchange.rs`), so a frame never shows half drawn and the UI thread never waits for mpv.
- **The windows that show videos** (`hosts.rs`: the main window and the Multiview fullscreen window) forward their rendering
  notifier: at `RenderingSetup` the window's share group is captured and every surface attached to it gets its render context, at
  `BeforeRendering` the newest texture of each surface becomes the row of the window's `video-frames` model
  (`BorrowedOpenGLTextureBuilder`), at `RenderingTeardown` the surfaces lose their render context.
- **The texture has the size the picture is drawn at** (at most the video's own size): mpv always renders the whole, fitted picture;
  the canvas places, scales and clips it like a picture (`video_placement.rs`), so the video zoom, the minimap (the same texture in
  small) and the tab grid's scrolling need nothing from mpv.
- **Moving a video between windows** (a tab into the fullscreen and back; the fullscreen stays a separate window, decision 2 of the
  hand-off) re-creates the render context in the other window's share group and selects the video track again. Meanwhile the render
  thread reads the newest picture back before it leaves the old window and the new window shows it as a still until mpv reports a
  real frame there (`Bound::has_frame`), so no black frame shows.
- Slint must get **desktop OpenGL** (`BackendSelector::require_opengl`): on Slint's default GLES 2.0 context mpv drew
  black in the lab. The renderer is **skia** (`system/renderer.rs`; `RUSTVIEW_RENDERER=femtovg` for one start): its EGL context is
  what mpv's zero-copy VAAPI and native Wayland need; in the VM both renderers drew the UI alike.
- mpv gets the window system's display connection (`MPV_RENDER_PARAM_X11_DISPLAY` / `WL_DISPLAY`, `ui/multiview/video_host.rs`)
  for hardware decoding interop; `hwdec=auto-safe` unless the user's `mpv.conf` says otherwise.
- **Teardown:** the render context is freed on its render thread, then the player shuts down (`mpv_terminate_destroy`), then the
  textures go. libmpv may still install its process-killing Xlib error handler when a hardware decoder's interop used Xlib, so
  `install_nonfatal_error_handler` stays after a teardown (`viewer/x11/error_handler.rs`; the VM has no VAAPI to prove otherwise).
- Settings > Debug shows the GL context of the windows (`HostGlFacts`), and its video self-test renders mpv's test picture
  through the same render API into a texture of its own (`mpv_engine/self_test.rs`).

## Decisions

- **2026-10-25 (night questionnaire, `~/claude/feedback/2026-10-25-rustview-night`):** keep the native mpv windows, start through
  XWayland in a Wayland session. The render-API prototype of that time (all videos drawn one after another on the UI thread) was
  about half as smooth with four videos; it stays untouched on the branch `render-api` (`docs/rendering_audit.md` there), neither
  merged nor deleted.
- **2026-10-26 (`mpv-slint-lab`, questionnaire `2026-10-26-rustview-video-in-slint`):** the render API with **one render thread per
  video** (lab approach 03) removes the reason for both: Slint's own menus, popups and tooltips appear over a video, nothing floats
  over other applications, and native Wayland works. Built in the milestones M1-M7 of `docs/video_in_slint/HANDOFF.md`.
  Decisions there: renderer skia; the fullscreen stays a separate window; **native Wayland is the default in a Wayland session**
  (switched in 0.5.15 after the test in the user's Plasma Wayland session, questionnaire `2026-10-26-rustview-wayland-test`; the three
  gaps go to the cleanup branch); a cleanup branch after the acceptance (`docs/video_in_slint/CLEANUP.md`).

## How the window system is chosen (0.4.14, default changed in 0.5.15)

`system/window_system/` decides once, at the start of `main` and before any window exists (`window_system::decide`, logged as
`[Startup: WindowSystem]`, stage `WindowSystem`); the pure part is `choice.rs` (`choose`, unit tested):

| Session | Result |
|---|---|
| no Wayland display, `DISPLAY` set (an X11 session) | X11 (`X11Session`; `XWayland` when `XDG_SESSION_TYPE=wayland`), whatever the variable says |
| Wayland display | native Wayland (`Session`) |
| Wayland display, `RUSTVIEW_WINDOW_SYSTEM=wayland` | native Wayland (`Requested`) |
| Wayland display, `RUSTVIEW_WINDOW_SYSTEM=x11`, the X server of `DISPLAY` answers | X11 through XWayland (`XWayland`) |
| Wayland display, `RUSTVIEW_WINDOW_SYSTEM=x11`, no answer or no `DISPLAY` | native Wayland (`XDisplayUnreachable`, `NoXDisplay`) |
| neither display | nothing is forced; winit reports the error as before |

Until 0.5.14 a Wayland session started through XWayland (and the X display was asked at every start). What native Wayland lacks
(dialog parents, dragging files out, the pointer lock of the minimap) is listed in `docs/video_in_slint/WAYLAND.md`; Settings > Debug
names it.

- The backend is forced through Slint's API, not the environment: `BackendSelector::with_winit_event_loop_builder` with winit's
  `EventLoopBuilderExtX11::with_x11` (or `with_wayland`), in `window_system::select_backend`. Programs RustView starts (file
  manager, "Open With") still see the session as it is. winit takes Wayland whenever `WAYLAND_DISPLAY` is set and cannot try
  Wayland after a failed X11 connection, so when XWayland is asked for, the X display is asked first with one short x11rb connection
  (`viewer::x11_display_answers`).
- Everything that depends on the window system asks `window_system::current()` instead of reading the environment: the image
  clipboard tool (`system/clipboard.rs`: xclip on X11, under XWayland with wl-copy as the fallback, wl-copy on Wayland), the drag
  and drop telemetry (`SystemDndHandler::detect_session_type`), the startup diagnostics (`window_system` next to the session type)
  and the Multiview fullscreen (X11: `_NET_WM_STATE` on the chosen monitor; Wayland: winit's borderless fullscreen). The X11
  features degrade on native Wayland (`get_raw_window_xid` is `None` there).
- Checked in 0.4.14 in the VM (an X11 session) inside a headless nested Wayland session (`dbus-run-session -- kwin_wayland --virtual
  --xwayland`, scratch XDG dirs, the Slint MCP build to click, since XTEST input does not reach Xwayland there): the XWayland
  choice and the three native Wayland cases (requested, no `DISPLAY`, a `DISPLAY` that does not answer: first frame, clean
  shutdown). The checks of native Wayland with videos (M7) are in `docs/video_in_slint/WAYLAND.md`.
