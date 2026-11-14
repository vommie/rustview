# Responsiveness work, November 2026 (0.6.16)

Measurements, changes and limits of the work on the responsiveness of the Browse view (thumbnail view, table, resizers,
column handling, folder switches). Method and tools: [`README.md`](README.md). **No migration**: the library and the stored
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
frames per second and frame gaps measured here are a lower bound, not RustView's rate. The value that holds on any machine
is **`ui`: the CPU time of the UI thread per frame** (events, callbacks, model updates, layout and the drawing calls). A
scenario can reach 60 frames per second on real hardware when its `ui` p95 stays below 16.7 ms, 144 when it stays below
6.9 ms. To measure the real pacing on a real machine:

```bash
scripts/perf/run-probe.sh --home /path/to/test-profile scripts/perf/plans/grid.plan
```

```bash
scripts/perf/analyze.py scripts/perf/plans/grid.log
```

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
- **Multiview gutters, the dock's panel drag, the tab strip and drops onto other applications** were only read, not
  measured; the gutter drag rewrites every tile row per pointer movement and is a candidate for a later round.

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
