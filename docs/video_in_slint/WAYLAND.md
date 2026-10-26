# Native Wayland as the default (decision 3)

Decision 3 of `HANDOFF.md` (2026-10-26): native Wayland becomes RustView's default in a Wayland session, **after a joint test
with the user** in a Plasma Wayland session; XWayland stays selectable. Until then a Wayland session starts through XWayland
(`system/window_system/choice.rs`). This file lists what native Wayland lacks compared with X11/XWayland, what M7 checked
already, and the plan of the joint test. The German checklist for the user is the questionnaire
`2026-10-26-rustview-wayland-test` (`~/claude/feedback/questionnaires/rustview/`).

## What works the same on native Wayland

Since `video-in-slint` everything over a video is drawn by Slint, so this needs no X11: videos in the tab view, the
fullscreen and the tab grid (mpv's render API on an EGL context in Slint's share group; mpv gets the `wl_display` for hardware
decoding), every bar, menu, popup, tooltip, hint, feedback icon, the corner marks, the video zoom and minimap, the handover
between the main window and the fullscreen with a still. Hiding the resting cursor goes through winit. The main window
maximizes natively at startup. Native Wayland also brings sharp fractional scaling (XWayland scales X11 windows blurrily).

## What native Wayland lacks (to be solved or accepted by the user)

| # | Feature | X11 / XWayland | Native Wayland today | Possible way |
|---|---|---|---|---|
| W1 | **Dragging files to other programs** (Dolphin, a browser, a mail) | XDND source (`viewer/x11/xdnd.rs`) with a ghost window that follows the pointer (`ghost.rs`) | not possible: the drag stays inside RustView; drops INTO RustView and drags inside it work | a Wayland drag source (`wl_data_device.start_drag` with `text/uri-list`); winit has none, it needs its own Wayland connection to the same surface (`smithay-client-toolkit`), a larger piece of work |
| W2 | **Dialogs tied to their window** (F2 rename, Delete, Save As, the settings window) | `WM_TRANSIENT_FOR` + modal: KWin centers the dialog over its parent, dims the parent, keeps it above only its parent, a click on the parent brings the dialog back | ordinary windows: KWin places them where it likes, the parent is not dimmed; the parent stays locked (RustView's own lock), a click on the locked main window closes the open dialogs instead of raising them | `xdg_toplevel.set_parent` (+ `xdg-dialog-v1` for modal) once winit offers it (not in winit 0.30), or through a Wayland connection of our own as in W1 |
| W3 | **Pointer lock of the minimap drag** (the pointer is hidden and held while the picture moves finely) | XFixes hide + `XWarpPointer` (`pointer_lock.rs`) | falls back: the drag moves the picture as far as the pointer can go inside the window (no endless drag) | winit's `CursorGrabMode::Locked` + relative motion (`DeviceEvent::MouseMotion`) works on Wayland (`zwp_pointer_constraints`/`relative_pointer`); needs the device events, which Slint's winit backend does not forward today |
| W4 | **Fullscreen on a chosen monitor** (Settings > Multiview: monitor) | `_NET_WM_FULLSCREEN_MONITORS` on the monitor's rectangle | winit's borderless fullscreen on the output at the monitor's position (M7); KWin may still pick the output the window opened on | check in the joint test with two monitors, if there are two |
| W5 | **Window positions** ("Reset window" centers the main window; the fullscreen window is moved to its monitor before it shows) | set by RustView | a Wayland client cannot place its windows; KWin decides | accept (KWin's rules can place windows) |
| W6 | **Image to the clipboard** (copy a picture) | `xclip` (under XWayland `wl-copy` as the fallback) | `wl-copy` only | install `wl-clipboard` (`sudo pacman -S wl-clipboard`); Settings > Debug says when it is missing |
| W7 | **Settings > Debug** facts of the X server (compositor, X extensions) | shown | not shown (no X server) | accept |

## What M7 checked in the VM (an X11 session)

In a headless nested KWin Wayland session (`dbus-run-session -- kwin_wayland --virtual`, scratch XDG dirs, the Slint MCP build
to drive the app, since no X11 input reaches native Wayland windows): see the log below. A real Plasma Wayland session on the
VM's display (VirGL), mpv's hardware decoding on Wayland and two monitors are left for the joint test.

## The joint test (with the user, in a Plasma Wayland session)

1. The user logs out of the X11 session and into "Plasma (Wayland)" on the VM's login screen (or on the AMD PC).
2. Native Wayland: start RustView with `RUSTVIEW_WINDOW_SYSTEM=wayland`; Settings > Debug shows "Native Wayland". Go through
   the checklist (videos in tab, fullscreen and grid; bars and menus over videos; dialogs; the minimap drag; copying a
   picture; dragging a file to Dolphin; startup maximized; fractional scaling if the display uses it).
3. XWayland for comparison: start RustView without the variable (today's default) and repeat a few items.
4. The user decides per item of W1-W7 whether it must be solved before the switch or is accepted.
5. Then the default changes: in `choice.rs` a Wayland session takes native Wayland, `RUSTVIEW_WINDOW_SYSTEM=x11` (new) asks for
   XWayland, the tests, `docs/rendering.md`, CLAUDE.md "Platform", README and Settings > Debug follow.

## Log

- 2026-10-26 (M7): list made. Found and fixed on the way: the Multiview fullscreen never opened on native Wayland (it waited
  for an X11 window id and closed itself after 40 tries); it now enters winit's borderless fullscreen (`ui/multiview/fullscreen_mode.rs`).
- 2026-10-26 (M7, in the user's Plasma Wayland session in the VM; Claude drove the MCP build, spectacle took the shots; no pointer injection
  tool exists there, so nothing that needs a hovering mouse or a drag was run). Native Wayland (`RUSTVIEW_WINDOW_SYSTEM=wayland`, EGL on
  virgl): starts maximized; a video tab; the control bar (shown by the jump key); the tab's context menu over the playing video; the
  fullscreen and back; four videos in the fullscreen; zoom with the live minimap; the tab grid with the time badge; the rename dialog (an
  ordinary window, W2); Settings > Debug shows "Native Wayland" with the degraded features. Found: after the first open, every second
  fullscreen was not fullscreen (the panel stayed above it; KWin dropped the request once the reused window had been hidden). Fixed: the
  state is checked 200 ms after the request on Wayland too (winit's last configure) and asked for again; 6 of 6 opens fullscreen
  afterwards (5 needed the second request). XWayland (no variable): tab, context menu over the video, 4 of 4 fullscreen opens (one
  second request). Not run by Claude: copying a picture (the media context menu cannot be driven through the MCP), dragging in and out,
  the minimap's pointer lock, fractional scaling.
- 2026-10-26 (0.5.15): questionnaire saved: switch after the test (done), W1 dragging out, W2 dialog parents and W3 the pointer lock
  "later" (cleanup branch, CLEANUP #19-#21); W4-W7 accepted as listed. The user ran no tests of their own ("passt", Claude's checks in the
  VM's Wayland session count). Default switched: a Wayland session runs on native Wayland, `RUSTVIEW_WINDOW_SYSTEM=x11` (or `xwayland`)
  asks for XWayland, an X11 session stays on X11 whatever the variable says. Settings > Debug: "Native Wayland" is Info (was Warning).
