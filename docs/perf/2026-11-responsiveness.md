# Responsiveness work, November 2026 (0.6.16 to 0.6.18)

Measurements, changes and limits of the work on the responsiveness of the Browse view (thumbnail view, table, resizers,
column handling, folder switches) and, in 0.6.18, of the Multiview. Method and tools: [`README.md`](README.md). **No migration**: the library and the stored
thumbnails are untouched; `settings.json` gains `view.table_column_order` (an older file gets the default order).

## Environment

Manjaro virtual machine (KVM), 6 cores, 32 GB RAM, virtio-gpu with VirGL (OpenGL on the host's Radeon 780M), KDE Plasma on
X11 with KWin compositing, display 1600x900 at 60 Hz, RustView maximized (1600x844), scale 1, dev profile
(`opt-level = 3`). Library: 126,000 WebP pictures of about one megapixel in four folders (`Flat-1k`, `Flat-5k`, `Flat-20k`,
`Flat-100k`), thumbnails 256 px WebP q80, 400 tags (all expanded in the tag tree), default settings (4 worker threads,
automatic memory budget, 190 px thumbnails).

**What the virtual machine cannot show.** Frames come in an alternating rhythm of one and two or three display intervals
whatever the application does: a splitter drag that costs 5 ms of CPU per frame gets about 44 frames per second, the same as
a scroll that costs 15 ms (in an earlier display mode of 75 Hz, `glxgears` with vsync got 58). The display path of the guest decides the pacing, so
frames per second and frame gaps measured here are a lower bound, not RustView's rate. The value that can be compared
is **`ui`: the CPU time of the UI thread per frame** (events, callbacks, model updates, layout and the drawing calls). A
scenario can reach 60 frames per second on real hardware when its `ui` p95 stays below 16.7 ms, 144 when it stays below
6.9 ms.

**Method.** Two binaries, 0.6.15 plus the probe ("before") and 0.6.16 ("after"), run interleaved (before, after, before,
...), ten runs each per plan; every number below is the median of the ten runs. A run during which another build or
application was active on the machine was discarded and repeated; the settings of the profile were restored before every
run. Scrolling moves the list at a constant speed like a drag of the scroll bar; 3,000 px/s is a calm scroll, 12,000 px/s a
fast one, 25,000 px/s and more is what a drag of the scroll bar or a flick produces.

## Results

### Thumbnail view, folder of 100,000

| Scenario | ui p50 | ui p95 | ui max | blank tiles | process CPU |
|---|---|---|---|---|---|
| scroll 3,000 px/s | 5.8 > 5.1 ms | 8.2 > 6.9 ms | 9.7 > 8.0 ms | 0 > 0 % | 28 > 25 % |
| scroll 12,000 px/s | 9.2 > 6.5 ms | 13.3 > 9.9 ms | 15.0 > 10.6 ms | 0 > 0 % | 47 > 41 % |
| scroll 25,000 px/s | 14.6 > 8.8 ms | 16.6 > 12.5 ms | 18.2 > 14.4 ms | 15.6 > 0 % | 76 > 58 % |
| scroll 40,000 px/s | 14.3 > 10.5 ms | 17.8 > 13.3 ms | 19.3 > 14.3 ms | 47.8 > 0 % | 87 > 74 % |
| wheel, 25 notches/s | 4.9 > 4.9 ms | 7.7 > 6.8 ms | 9.5 > 7.7 ms | 0 > 0 % | 25 > 22 % |

"Blank tiles" is the share of visible tile-frames without a thumbnail: what is seen as empty tiles that fill a moment later.
Before, a row's thumbnails arrived one frame after its tiles as soon as the view moved faster than the three spare rows of
the tile pool could cover; at 40,000 px/s almost every second frame showed blank tiles.

Memory at the end of the plan (about 5,000 thumbnails decoded): 965 > 1,185 MB resident (+23 %). The decoded thumbnails are
four bytes a pixel instead of three and the rows ahead of the view are decoded too; both stay inside the thumbnail cache's
byte budget (`performance.memory_budget_mb`), which is unchanged.

### Table, folder of 100,000

| Scenario | ui p50 | ui p95 | ui max | process CPU |
|---|---|---|---|---|
| scroll 3,000 px/s | 8.5 > 7.4 ms | 12.5 > 10.1 ms | 14.1 > 11.7 ms | 35 > 31 % |
| scroll 12,000 px/s | 15.4 > 12.2 ms | 22.1 > 17.9 ms | 24.8 > 19.9 ms | 64 > 53 % |
| scroll 30,000 px/s | 49.8 > 24.8 ms | 52.2 > 27.3 ms | 53.5 > 28.7 ms | 99 > 89 % |
| wheel, 25 notches/s | 8.0 > 6.9 ms | 10.7 > 8.8 ms | 12.9 > 10.2 ms | 31 > 28 % |
| resize a column (after only) | 6.2 ms | 7.6 ms | 8.6 ms | 26 % |
| move a column (new) | 5.2 ms | 6.6 ms | 7.3 ms | 24 % |
| move an entry of the thumbnail view's sort bar (new) | 4.4 ms | 5.7 ms | 6.3 ms | 20 % |

The column resize has no "before": the probe of the old build pressed next to the handle.

### Resizers (thumbnail view, folder of 5,000, 400 expanded tags)

| Scenario | ui p50 | ui p95 | ui max | process CPU |
|---|---|---|---|---|
| left sidebar | 6.2 > 6.0 ms | 13.3 > 9.5 ms | 18.4 > 12.9 ms | 33 > 28 % |
| right sidebar (tag tree and Tag History in it) | 9.7 > 9.6 ms | 19.2 > 14.8 ms | 22.5 > 17.6 ms | 44 > 41 % |
| bar between Folders and Info | 5.2 > 4.9 ms | 6.1 > 6.1 ms | 7.1 > 7.2 ms | 26 > 22 % |
| bar between the tag tree and Tag History | 33.7 > 5.4 ms | 36.1 > 7.4 ms | 38.0 > 55.4 ms | 96 > 34 % |

Before, the tag tree was filled again for every 4 pixels the bar moved (every frame of the drag cost 34 ms). Now it is
filled when the number of rows per column changes and, while the bar moves, only when the pointer rests or after 0.4 s; one
fill of 400 tags still takes about 55 ms (the `ui max`), so a drag has at most two or three such frames a second instead of
all of them.

### List loads (time from the request)

| Load | list ready | all visible thumbnails shown |
|---|---|---|
| folder of 1,000 | 1.6 > 1.6 ms | 38 > 34 ms |
| folder of 5,000 | 4.2 > 4.3 ms | 34 > 29 ms |
| folder of 20,000 | 14.8 > 16.1 ms | 44 > 43 ms |
| folder of 100,000 | 77 > 79 ms | 110 > 105 ms |
| jump inside 100,000 (worst frame, ui) | | 18.3 > 13.6 ms |
| sort 100,000 by size | 12.0 > 10.8 ms | never > 28 ms |
| sort 100,000, worst frame (ui) | | 63.6 > 12.6 ms |

The times to the first complete picture include the guest's frame rhythm (30 to 40 ms from "list ready" to the frame).
A folder of 100,000 is loaded by the list worker; the UI thread only commits the result.

### Folder switches during a scan and thumbnail generation

Seven switches in 2.3 s over three folders of 3,000 pictures that were not indexed yet (each switch started a scan and a
generation, the next one canceled them): every list was shown at once, nothing was reported as an error, and in the 3 s
after the last switch the process used 0.5 % CPU (no worker went on). A canceled job no longer appends bytes to the
thumbnail store. **Open:** the thumbnails that were not generated before the switch are not generated when the folder is
opened again (19 of 20 visible tiles stayed without a picture until a manual refresh). Generating them without a refresh
would read files outside a refresh; that is a decision for the user, not made here.

### Multiview (fullscreen), 0.6.18

Plan `scripts/perf/plans/multiview.plan` in the fullscreen (1600x900) with the sample media of the test environment
(pictures of 1920x1080, 1080x1920, 1200x1200 and 2560x1080; videos of 1280x720, 720x1280, 720x720 and 1920x804 at
25 frames per second, playing). 0.6.17 plus the probe's Multiview steps ("before") and 0.6.18 ("after"), interleaved,
median of ten runs each. A gutter is dragged 300 px back and forth for 3 s, the pointer moves over the tiles for 3 s,
the zoom is six wheel notches in one second.

| Scenario | `ui` p50 (ms) | `ui` p95 (ms) | Input latency p95 (ms) | CPU of the process (%) |
|---|---|---|---|---|
| Gutter drag, 2 pictures | 14.73 > 0.68 | 16.66 > 0.97 | 17.0 > 6.6 | 60.0 > 4.1 |
| Gutter drag, 4 pictures, first gutter | 29.70 > 0.74 | 32.48 > 1.04 | 32.6 > 6.2 | 89.3 > 4.5 |
| Gutter drag, 4 pictures, second gutter | 29.60 > 0.76 | 31.80 > 1.10 | 31.9 > 7.6 | 88.9 > 4.4 |
| Gutter drag beside a zoomed picture | 16.21 > 1.75 | 18.76 > 2.62 | 18.8 > 10.1 | 102.2 > 9.1 |
| Pan of a zoomed picture | 15.57 > 1.79 | 17.72 > 2.55 | 17.5 > 10.1 | 63.7 > 12.2 |
| Zoom by wheel (picture) | 15.62 > 1.83 | 21.15 > 13.25 | 167 > 162 | 36.5 > 13.2 |
| Pointer over 2 pictures | 0.56 > 0.52 | 0.82 > 0.75 | 16.5 > 16.5 | 2.8 > 2.6 |
| Gutter drag, 2 videos | 0.94 > 0.94 | 2.06 > 2.14 | 8.7 > 9.0 | 18.2 > 17.2 |
| Gutter drag, 4 videos, first gutter | 1.07 > 0.98 | 1.73 > 1.94 | 9.2 > 9.0 | 30.4 > 28.4 |
| Gutter drag, 4 videos, second gutter | 1.07 > 0.94 | 2.06 > 1.62 | 9.2 > 9.0 | 20.3 > 15.2 |
| Pointer over 2 videos | 0.65 > 0.66 | 1.44 > 1.42 | 14.9 > 16.4 | 9.9 > 10.1 |

Before, every pointer movement of a gutter drag, a pan or a zoom step converted every picture of the view from RGB to
RGBA and uploaded it again (about 7 ms per picture and frame; four pictures took 30 ms, half the frames were late). The
drags with pictures now cost about one millisecond of the UI thread. A gutter drag beside a zoomed picture also started
a sharp render of the zoomed picture for every room size it passed (102 % CPU); the render now waits until the room
rests. Videos were cheap before and are unchanged within the noise: the UI thread never touched their pictures. The
wheel zoom's p95 and latency are those of the frames in which a sharp render arrives (one upload of a new picture) and
of the zoom animation, which neither version changes.

**Checked in screen recordings, not by numbers:** a gutter drag with a zoomed picture and its minimap, with an open crop
tool (the overlay follows the picture) and with a playing video looks right; no player is re-created by a drag, a zoom or
a switch of the medium (the log shows the player being reused). **Not measured:** the tab view on its own (same models
and instance code as the fullscreen; the plan drives only the fullscreen) and the GPU side of keeping a video's texture
during a drag (only the process's CPU is visible here).

## Decisions of the user (questionnaire of 2026-11-14)

- Thumbnails that a folder switch canceled are generated **only by a refresh**, as before: nothing reads files without
  one. The same rule keeps the scan of a folder without media on every visit as it is.
- The table gets RustMule's header menu (show and hide columns, Reset Columns) and its stacked sort (Shift + click, up
  to four levels); both are in 0.6.17.
- A tile whose thumbnail is loading shows no icon for half a second.
- The title "Filesize (MB)" stays, also where the sort mark shortens it.

## What was changed

| Change | Where | Effect |
|---|---|---|
| Rows ahead of a scrolling view are decoded before they have tiles (up to 24 rows, 4 per row the view moved; none after a jump) | `ui/browse_window/prefetch.rs`, `window_sync.rs` | blank tiles 15.6 % / 47.8 % > 0 % |
| View loader threads = worker threads (2 to 6) instead of half of them (2 to 4) | `thumbnails/loader.rs` | a screen of thumbnails decodes in one frame |
| Decoded thumbnails are opaque RGBA in RAM | `thumbnails/ram_cache.rs` and its users | the renderer's per-pixel RGB conversion (32 % of the UI thread at 25,000 px/s) is gone: grid ui p50 14.6 > 8.8 ms |
| Table cells with fixed geometry, rows clipped | `ui/media_table.slint`, `ui/media_table_columns.slint` | text is no longer measured for layouts (71 % of the UI thread at 30,000 px/s): table ui p50 49.8 > 24.8 ms |
| File name of a grid tile with a fixed place | `ui/media_grid.slint` | no text measuring for the tile's layout |
| Background saves of the settings are coalesced (one write 0.3 s after a burst) | `settings/registry.rs` | a resizer no longer writes `settings.json` per pointer movement (sidebar ui p95 13.3 > 9.5 ms) |
| Tag tree: refilled when the rows per column change, paced during a drag; height read on every fill | `ui/tags/` | splitter ui p50 33.7 > 5.4 ms; the half-drawn tree at startup cannot happen any more |
| Sort and filter keep the view's place | `ui/media/reload.rs` | the empty view after a sort is gone |
| Folder switch: subfolder check from memory, no save for the same folder | `ui/media/virtual_folder.rs`, `ui/folder_tree/selection.rs` | one library walk and one settings write less per switch |
| Multiview: the pictures of the tiles live in models of their own, whose rows change only when the picture does (0.6.18) | `ui/multiview/views.rs`, `ui/multiview_canvas.slint` | gutter drag with 4 pictures: ui p50 29.7 > 0.7 ms, CPU 89 % > 5 % |
| Multiview: the sharp render of a zoomed picture waits until its room rests (0.6.18) | `viewer/zoom/state.rs` | gutter drag beside a zoomed picture: CPU 102 % > 9 % |
| Multiview: a video keeps its texture during a gutter drag while it is at most 25 % too small, exact size at the end (0.6.18) | `viewer/multiview/video_placement.rs`, `instance_video_zoom.rs` | fewer texture re-creations on the render threads; not visible in the UI thread's numbers |

## Looked at and not changed

- **The remaining cost of the table is text shaping.** A text that is drawn for the first time costs 60 to 80 µs in Slint
  1.18: for every text its font stack is resolved again, and the generic families of this system are 382 font families
  (`fontique`'s own list and the system's are chained, so the list cannot be shortened through its API; a test program
  measured 3.95 µs per query whatever the list). At 30,000 px/s every frame draws about 300 new texts. This is the limit of
  what RustView can do without changing Slint.
- **One fill of the tag tree (about 55 ms for 400 expanded tags)** is mostly the measuring of every tag's text for the
  widths of the columns. Keeping the elements and updating them in place would not avoid it, so it was not built.
- **Decoding thumbnails straight into the RGBA buffer** (libwebp's `WebPDecodeRGBAInto`) would save two copies of about
  0.2 MB per thumbnail, a few percent of the 1 ms a decode takes, and needs `unsafe` and a direct dependency. Not built:
  the blank tiles are gone without it.
- **A larger tile pool** (more spare rows) would also hide late thumbnails, but every spare row is a row of tiles that is
  built and laid out. Decoding ahead needs no tiles.
- **The dock's panel drag, the tab strip and drops onto other applications** were only read, not measured.
- **The Multiview's gutter drag still rewrites every tile row per pointer movement.** With the pictures out of the rows
  that costs about a millisecond; diffing the rows would save a part of it and was not built.

## Bugs found by the measurements

- **Empty list after sorting or filtering a scrolled list.** The tile pool was rebuilt for the top of the list while the
  view stayed at its scroll position, and nothing reported the viewport again (neither the place nor the row count had
  changed). Found by the probe's blank-tile counter (36 of 36 visible tiles empty for as long as the run waited).
- **Tag tree with the rows of a default height.** The tree's height was reported by elements that are created late; a
  `changed` handler reports only changes after its element exists, so a tree that got its final height at once kept the
  450 px the code started with (18 rows per column, the rest of the panel empty) until a resizer moved. It did not
  reproduce in 22 starts here (the height always changed once after the start), so the fix is by construction: Rust reads
  the height whenever it fills the tree.
- **Dragged media released over RustView itself** (not over a folder) were offered to RustView's own window as a drop
  from another application. **Escape did not end a drag.**
- **A thumbnail job canceled by a folder switch** could append a thumbnail to the pack store without recording it.
- **Last picture of the old video in the new video's shape (0.6.18).** When a Multiview that follows the Browse
  selection switched to another video, the reused player's texture took the new medium's shape at once and mpv drew the
  old file's last picture into it, small and with bars, until the new file's first picture came; visible for as long as
  the file takes to open (reproduced with a load delayed by 2.5 s). The tile now shows nothing from the switch until the
  first picture of the new file (`MpvNotice::FirstPicture`); the player is still reused.

## First baseline (another display mode)

Before the session's display changed, a first baseline was taken at 1920x938 and 75 Hz (median of 3 runs, 0.6.15 plus the
probe, no tags). It is kept for reference; its numbers do not compare with the tables above.

| Scenario | ui p50 | ui p95 | blank tiles |
|---|---|---|---|
| thumbnail view, 12,000 px/s | 8.7 ms | 15.0 ms | 0 % |
| thumbnail view, 25,000 px/s | 16.5 ms | 21.2 ms | 8.0 % |
| thumbnail view, 40,000 px/s | 20.4 ms | 23.6 ms | 37.9 % |
| table, 12,000 px/s | 14.6 ms | 22.4 ms | |
| table, 30,000 px/s | 52.9 ms | 55.2 ms | |
