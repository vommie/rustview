# Backlog

Work the user decided to do later (questionnaire `2026-10-26-rustview-aufraeumen`, the cleanup findings of `video-in-slint`,
`docs/video_in_slint/CLEANUP.md`). Nothing here is planned yet; each entry says what, where, why and what the user said. When the
user asks what else could be optimized or improved, start here.

## Performance

### B1 (CLEANUP #3): every isolated redraw is rendered twice on X11

- **What:** measured in the M0 lab of `video-in-slint` with Slint 1.18 on X11: one redraw request (`window.request_redraw()` with
  nothing else going on) produces two rendered frames; the second comes from the `TimerBasedFrameThrottle` path of the winit backend.
  It affects every window, and since videos are drawn by Slint it doubles the GPU work per video frame.
- **Where:** the redraw path of our windows (`viewer/video_surface`, the render notifier of the main and fullscreen windows,
  everything that calls `request_redraw`), Slint's winit backend (`TimerBasedFrameThrottle`).
- **Why:** half the GPU and CPU work per frame while videos play.
- **User (2026-10-26):** never report anything to third parties. The user thinks it is more likely a mistake of ours than a Slint
  bug. Keep it as a possible optimization to look at: find out first whether one of our own calls (a second `request_redraw`, a
  property change during rendering, the video frame callback) causes the second frame, by counting frames per request with
  `RUST_LOG=debug` and a minimal window.

## Code size

### B2 (CLEANUP #5 and #18): two files over the ~600-line limit

- **What:** `crates/rustview/src/ui/multiview/session.rs` (about 700 lines) is over the file size limit of AGENTS.md;
  `ui/multiview/video_bar/window.rs` (694 lines in M0) went away with M2.
- **Where:** `session.rs` holds the fullscreen session: opening, populating, refreshing, bars, closing.
- **Why:** the file size rule of AGENTS.md.
- **User:** later. Split by topic like the other Multiview presenters (for example the open/populate part and the bars) when the file
  is touched next.

## Native Wayland

User decision 2026-10-26 (Wayland questionnaire and the cleanup questionnaire): "into the backlog for the future". What native
Wayland lacks in general is in `docs/video_in_slint/WAYLAND.md`.

### B3 (CLEANUP #19, W1): dragging files to other programs

- **What:** no drag source to other programs on native Wayland. winit 0.30 offers none, and a Wayland data device of our own needs the
  serial of the pointer press that started the drag, which Slint does not hand out.
- **Where:** `crates/rustview/src/system/dnd/`, `viewer/x11/xdnd.rs` (the X11 source side).
- **Why:** dragging media from RustView into a file manager or another program only works on X11 (or with
  `RUSTVIEW_WINDOW_SYSTEM=x11` through XWayland).

### B4 (CLEANUP #20, W2): dialogs bound to their window

- **What:** on native Wayland a dialog is an ordinary window: winit 0.30 has no `xdg_toplevel.set_parent` and no xdg-dialog, so KWin
  neither keeps it above its parent nor dims the parent, and a click on the locked main window closes the dialogs.
- **Where:** `crates/rustview/src/ui/dialog_parent.rs` (`raise_open` returns false on Wayland).
- **Why:** dialogs can disappear behind the main window and do not behave modally the way they do on X11.

### B5 (CLEANUP #21, W3): pointer lock of the minimap drag

- **What:** the minimap drag of a zoomed picture has no pointer lock on native Wayland; it falls back to the rectangle following the
  pointer. winit's `CursorGrabMode::Locked` plus relative pointer motion would do, but Slint does not forward device events.
- **Where:** `crates/rustview/src/ui/multiview/pointer_lock.rs`.
- **Why:** a long drag in the minimap stops at the screen edge instead of moving the picture on.
