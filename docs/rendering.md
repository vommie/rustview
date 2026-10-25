# Drawing videos and the window system

Every video is a native mpv window (libmpv, `vo=gpu`, embedded with `wid`) inside a RustView window. Everything that has to
appear over a video works around that window: `viewer/x11/subwindow.rs` cuts holes into it, the action bars, the video control
bar and the hint are override-redirect windows of their own (`bar_overlay`, `video_bar`, `hint`), and mpv draws the rating icon,
the selection marks and the grid time badges itself (`overlay-add`, `osd-overlay`). Such child windows exist only on X11.

## Decisions (night questionnaire, 2026-10-25, `~/claude/feedback/2026-10-25-rustview-night`)

- **Drawing path: keep the native mpv windows for now.** The render-API prototype (mpv draws every frame into a texture of the
  Slint window and everything over it is ordinary UI, as in RustCut) stays untouched on the branch `render-api`; the measurements
  and the comparison are in `docs/rendering_audit.md` there. That branch is neither merged nor deleted. Reason: with several videos
  the native windows are about twice as smooth (the prototype draws all videos one after another on the UI thread; four videos
  reached half the frame rate in the VM), and everything works today (control bar, tab grid, video zoom). The video zoom fits both
  paths, so nothing is lost if the path changes later; such a move would come in stages (a render thread of its own first, then
  the tab view, the fullscreen and the grid, each stage removing its workarounds).
- **Wayland: start through XWayland.** In a Wayland session RustView runs as an X11 program through XWayland (KDE Plasma ships
  it), so videos and everything else work as on X11. Native Wayland, without the X11 workarounds, can come later with the render
  API. Until this decision RustView showed no video at all under Wayland.

## How the window system is chosen (0.4.14)

`system/window_system/` decides once, at the start of `main` and before any window exists (`window_system::decide`, logged as
`[Startup: WindowSystem]`, stage `WindowSystem`); the pure part is `choice.rs` (`choose`, unit tested):

| Session | Result |
|---|---|
| no Wayland display, `DISPLAY` set | X11 (`X11Session`; `XWayland` when `XDG_SESSION_TYPE=wayland`) |
| Wayland display and `DISPLAY`, the X server answers | X11 through XWayland (`XWayland`) |
| Wayland display and `DISPLAY`, no answer | native Wayland (`XDisplayUnreachable`) |
| Wayland display, no `DISPLAY` | native Wayland (`NoXDisplay`), with a warning |
| `RUSTVIEW_WINDOW_SYSTEM=wayland` and a Wayland display | native Wayland (`Requested`) |
| neither display | nothing is forced; winit reports the error as before |

- The backend is forced through Slint's API, not the environment: `BackendSelector::with_winit_event_loop_builder` with winit's
  `EventLoopBuilderExtX11::with_x11` (or `with_wayland`), in `window_system::select_backend`. Programs RustView starts (file
  manager, "Open With") still see the session as it is. winit takes Wayland whenever `WAYLAND_DISPLAY` is set and cannot try
  Wayland after a failed X11 connection, so in a Wayland session the X display is asked first with one short x11rb connection
  (`viewer::x11_display_answers`).
- Everything that depends on the window system asks `window_system::current()` instead of reading the environment: the image
  clipboard tool (`system/clipboard.rs`: xclip on X11, under XWayland with wl-copy as the fallback, wl-copy on Wayland), the drag
  and drop telemetry (`SystemDndHandler::detect_session_type`), the startup diagnostics (`window_system` next to the session type)
  and mpv. The X11 fallbacks for native Wayland stay as they are (`get_raw_window_xid` is `None` there).
- mpv: its Wayland GPU contexts ignore `wid` and open a window of their own, and mpv's automatic probe tries them first whenever the
  session has a Wayland display. `MpvPlayer::attach_to_window` therefore limits `gpu-context` to the X11 ones
  (`mpv_engine/gpu_context.rs`: mpv's probe becomes `x11vk,x11egl,x11`, the same order mpv probes them in; X11 entries of the user's
  `mpv.conf` are kept, Wayland entries dropped). On an X11 session mpv ends up with the same context as before (in the VM:
  `x11egl`).
- Checked in the VM (an X11 session) inside a headless nested Wayland session (`dbus-run-session -- kwin_wayland --virtual
  --xwayland`, scratch XDG dirs, the Slint MCP build to click, since XTEST input does not reach Xwayland there): the XWayland
  choice, the X11 main window, a video tab whose mpv window is a child of RustView's X11 window drawing with `x11egl` (mpv
  reports its frames shown; an X capture of the GL window stays black, so the picture itself was not seen), and the three native
  Wayland cases (requested, no `DISPLAY`, a `DISPLAY` that does not answer: first frame, clean shutdown). Not tested: a real
  Plasma Wayland session on a GPU, KWin's hand-over of the X11 clipboard and of XDND drops to Wayland programs, the pointer lock of
  the minimap drag (Xwayland emulates the pointer warp only while the cursor is hidden) and fractional scaling under XWayland.
