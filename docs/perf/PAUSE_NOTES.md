# Performance project (Browse view) - pause notes (second pause, budget used up)

Branch `claude/sharp-faraday-xyc14n` (master = `362d290`, version **0.3.46** already bumped in Cargo.toml / Cargo.lock / packaging/PKGBUILD).
Everything is committed and pushed. Raw results of every measurement are in `docs/perf/results/` (see "Results"). Delete `docs/perf/`,
`scripts/perf/`, `benches/`, `examples/perf_*.rs` and the scaffolding tests only when the user confirms the feature (CLAUDE.md "Tests are
scaffolding"); ask the user about merging to master then.

## The task (user, German, summarized)
Make the Browse view (grid AND table) maximally fast and frugal: no latency while scrolling, loading a folder, querying or switching
folders; never touch the drive without a manual refresh; RAM must never overflow even for folders with millions of items; 200x200
thumbnails, 1:1; 1000 test images + benchmarks (criterion too); show before/after tables for every notable change (improvements AND
regressions); research prior art; review Settings > "Performance & Cache" (obsolete options out, RAM budget in: default = half of the
detected RAM, startup check + warning); **no DB schema change** unless a large measured gain (then a migration, and the final chat
report names every migration FIRST - so far: **no migration**); final report: tables + recommended settings for 64 GB RAM / modern
hardware; answer in German.

## Commits so far
- `e1a1df9` WIP redesign (ring tile pool, RAM cache, view loader, MediaStore, RAM budget, probe harness)
- `bcff457` event-driven thumbnail drain, path-free pipeline start, RowMap hasher, probe fixes, **version 0.3.46**
- `66ec661` compact records (~300 B instead of ~1.2 KB), derived labels, libwebp decode, perf_scale example, CLAUDE.md Browse section
- (this pause commit) async list loader for large lists + results + notes. **State of this last commit: `cargo check --lib --tests --benches
  --examples` is clean (only the known `fetch_update` warning, plus the three unused imports that were removed afterwards without
  re-checking), but it was NOT built as a binary, its unit tests were NOT run and nothing was measured or looked at.** Treat it as unverified.

## What the async list loader (this commit) is
Measured before it (100k items, real app): switching to a 100k folder blocks the UI thread 278-300 ms, clearing a query 70 ms; at 1M this
extrapolates to seconds. Design (all in code, unverified):
- `src/query/media_store.rs`: records behind `Arc` (copy-on-write), `generation()` (changes on replace/clear/retain/remove, never repeats),
  `snapshot()`, `from_records()` (index built off the UI thread).
- `src/query/list_layout.rs`: `ListRequest`, `ListLayout`, `lay_out()` = filter + sort + id->row map + total bytes (pure, any thread);
  `RowMap` moved here (re-exported by `ui::bridge_state`).
- `src/ui/list_loader/{mod,worker}.rs`: `LatestJobWorker` (one thread, newest waiting job wins), `ASYNC_ABOVE_RECORDS = 20_000`, two job
  kinds with sequence numbers: *relayout* (superseded by newer relayout / folder load / any synchronous reload) and *folder load*
  (superseded only by a newer folder load). `list_loader::shutdown()` is called from `ui/shutdown/mod.rs::stop_background_work`.
- `src/ui/bridge_media.rs`: `reload_with_options` = (optional sync LMDB load) + `capture_request` + `lay_out` + `commit_layout`;
  `reload_deferred` (used by the query, regex, media-type and sort handlers) lays out on the worker when the list has > 20k records
  and falls back to the sync path when the records changed structurally meanwhile; `commit_layout` is O(tile pool + selection)
  (`FileSelectionModel::prune_where` instead of an O(N) HashSet; `UiState::set_visible_total_bytes`); `PendingList { store, layout, query }`;
  `show_empty_list`; `refill_view` (the grid<->table toggle no longer filters/sorts the whole list again).
- `src/ui/bridge_folders.rs`: `select_folder_internal` split into `fetch_media_ids` / `load_folder_records` / `load_folder_deferred`
  (worker: load + index + `MediaStore::from_records` + layout, then commit on the UI thread if the folder token is still current) /
  `after_folder_list_ready` (the old tail: inspector, status, stubs auto-completion, scan after explicit refresh; moved verbatim).
  Small folders (<= 20k ids) take exactly the old synchronous path. The explicit-refresh scan thread now also builds store + layout on
  its own thread and commits via `commit_layout`.
- Known behavior to verify: while a large folder loads the view shows an empty list and the status bar says "Loading N items...".

## Immediate next steps (in this order)
1. Disk routine (CLAUDE.md): `rm -rf target/debug/incremental`; the sandbox had only ~4-6 GB free (target/ is ~17 GB, scratch binaries are
   ~1 GB each). Free scratch space first (`rustview_base`, `home_seed_huge` 798 MB, run homes). Never run `--release` (it starts a cold
   release build of everything).
2. Build + targeted tests (one chained background command, ~12 min build + ~12 min test build):
   `CARGO_INCREMENTAL=0 cargo build --bin rustview --example perf_scale && CARGO_INCREMENTAL=0 cargo test --lib -- list_loader media_store
   indexed_record folder_dir record_labels sorting generator browse_window memory_budget ram_cache view_loader ready_queue`.
   Fix failures. (Do NOT use `pgrep -f cargo` in a wait loop: it matches its own shell. Use `pgrep -x cargo`.)
3. Re-run the probe on the 100k library: `PERF_DIR=<scratch> scripts/perf/run_probe.sh huge2 <bin> "$PLAN" <home_seed_huge>` with
   `PLAN="jump:15,switch:small_100,switch:huge_100000,query:0007,query:,jump:15,switch:small_100,switch:huge_100000,grid:100000"`
   and compare with `docs/perf/results/probe_huge1.txt` (before async: switch to 100k `sync_ms` 278-300; query cleared 70 ms; first jump p50
   180 ms). Expectation: `sync_ms` of the 100k switch/query below ~20 ms and `nonrender_max_ms` < 30 ms. Also re-run the 1000-item plan
   (`PLAN="grid:3000,grid:3000,grid:9000,grid:1500,jump:15,switch:small_100,switch:large_5000,grid:3000,jump:10,switch:main_1000,query:0007,query:,table:3000,table:9000,jump:10"`
   on `home_seed`) to prove no regression, then screenshots (`scripts/perf/shots_views.sh`, `shots_rb.sh`): empty "Loading" state, folder
   switch, query typing at 100k, view toggle, select all, rubber band.
4. Remaining O(N) work per user event on the UI thread (found by reading, each is an O(N) scan; fix with the row map):
   - `UiState::get_selected_media_ordered` (src/ui/bridge_state.rs) iterates ALL active indices with a HashSet lookup each: used by inspector /
     context menus on every selection change. Replace by: selected ids -> `media_row_map` rows -> sort by row (O(S log S)), keep the
     "empty when `active_indices` is empty" behavior. Add `UiState::row_of(id)` (= `media_row_map.get`, fallback `position_of` when
     `active_indices` is empty).
   - linear position scans: `bridge_file_selection.rs:~201` (every single click!), `keyboard_navigation.rs:~210` (fallback "first selected
     in visual order": use min row of the selected ids), `bridge_viewer_tabs.rs:~858` and `~1653` (tab prev/next availability).
   - `bridge_tags.rs` tag-query flow (`library.media().search` + `preindex_records` + `replace` + reload, lines ~670-840) is still fully
     synchronous: move it onto the same `PendingList`/worker mechanism if time permits (global query over a huge library).
   - `MediaStore::remove/retain` rebuild the whole id index (O(N) per delete): fine for rare actions, note it.
5. Loader decode speed: libwebp decode is in (`ThumbnailGenerator::decompress`); run the new criterion group to get the table:
   `cargo bench --profile dev --bench browse_data -- thumbnail_decode` (the dev profile is already opt-level 3; plain `cargo bench`
   would do a cold release build). Then `cargo bench --profile dev --bench browse_data` once for all before/after groups (`legacy_*` vs new).
   The measured jump latency with cold thumbnails at 100k was 180 ms p50 (~100 tiles decoded by 2 loader threads); consider more loader
   threads (= physical cores - 2) if decode stays above ~1 ms per thumbnail. Velocity-aware loading is not needed so far (see results).
6. Finish: CLAUDE.md (add a bullet about `list_loader`, `MediaStore` copy-on-write snapshots/generation, the 20k threshold and the
   "never block the UI thread for O(N)" rule; the Browse section already documents ring pool, row builder, thumbnails, no polling, budget),
   perf_scale numbers for 1M, final report.
7. Final report (German): migration statement first ("keine Migration"), the before/after tables below (baseline vs new, idle, scale,
   100k), what was and was not verified (no clippy in the sandbox, no real GPU, llvmpipe), recommended settings for 64 GB RAM /
   modern hardware: Memory budget = Automatic (32 GB budget; thumbnail cache automatic cap 4 GB = 34k thumbnails is plenty, RSS grows only
   with what was viewed), worker threads = physical cores - 2, thumbnail size 200, keep WebP q80; mention that records cost ~300 B each
   (1M records ~ 300 MB). Ask the user about merging to master and about stripping the scaffolding.

## Results (raw files in docs/perf/results/)
Host: Ubuntu 24.04 VM, 4 cores, llvmpipe software GL, 1600x900, 200 px / 1:1 thumbnails. `nr*` = non-render tick time (UI-thread work).
All numbers same host; baseline = commit 362d290 + probe (`rustview_base`), `base_b` = its re-run on this host. The non-render floor of
~14-16 ms is the swap/llvmpipe cost, not app code.

| scenario (1000-item folder) | build | nr p50 | nr p95 | nr p99 | nr max | nr>25 | nr>50 | slice reloads (avg ms) | RSS peak |
|---|---|---|---|---|---|---|---|---|---|
| grid 3000 px/s, cold | baseline | 17.2 | 70.3 | 95.3 | 124.4 | 81 | 37 | 90 (23.65) | 372 MB |
| | new2 | 16.5 | 25.7 | 33.0 | 37.7 | 45 | 0 | 475 (0.14) | 326 MB |
| grid 3000, warm | baseline | 16.4 | 30.5 | 36.7 | 40.5 | 70 | 0 | 90 (0.46) | 375 |
| | new2 | 15.9 | 24.1 | 34.5 | 46.4 | 36 | 0 | 476 (0.12) | 330 |
| grid 9000 | baseline | 16.9 | 29.0 | 34.1 | 34.1 | 32 | 0 | 82 (0.47) | 363 |
| | new2 | 14.0 | 24.1 | 38.2 | 38.8 | 9 | 0 | 176 (0.24) | 328 |
| grid 1500 | baseline | 14.7 | 24.7 | 31.6 | 45.8 | 82 | 0 | 90 (0.46) | 375 |
| | new2 | 15.7 | 24.7 | 31.9 | 58.4 | 87 | 2 | 478 (0.12) | 330 |
| table 3000 | baseline | 25.6 | 65.7 | 69.6 | 75.1 | 89 | 48 | 67 (0.27) | 365 |
| | new2 | 21.4 | 27.0 | 29.2 | 45.0 | 43 | 0 | 309 (0.20) | 747* |
| table 9000 | baseline | 58.0 | 69.8 | 77.6 | 77.6 | 34 | 31 | 34 (0.27) | 365 |
| | new2 | 20.8 | 27.3 | 29.4 | 29.4 | 9 | 0 | 72 (0.68) | 747* |

\* new2 ran these after the 5000-item folder: the byte-budgeted RAM cache holds the thumbnails that were viewed (749 MB after scrolling 5000 items
= 120 KB each); the baseline capped at 512+2048 slots. Rendering (software GL) was also faster in new2 (grid cold render p50 36.5 vs 36.1 ms is
equal, table render p50 34/54 ms vs 50/117 ms baseline). `new1` (before the event-driven drain) is in `scroll_table_base_new1_new2.txt`.
Regression to state in the report: grid 1500 px/s shows no improvement in the over-25 ms count (noise level, the floor dominates).

| other (1000-item library) | baseline | new |
|---|---|---|
| idle CPU / context switches per s / RSS | 1.25-1.30 % / 97 / 243 MB | 0.30 % / 37-38 / 221-229 MB |
| jump (random scrollbar jump, pool complete) | not measurable (old handler decoded synchronously) | 16 ms (1 tick) cached |
| switch folder sync part (UI blocked) | scenario did not run (probe could not find folder) | 100 items 3.6 ms, 1000 items 11.7 ms, 5000 items 24.4 ms |
| query "0007" / clear (1000 items) | sync 0.6-0.7 ms, 16.5 ms to first screen | 0.6 ms sync, 16 ms visible |
| `update_metrics` | O(N) | O(selection) |
| startup FirstFrame (stage log) | n/a | ~1.6-1.9 s under llvmpipe |

Records at scale (`examples/perf_scale.rs`, synthetic records, same code paths as the app):

| N | metric | before compaction | after compaction |
|---|---|---|---|
| 100k | bytes per record | 1241 | 305 |
| 100k | build records / store index / row map | 215 / 10.9 / 11.8 ms | 40.7 / 10.2 / 10.7 ms |
| 100k | sort filename / modified / size | 47.7 / 15.4 / 2.9 ms | 24.0 / 17.1 / 2.9 ms |
| 100k | text filter over all records | 23.3 ms | 8.5 ms |
| 1M | resident memory for the records | 1097 MB (1150 B each) | 279 MB (293 B each) |
| 1M | build records | 2053 ms | 461 ms |
| 1M | store index / row map (regression, noise or memory effects) | 178 / 185 ms | 248 / 260 ms |
| 1M | sort filename / modified / size | 953 / 512 / 268 ms | 501 / 569 / 206 ms |
| 1M | text filter over all records | 235 ms | 106 ms |

Real app with a 100k-item folder (`probe_huge1.txt`, compact build, BEFORE the async loader):
- startup directly on the 100k folder works (FirstFrame 1.58 s), RSS peak < 400 MB;
- switch to the 100k folder: UI thread blocked 278-300 ms (`sync_ms`); switch to a 100-item folder 18-20 ms;
- query "0007" (1100 matches) 13.7 ms sync, clearing it (back to 100k) 70 ms;
- jump:15 cold: p50 180 ms / p95 231 ms to a complete pool (thumbnails decoded on demand by 2 threads), nr p95 17 ms; warm: 16 ms;
- grid 100000 px/s fling over 25k rows: nr p50 13.2, p95 20.0, p99 25.5 ms, max 321 ms (one stall), 13 of 1199 ticks > 25 ms;
- thumbnail generation: 315/s (100k in 318 s), 7.7 KB per thumbnail, pack store 775 MB, refresh of 100k records 4.1 s.

Visual checks of new2 (Xvfb screenshots viewed): grid, table, toggle keeps position, folder switch, query, rubber band, select all, keyboard.

## Scratchpad state (ephemeral; recreate if gone)
`/tmp/claude-0/-home-user-rustview/8fcb47df-12c9-513b-9337-a95b0a23c1fb/scratchpad/perf/`: `rustview_base` (baseline binary), `home_seed`
(1000/100/5000 library), `home_seed_huge` (100k + 100), `lib`, `lib_huge`, `images1000`, `runs/*.log`, scripts. Everything needed is in
`scripts/perf/` (`gen_images.py`, `run_probe.sh`, `idle_probe.sh`, `profile_probe.sh`, `shots_views.sh`, `shots_rb.sh`, `seed_huge.sh`,
`table.py`, `start_x.sh`; all need `PERF_DIR=<scratch>`; Xvfb :83 + openbox + xcompmgr, `xdotool`, `imagemagick import`, `perf` via
`/usr/lib/linux-tools/6.8.0-147-generic/perf`). Seed layout: `lib/main_1000`, `lib/small_100` (100 hardlinks), `lib/large_5000` (5 copies),
settings.json with `root_folder`, `last_folder_path`, thumbnail size 200, aspect 1:1, window maximized 1600x900, then `perf_seed`.

## Facts worth remembering
- Build times at opt-level 3: lib ~9-12 min, `cargo check --lib --tests --benches --examples` ~2.5 min; editing lib sources after rustc parsed
  them (1-2 min after start) is safe; never edit `main.rs` while the bin is queued.
- Slint femtovg: one GL texture per `Image`; `set_row_data` inside the model updates in place; replacing the model or insert/remove recreates tiles.
- Research summary: `docs/perf/research_thumbnail_grids.md` (darktable 12.5 % of RAM, digiKam 5 %, Shotwell 85 MB; visible-first + cancel-on-scroll
  + velocity gating are the proven loading policies).
- `xdotool click` loops of thousands of wheel clicks are far too slow; use the probe for scrolling, xdotool only for single actions.
