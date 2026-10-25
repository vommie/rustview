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
