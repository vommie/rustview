# Measuring the responsiveness of the UI

RustView has a built-in probe for developers. It plays a plan of scripted steps in the main window (scrolling, jumps, folder
switches, sorting, dragging resizers) and logs every frame, so the same scenario can be measured before and after a change, on
any machine. Results of past work: [`RESULTS.md`](RESULTS.md) (Browse performance, 0.3.46) and
[`2026-11-responsiveness.md`](2026-11-responsiveness.md).

The probe is part of every build and costs one relaxed atomic load per frame while it is not used. It never runs unless the
environment variable `RUSTVIEW_PERF_PLAN` names a plan file.

## One run

```bash
scripts/perf/run-probe.sh --home /path/to/test-profile scripts/perf/plans/grid.plan
```

```bash
scripts/perf/analyze.py scripts/perf/plans/grid.log
```

`run-probe.sh` starts the binary (`target/debug/rustview`, or `--bin PATH`), lets the plan run and ends the application again
(the plan's `quit`, or the time limit). **`--home DIR` uses an isolated profile** (`DIR/data`, `DIR/config`, `DIR/state`);
without it the run works on your own library and settings, which a resize or sort step changes. `analyze.py` prints one row
per scenario; `--median` over several logs gives the median of every value, `--json` the numbers, `--compare A.json B.json`
two variants side by side.

## What is measured

Per frame of the main window (`src/ui/perf_probe/recorder.rs`):

| Value | Meaning |
|---|---|
| gap | Time from one frame to the next. 16.7 ms is 60 frames per second; a gap above 25 ms is a dropped frame ("late"). |
| ui | CPU time of the UI thread per frame: events, callbacks, model updates, layout and the drawing calls. This is what the application itself costs, whatever the display or a software rasterizer adds. |
| draw | The part of it inside Slint's drawing. |
| lat | From a pointer event to the end of the frame that shows its effect. |
| blank | Visible media tiles of the thumbnail view that had no thumbnail in that frame (what is seen as empty tiles while scrolling). |
| cpu, rss | CPU time of the whole process and its resident memory at the scenario's marks. |

A list load (folder switch, sort, jump) is measured from the request to the list being ready, to the first frame, to the
first thumbnail and to all visible thumbnails.

**Compare only runs of one machine in one session**, interleaved (A B A B ...), and take the median of at least ten runs.
In a virtual machine the display may be throttled (a guest window that is not shown gets about one frame per second): gaps
are then meaningless, the `ui` column still holds.

## Plans

One step per line, `#` starts a comment (`src/ui/perf_probe/plan.rs`):

| Step | What it does |
|---|---|
| `ready` | Waits until the start is over and nothing works in the background. |
| `settle MS` | Waits. |
| `begin NAME` / `end` | Marks a scenario; the frames between the marks belong to it. |
| `tree` | Refresh Tree (Full), waits until it is done. |
| `folder NAME [index\|nowait]` | Opens the folder with that name and waits until its list and visible thumbnails are shown; `index` waits for its scan and thumbnail generation, `nowait` goes on at once. |
| `view grid\|table` | Switches the Browse view. |
| `sort COLUMN` | Clicks a column header (0 = name), waits for the list. |
| `scroll PX_PER_S SECONDS` | Moves the list like a drag of the scroll bar (negative: up). |
| `wheel NOTCHES_PER_S SECONDS` | Turns the mouse wheel over the list. |
| `jump FRACTION` | Jumps to a place of the list (0 to 1) and waits for its thumbnails. |
| `press @ELEMENT`, `sweep DX DY SECONDS CYCLES`, `moveto @ELEMENT SECONDS`, `drag DX DY SECONDS`, `release` | Pointer input: press on an element, move away and back, move to an element, move by a distance and stay, release. |
| `key NAME` | escape, return, delete, up, down, pageup, pagedown. |
| `shot NAME` | Saves a picture of the window next to the log (`LOG.NAME.png`). |
| `quit` | Closes the application. |

Elements (`probe-anchor` in `ui/main.slint`): `@left-resizer`, `@right-resizer`, `@left-splitter:N` and `@right-splitter:N`
(the bar below panel group N of a sidebar), `@column:N` (the resize handle of table column N), `@header:N` (the header of table column N), `@item:N` (the tile or
row at list position N), `@view`, `@folder-tree`.

The pointer events are the probe's own: they enter the window like real ones, and the real pointer is not moved. **A media
drag is never released over anything but a folder**: such a release offers the files to the window under the real pointer.

The plans in `scripts/perf/plans/` are short (about 20 seconds each, so a run fits between other work on the machine):
`grid.plan` (thumbnail view: scrolling, wheel), `table.plan` (table: scrolling, wheel, column resize), `loads.plan` (folder
switches, jumps, sorting) and `resize.plan` (sidebars and splitters). They expect folders named `Flat-1k`, `Flat-5k`,
`Flat-20k` and `Flat-100k` (1,000 to 100,000 pictures each) below the root folder of the profile; `index-stress.plan`
indexes them once.
