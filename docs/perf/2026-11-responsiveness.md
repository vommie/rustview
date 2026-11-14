# Responsiveness work, November 2026 (0.6.16)

Running protocol of the measurements and changes. Method and tools: [`README.md`](README.md). No database migration.

## Environment

Manjaro virtual machine (KVM), 6 cores, 32 GB RAM, virtio-gpu with VirGL (OpenGL on the host's Radeon 780M), KDE Plasma on
X11 with KWin compositing, display 1920x994 at 75 Hz, RustView maximized (1920x938), scale 1, dev profile (`opt-level = 3`).
Library: 126,000 WebP pictures of about one megapixel in four folders (`Flat-1k`, `Flat-5k`, `Flat-20k`, `Flat-100k`),
thumbnails 256 px WebP q80, default settings (4 worker threads, automatic memory budget, 190 px thumbnails).

**What the virtual machine cannot show.** Frames come in an alternating 13 ms / 38 ms rhythm whatever the application does
(a splitter drag that costs 4 ms of CPU per frame gets 47 frames per second; `glxgears` with vsync gets 58 of 75). The
display path of the guest decides the pacing, so frames per second and frame gaps measured here are a lower bound, not
RustView's rate. The value that holds on any machine is **`ui`: the CPU time of the UI thread per frame** (events,
callbacks, model updates, layout and the drawing calls). A scenario can reach 60 frames per second on real hardware when
its `ui` p95 stays below 16.7 ms, 144 when it stays below 6.9 ms. `scripts/perf/run-probe.sh` measures the real pacing on
a real machine.

Runs during which another build or application was active on the machine were discarded and repeated.

## Baseline (0.6.15 plus the probe), median of 3 runs

### Thumbnail view, folder of 100,000

| Scenario | ui p50 | ui p95 | ui max | input latency p95 | blank tiles | process CPU |
|---|---|---|---|---|---|---|
| scroll 3,000 px/s | 4.7 ms | 8.0 ms | 9.9 ms | 10.9 ms | 0 % | 26 % |
| scroll 12,000 px/s | 8.7 ms | 15.0 ms | 18.1 ms | 15.2 ms | 0 % | 54 % |
| scroll 25,000 px/s | 16.5 ms | 21.2 ms | 23.7 ms | 21.2 ms | 8.0 % (up to 24 of 36) | 94 % |
| scroll 40,000 px/s | 20.4 ms | 23.6 ms | 26.6 ms | 23.9 ms | 37.9 % (up to 30 of 36) | 123 % |
| wheel, 25 notches/s | 3.8 ms | 6.8 ms | 8.7 ms | 19.6 ms | 0 % | 21 % |

"Blank tiles" is the share of visible tile-frames without a thumbnail. At 40,000 px/s almost every second frame shows
blank tiles: a row's thumbnails arrive one frame after its tiles.

### Table, folder of 100,000

| Scenario | ui p50 | ui p95 | ui max | input latency p95 | process CPU |
|---|---|---|---|---|---|
| scroll 3,000 px/s | 7.3 ms | 12.2 ms | 15.0 ms | 12.7 ms | 33 % |
| scroll 12,000 px/s | 14.6 ms | 22.4 ms | 24.8 ms | 22.8 ms | 62 % |
| scroll 30,000 px/s | 52.9 ms | 55.2 ms | 55.6 ms | 55.3 ms | 99 % |
| wheel, 25 notches/s | 7.1 ms | 10.2 ms | 11.5 ms | 21.7 ms | 29 % |

At 30,000 px/s every frame refills all rows of the table: about 1 ms per row, almost all of it inside the drawing.

### List loads (time from the request)

| Load | list ready | first frame | all visible thumbnails |
|---|---|---|---|
| folder of 1,000 | 1.4 ms | 45 ms | 46 ms |
| folder of 5,000 | 4.3 ms | 36 ms | 37 ms |
| folder of 20,000 | 14.0 ms | 32 ms | 56 ms |
| folder of 100,000 | 78 ms | 112 ms | 112 ms |
| jump inside 100,000 (4 places) | | 35 ms | 35 ms |
| sort 100,000 by size | 10.7 ms | 16.5 ms | never (see the bug below) |

The first frame comes 30 to 40 ms after the list is ready because of the guest's frame rhythm.

### Resizers (thumbnail view, folder of 5,000, no tags)

| Scenario | ui p50 | ui p95 | ui max |
|---|---|---|---|
| left sidebar | 4.1 ms | 16.8 ms | 23.5 ms |
| right sidebar | 3.7 ms | 14.4 ms | 24.3 ms |
| splitter in the left sidebar | 3.7 ms | 5.2 ms | 6.1 ms |
| splitter in the right sidebar | 4.0 ms | 5.2 ms | 5.7 ms |

## Bugs found by the measurements

- **Empty list after sorting or filtering a scrolled list.** The tile pool was rebuilt for the top of the list while the
  view stayed at its scroll position, and nothing reported the viewport again (neither the place nor the row count had
  changed): the view stayed empty until the next scroll. Found by the probe's blank-tile counter (36 of 36 visible tiles
  empty, for as long as the run waited).
