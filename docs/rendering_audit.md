# Rendering audit: native mpv windows vs. mpv's render API (2026-10-25)

Question (user, night of 2026-10-25): now that RustView no longer passes keys and the pointer to mpv, would drawing the videos
the way RustCut does (mpv's render API into an OpenGL texture that Slint shows) make RustView simpler, lighter, better on
Wayland - while keeping the handover of a running player between fullscreen, tab and tab grid free of interruptions and resets?

Method: inventory of today's code, the mpv documentation (`~/claude/docs/mpv/src/include/mpv/render.h`), Slint 1.18's winit
backend sources, and a probe (`tools/render-probe`, branch `render-api`, not part of the workspace) built from RustCut's
surface code (`rustcut-player/src/surface/{gl,mpv_render}.rs`). Environment: this VM (6 vCPUs, QXL, Mesa llvmpipe = software
OpenGL, software video decoding), X11 KDE session, mpv 0.41, Slint 1.18.1 (femtovg renderer, GLX with EGL fallback).

## How RustView draws videos today

Every video is mpv's own window (`vo=gpu`, `wid`) inside an X11 container window that RustView creates as a child of the Slint
window (`viewer/x11/subwindow.rs`). Nothing Slint draws can appear over such a window, so everything that has to be over a video
needs a workaround:

| Workaround | Why it exists | Code |
|---|---|---|
| Container + subwindow, X shape bounding cutouts for context menus and tooltips | a Slint popup over a video would be hidden behind the native window | `x11/subwindow.rs` (571 lines), `context_menu/cutouts.rs`, `viewer/manager/cutouts.rs`, `instance_host.rs` |
| Empty input shape (pointer passthrough) | the pointer over a video must reach Slint (menus, dialogs, now always) | `subwindow.rs`, `apply_pointer_passthrough` |
| Override-redirect ARGB windows for the media bar, the primary bar, the video control bars and the hint, with shaped solid fallbacks without a compositor, "host in front" checks, pointer polls for their menus | Slint cannot blend over a native window, only the compositor can blend two windows | `ui/multiview/bar_overlay*` (885), `video_bar/` (470), `hint.rs` (262), `bar_placement.rs`, `x11/overlay_server.rs` (204), three `*_window.slint` (586) |
| Bitmaps and text drawn by mpv itself (`overlay-add`, `osd-overlay`): rating/save feedback, selection corner marks, tab grid time badges | the same reason; mpv draws them into its own window | `feedback_overlay.rs` (254), `focus_overlay.rs` (148), `mpv_engine/overlay.rs`, `osd_text.rs` |
| Placeholders when a video's window is in another top-level window | a native window can only be in one place | `fullscreen_away.slint` (99), `AwayBadge` |
| Reparenting the container between the fullscreen and the main window, strict teardown order (players before windows), a non-fatal Xlib error handler | libmpv's Xlib error handler kills the process when its window goes first | `session_convert.rs`, `teardown.rs`, `x11/error_handler.rs` |
| Cursor hiding on mpv's windows, tab grid clip shapes | native windows ignore Slint's cursor and clipping | `subwindow.rs`, `grid_host.rs` |

Roughly 4 000 lines exist only because videos are native windows, and they make every new feature that touches a video more
expensive (each overlay needs its own window or an mpv bitmap). On a Wayland session RustView picks winit's Wayland backend
(no backend is forced in `main.rs`), `get_raw_window_xid` is `None`, and videos cannot be shown at all today.

## What the render API changes

mpv draws each new frame into an OpenGL texture of the window's own GL context (RustCut: inside Slint's rendering notifier, on the
UI thread); Slint shows it as an `Image` like any picture. Then:

- menus, tooltips, both action bars, the control bars, hints, corner marks, badges and placeholders are ordinary Slint elements
  over the video: every workaround of the table above goes away (the X11 module keeps only window-manager hints, the pointer lock
  and XDND);
- videos are clipped, scrolled and scaled by Slint like pictures (tab grid, zoom room, animations);
- it works on Wayland, where native child windows do not exist (mpv's own documentation recommends the render API over `wid`:
  "window embedding can cause various issues, especially with GUI toolkits").

## Measured with the probe

### Handover between windows

Slint 1.18 gives every window a GL context of its own and does not share them (`i-slint-backend-winit/renderer/femtovg/glcontext.rs`
builds each context without sharing), so a texture of the fullscreen window cannot be shown by the main window. A player has to
free its render context in the old window and create a new one in the new window. mpv then disables the video track (`vid` becomes
`no`, `render.h`: "Calling mpv_render_context_free() while a VO is using the render context is active will disable video"); setting
`vid` back right after the new context exists brings it back. Probe (`render-probe handover`, 1080p25, twice: playing A -> B,
paused B -> A):

| | playing | paused |
|---|---|---|
| time from freeing to the first video frame in the other window | 45 ms | 33 ms |
| position | kept (playback went on: 2.680 -> 2.926 s) | kept (5.640 -> 5.664 s, one frame) |
| pause state, volume, file | kept | kept |

So the player state survives (no reload, no reset), but the picture is gone for one to three frames. Showing the last frame as a
still image (one `glReadPixels` of the old texture right before it is freed) would cover that gap.

### CPU and frame rate (software OpenGL in this VM)

| Case | native mpv windows (as today) | render API in a Slint window (probe) |
|---|---|---|
| 1 x 1080p25 | 375 % CPU, 25 fps | 415 % CPU (+11 %), 25 fps, 0 drops |
| 1 x 720p60 | 309 % CPU | 353 % CPU (+14 %), ~55 fps (some drops) |
| 4 x 1080p25 (2 x 2) | 554 % CPU, ~18 fps per video (~140 of 500 frames dropped per video in 20 s) | 496 % CPU, ~9 fps per video |

(100 % = one core; both decode in software here. mpv's render time per 1080p frame with llvmpipe: ~27 ms; on a real GPU it is a
few milliseconds, which this VM cannot measure.)

The render API costs a little more for one video (Slint redraws the whole window for every video frame) and, more important,
**renders all videos one after the other on the UI thread**: with four videos the frame time is the sum of four renders, which
halves the achievable frame rate here, and a busy UI thread (a long Browse operation) would stall the videos too. Native windows
render every video on mpv's own thread, in parallel, independent of the UI.

## Assessment

| | native windows (today) | render API |
|---|---|---|
| Code size and complexity | ~4 000 lines of workarounds, every overlay is a project | overlays are plain Slint; X11 code mostly gone |
| Performance, real GPU | best (parallel, nothing redrawn but the video) | very likely fine (a few ms per video and frame), whole window redrawn per video frame |
| Performance, software GL (this VM) | better, especially with several videos | +10-15 % for one video, half the frame rate for four |
| UI and video independent | yes | no: both on the UI thread (unless a render thread with a shared context is built, see below) |
| Handover fullscreen / tab / grid | seamless (reparenting) | state kept, 1-3 frames gap (coverable with a still) |
| Wayland | does not work today | works |
| Zoom, panning, minimap for videos | mpv `video-zoom`/`video-pan` + growing the native window | the same mpv properties, the "room" is just a bigger texture area |

A render thread per window would remove the UI-thread coupling: create a second GL context that shares objects with Slint's (taken
with `glXGetCurrentContext`/`eglGetCurrentContext` inside the rendering notifier), let mpv render there into textures, synchronize
with fences. Slint does not offer this; it depends on GLX/EGL details and is the riskiest part of a migration.

## Recommendation

1. **Keep the native windows for now on X11.** They are complete, tested, and the fastest option with several videos and software
   rendering; the control bar and grid work of 0.4.7/0.4.8 just landed on them.
2. **Cheap Wayland support next:** on a Wayland session, run RustView through XWayland (select winit's X11 backend at startup).
   Everything, including the bar windows, then works as on X11. Not testable in this VM (no Wayland session).
3. **Video zoom, panning and the minimap now with mpv's properties**: that code is the same for both ways of drawing (mpv zooms in
   both); only "the window grows into the free room" would change.
4. **Render API as a staged project when complexity or native Wayland matter more:** first a spike for a render thread with a
   context shared with Slint's (the performance risk above), then one host after another (tab view, fullscreen, grid), each
   removing its workarounds, handover with a still image. The probe in `tools/render-probe` is the starting point.

## Reproduce

```
cd tools/render-probe
CARGO_TARGET_DIR=/tmp/render-probe-target cargo build --release
/tmp/render-probe-target/release/render-probe handover <video>
/tmp/render-probe-target/release/render-probe grid 22 <video> [<video> ...]   # 1-4 videos
```
Native baseline: `mpv --no-config --vo=gpu --gpu-sw=yes --hwdec=auto-safe --ao=null --loop-file=inf --geometry=WxH+X+Y <video>`,
CPU from `/proc/<pid>/stat` over 15 s after 5 s of warm-up; frame drops from mpv's `frame-drop-count`.
