# Browse view performance project: results

Branch `claude/sharp-faraday-xyc14n`, version 0.3.46. **No database migration** (no schema, LMDB or pack-store change; the old
`l1_cache_slots` setting is simply ignored when present). Raw measurement files: `docs/perf/results/`; prior-art research:
`docs/perf/research_thumbnail_grids.md`; tools: `scripts/perf/`, `examples/perf_seed.rs`, `examples/perf_scale.rs`, `benches/browse_data.rs`.

## What changed (and why)
| Finding in the baseline (362d290) | Change |
|---|---|
| Thumbnails of a slice were decoded (image-webp) on the UI thread: 22-24 ms per cold slice, up to 111 ms for the first one | on-demand view loader threads + one byte-budgeted RAM cache, libwebp decode (2.6x faster); the UI thread only swaps pixels in |
| every slice replaced the model: all Slint tiles (and GL textures) were recreated | one stable ring tile pool (`browse_window`), only the rows whose position changed are written |
| two caches (L1 512 slots + UI store 2048) with O(capacity) LRU, 120 KB copied per hit | one `ThumbnailRamCache`: byte budget, O(1) LRU, zero-copy `SharedPixelBuffer` hits |
| the folder pipeline pre-decoded every existing thumbnail of the folder (RAM overflow risk, 120 KB per item queued) | it only scans LMDB for missing thumbnails and generates those |
| 16 ms polling timer for the thumbnail drain | event-driven drain, 100 ms / 1 s housekeeping timer |
| ~40 linear scans over all records per event, O(N) status bar and selection lookups | id-indexed `MediaStore`, `row_of`, O(selection) selection ordering and status bar |
| ~1.2 KB per record (13 formatted strings, path copies) | ~300 bytes per record, labels derived when a row is built (`RecordLabels`) |
| folder load, query, sort and filter ran completely on the UI thread (O(N log N)) | above 20k records: worker thread against a copy-on-write snapshot, UI thread only commits |
| `root.is_dir()` probe and a second full read of the folder (external stub lookup) on every folder switch | probe only for an empty folder; stub lookup uses the records the load already read |
| RAM options per cache (`l1_cache_slots`) | one RAM budget setting (default half of the RAM, startup check, warnings) |

## Measurement environment
Ubuntu 24.04 VM, 4 cores, Xvfb + openbox + xcompmgr, **llvmpipe software GL** (render times are not representative; the jank metric is the
*non-render tick time* = UI-thread work between frames, floor ~9-16 ms from swap/llvmpipe), 1600x900, 200 px / 1:1 thumbnails, WebP q80.
The host of the VM changed between sessions (host B: `base_b`, `new1`, `new2`, `huge1`; host C: `main3`, `main4`, `huge2-4`, baseline binary
`rustview_base2`). Only numbers measured on the same host in the same session are compared directly.

## 1. Scroll jank, same host (host B), 1000-item folder
`nr` = non-render tick time (ms), `slices` = tile-pool/slice updates, `slice avg` = UI-thread cost per update.

| scenario | build | nr p50 | nr p95 | nr p99 | nr max | ticks >25 ms | ticks >50 ms | slices (avg ms) | RSS peak |
|---|---|---|---|---|---|---|---|---|---|
| grid 3000 px/s, cold | baseline | 17.2 | 70.3 | 95.3 | 124.4 | 81 | 37 | 90 (23.65) | 372 MB |
| | new | 16.5 | 25.7 | 33.0 | 37.7 | 45 | 0 | 475 (0.14) | 326 MB |
| grid 3000, warm | baseline | 16.4 | 30.5 | 36.7 | 40.5 | 70 | 0 | 90 (0.46) | 375 |
| | new | 15.9 | 24.1 | 34.5 | 46.4 | 36 | 0 | 476 (0.12) | 330 |
| grid 9000 | baseline | 16.9 | 29.0 | 34.1 | 34.1 | 32 | 0 | 82 (0.47) | 363 |
| | new | 14.0 | 24.1 | 38.2 | 38.8 | 9 | 0 | 176 (0.24) | 328 |
| grid 1500 | baseline | 14.7 | 24.7 | 31.6 | 45.8 | 82 | 0 | 90 (0.46) | 375 |
| | new | 15.7 | 24.7 | 31.9 | 58.4 | 87 | 2 | 478 (0.12) | 330 |
| table 3000 | baseline | 25.6 | 65.7 | 69.6 | 75.1 | 89 | 48 | 67 (0.27) | 365 |
| | new | 21.4 | 27.0 | 29.2 | 45.0 | 43 | 0 | 309 (0.20) | 747* |
| table 9000 | baseline | 58.0 | 69.8 | 77.6 | 77.6 | 34 | 31 | 34 (0.27) | 365 |
| | new | 20.8 | 27.3 | 29.4 | 29.4 | 9 | 0 | 72 (0.68) | 747* |

\* after the 5000-item folder: the RAM cache holds what was viewed (120 KB per 200 px thumbnail), bounded by the budget.
Not better: grid 1500 px/s (noise level, the floor dominates). More slice updates in the new build are intended (one per row instead of one
per ~120-item slice), each costs 0.1-0.7 ms instead of 0.2-23 ms.

Final build on host C (faster host, not comparable to the table above), two full runs: `main3`/`main4` in `docs/perf/results/`:
nr p99 18-24 ms in the grid, 20-23 ms in the table, over-50 ms ticks: 0 in all 14 runs, reproducible between the two runs.

## 2. Idle, same host (host C), 1000-item folder shown, 20 s, interleaved
| build | CPU | context switches / s | RSS |
|---|---|---|---|
| baseline | 1.15-1.20 % | 98 | 248 MB |
| final | 0.20-0.25 % | 37-38 | 226 MB |

## 3. 100k-item library (hardlinked test images, 100 100-item copies, thumbnails generated; 315 thumbnails/s, 7.7 KB each)
| measurement | baseline (362d290, host C) | before the async loader (compact build, host B) | final (host C) |
|---|---|---|---|
| UI thread blocked by restoring the 100k folder at startup | ~620 ms + 111 ms first slice decode | - | **~10 ms** (hand-off 9 ms + commit 0.7 ms); list ready after 193 ms on the worker |
| UI thread blocked by a folder switch to the 100k folder | not measured separately (same load path as at startup: ~620 ms) | 278-300 ms | **1.2-1.7 ms** (+ 0.4-0.7 ms commit; worker 133-212 ms) |
| query "0007" (1100 matches) | - | 13.7 ms | **0.1 ms** (worker 8-10 ms, commit 0.5 ms) |
| clearing the query (back to 100k) | - | 70 ms | **0.1 ms** (worker 22-27 ms, commit 0.5 ms) |
| 15 random scrollbar jumps, cold thumbnails: time to a complete tile pool, p50 / p95 | - | 180 / 231 ms | 104-113 / 141-170 ms (render-bound under llvmpipe) |
| the same jumps, thumbnails cached | - | 16 ms | 16 ms |
| fling 100000 px/s over 25k rows (grid): nr p50 / p99 / max | - | 13.2 / 25.5 / 321 ms | 8.8 / 14.6 / 36.9 ms (1 tick > 25 ms) |
| table 100000 px/s over 100k rows: nr p50 / p99 / max | - | - | 16.7 / 23.3 / 29.4 ms (5 ticks > 25 ms, none > 50 ms) |
| RSS idle after startup | 405 MB | - | 277 MB |
| RSS after pressing refresh (scan, list rebuilt) | 625 MB | - | 363 MB |

The 56-100 ms stall that remained after the first async version (external stub lookup re-reading the whole folder on the UI thread) was found
with `perf` and removed: folder switch `nr max` 56-100 ms -> 22-26 ms.

## 4. Records at scale (`examples/perf_scale.rs`, synthetic records, same code paths as the app)
| N | metric | before | after |
|---|---|---|---|
| 100k | bytes per record | 1241 | 305 |
| 100k | build / index / row map | 215 / 10.9 / 11.8 ms | 40.7 / 10.2 / 10.7 ms |
| 100k | sort filename / modified / size | 47.7 / 15.4 / 2.9 ms | 24.0 / 17.1 / 2.9 ms |
| 100k | text filter over all records | 23.3 ms | 8.5 ms |
| 1M | records in RAM | 1097 MB (1150 B each) | 279 MB (293 B each) |
| 1M | build records | 2053 ms | 461 ms |
| 1M | index / row map (regression, noise or memory effects) | 178 / 185 ms | 248 / 260 ms |
| 1M | sort filename / modified / size | 953 / 512 / 268 ms | 501 / 569 / 206 ms |
| 1M | text filter over all records | 235 ms | 106 ms |

At 1M records the work that used to block the UI thread (index, sort, filter, row map ~ 1.1 s) now runs on the worker; the commit stays O(tile pool).

## 5. Micro benchmarks (criterion, `cargo bench --profile dev --bench browse_data`, host C; full output `bench_browse_data_full.txt`)
| benchmark | before (legacy copy) | after |
|---|---|---|
| thumbnail decode 200 px (lossy WebP, ~8 KB) | 2.85 ms (image-webp) | **1.08 ms** (libwebp) |
| thumbnail RAM cache hit | 100.8 µs (copy 120 KB into a buffer) | 40 ns (zero-copy, 120 hits in 4.8 µs) |
| slice of 120 hits | 2.5 µs (UI store, no recency update) | 4.8 µs (with O(1) recency update): slightly slower, irrelevant in absolute terms |
| insert with eviction | 15.9 µs (L1) / 103.9 µs (UI store) | 95.6 µs |
| record lookup by id, 1k / 100k records | 440 ns / 341.9 µs (linear) | 3.6 ns / 3.0 ns |
| build 120 rows | 27.0 µs (strings stored per record) | 233 µs (derived labels): **8.6x slower per row (217 ns -> 1.5 µs)**, accepted for ~4x less RAM |
| advance the tile pool by one row | - | 16 ns |
| sort 100k records by file name | - | 14.0 ms (parallel) |

## 6. Tests and checks
- targeted unit tests of the last state: 79 passed, 0 failed (`unit_tests_final.txt`; includes records, store snapshots, list loader jobs,
  latest-job worker, labels, sorting, libwebp round trip, memory budget, RAM cache, view loader, ring window, probe plan);
- screenshots (grid, table, toggle keeps the scroll position, folder switch, query typing at 100k, rating query with and without results,
  restore after clearing, rubber band, select all, keyboard navigation, settings page with the RAM budget card incl. manual mode);
- **not verified:** clippy (not run in the sandbox by project rule), real GPU rendering, Wayland, KWin, a library on slow disks, a baseline scroll
  measurement on host C (the baseline has no probe; the host-B pair in section 1 is the same-host comparison), 1M-item folders in the real app
  (only the synthetic scale test), Ctrl+A on 1M items (O(N) selection set, ~100-200 ms expected).

## 7. Recommended settings for a 64 GB machine with a modern CPU
- RAM budget: **Automatic** (32 GB budget; thumbnail cache capped at 4 GB = ~34k thumbnails of 200 px, which already covers several screens of scrolling
  back and forth; RSS only grows with what was actually viewed). A manual value above ~24 GB buys nothing measurable.
- Rayon worker threads: default (logical cores - 2); the view loader now takes half of them (2..4 threads) for scrollbar jumps.
- Thumbnail size 200 px, WebP, quality 80 (8 KB per thumbnail; decode 1.1 ms); LMDB batch commit size: default 32.
- Folders of 100k+ items are fine; records cost ~300 bytes each (1M items = ~300 MB), thumbnails are loaded on demand.

## 8. Known limits and ideas
- `MediaStore::remove/retain` rebuild the id index (O(N) per delete) and Ctrl+A on huge lists builds a selection set of all ids (O(N)).
- The empty list shown while a large folder loads is intentional (the tiles of the previous folder must not stay clickable).
- Cold-thumbnail jumps at 100k are render-bound under llvmpipe; on a real GPU the decode (1.1 ms per thumbnail, 2-4 threads) should dominate.
