# Performance project (Browse view) - pause notes

Paused because of the weekly usage limit. Everything below is enough to continue at exactly this point.
Branch: `claude/sharp-faraday-xyc14n` (master = `362d290`, version 0.3.45). **No version bump yet** (bump to 0.3.46 at the first
user-testable state, see CLAUDE.md "Versioning"). Delete `docs/perf/` and `scripts/perf/` content that is only scaffolding when the
user confirms the feature (CLAUDE.md "Tests are scaffolding"); keep nothing here that the final CLAUDE.md section does not need.

## The task (user, German, summarized)
Make the Browse view (grid AND table) maximally fast and frugal: no latency while scrolling, loading a folder, querying or switching
folders; never touch the drive without a manual refresh; RAM must never overflow even for folders with millions of items;
200x200 thumbnails, 1:1; generate 1000 test images + benchmarks (criterion too); show before/after tables for every notable change
(improvements and regressions); research prior art (XnView etc.); review Settings > "Performance & Cache": make options obsolete,
add new ones (RAM budget, default = half of the detected RAM, startup check + warning in the option); **no DB schema change**
unless a large measured gain (then a migration, and the final chat report must name every migration FIRST, see CLAUDE.md "Database
compatibility"); final report: tables + recommended performance settings for 64 GB RAM / modern hardware; answer in German.

## State of the code (compiles: `cargo check --lib --tests --benches --examples` clean except the known `fetch_update` warning)
**Not run yet:** any unit test, any binary of this state, any measurement of the new code. Everything below is unverified at runtime.

Done in the working tree (committed as WIP):
1. **Measurement harness** (env `RUSTVIEW_PERF`, zero cost when unset): `src/ui/perf_probe/` (`plan.rs` parse, `frames.rs` frame/render
   accounting from the rendering notifier, `driver.rs` state machine on a 16 ms timer), `src/telemetry/interval_stats.rs`
   (percentiles), hooks in `main.rs` (rendering notifier + `bridge.start_perf_probe()`), `UiBridge::start_perf_probe`.
   Scenarios: `grid:<px/s>`, `table:<px/s>` (wall-clock scroll top->bottom->top), `jump:<n>`, `switch:<folder name>`, `query:<text>`.
   Metrics: tick gap, **non-render tick time** (tick gap minus render time = UI-thread work, the renderer-independent jank metric),
   frame gaps, slice cost, RSS, load latencies. Scripts: `scripts/perf/` (`run_probe.sh`, `profile_probe.sh` perf profile,
   `idle_probe.sh` idle CPU/wake-ups, `table.py` result table, `gen_images.py` test images, `start_x.sh` Xvfb). All need
   `PERF_DIR=<scratch dir>`. Seed tool: `examples/perf_seed.rs`. Criterion bench: `benches/browse_data.rs` (registered in Cargo.toml;
   contains verbatim `legacy_*` copies of the old caches for before/after in one run).
2. **Single thumbnail RAM cache** `src/thumbnails/ram_cache.rs` (`ThumbnailRamCache`: byte budget, O(1) LRU via slab, stores
   `SharedPixelBuffer` = zero-copy hits, `IdBuildHasher`). Replaces `L1ThumbnailCache` AND the UI's `LruThumbnailStore`
   (`UiState.thumbs` is a handle to the same Arc as `ThumbnailManager::cache()`).
3. **On-demand thumbnail loading** `src/thumbnails/view_loader.rs` (`ViewLoadQueue` pure + 2 `thumb-view-N` threads; a new request
   replaces pending ones, nearest-to-viewport first; results go through the existing ready queue with `VIEW_LOAD_PIPELINE_ID` so
   pipeline starts/cancels never drop them). `ThumbnailManager::{cached_thumbnail, request_view_thumbnails, cancel_view_requests,
   cache_handle}`. **Phase 1 "preload ALL existing thumbnails" of the folder pipeline was removed** (it decoded every thumbnail of the
   folder and queued 120 KB buffers per item: the RAM-overflow risk); the pipeline now only scans LMDB for missing thumbnails and
   generates those. `try_load_existing` stays for non-UI callers. `generator.decompress` uses `into_rgb8` (one copy less).
4. **Ring tile pool instead of slice rebuilds** `src/ui/browse_window/` (`ring.rs` RingWindow, `geometry.rs` ViewGeometry, `rows.rs`
   `update_tile`/`slot_of_media`, `view_builder.rs` `build_view`/`apply_record` (one builder instead of 5 copies),
   `window_sync.rs` `sync_window`). Slint: `MediaItemView.index` (position in the sorted list), grid/table tiles place themselves by
   `item.index`, one stable `VecModel` (set_row_data only for rows whose position changed), new callback
   `viewport-changed(first-row, visible-rows, columns, total)` replaces `visible-chunk-changed`, `visible-start-index` removed, `active`
   property so the hidden view neither reports nor instantiates tiles, placeholder rows have `index = -1` (hidden).
   Rust: `BridgeMedia::handle_viewport_changed`; `reload_with_options` refills the pool with `force`; all former
   `start_window` translations use `update_tile`. `UiState`: `ring`, `view_geometry`, `thumbs`, `invalidate_view_window`,
   `reset_view_window`, `item_at(position)`, `record_by_id`.
5. **`MediaStore`** (`src/query/media_store.rs`): `UiState.all_folder_media` is now id-indexed (O(1) `find/find_mut/position_of`, derefs to a
   slice; ~40 linear scans replaced mechanically). `retain/remove/replace/clear` keep the index.
6. Status bar `update_metrics` is O(selection) now (`UiState::visible_total_bytes()` memoized, `mark_list_bytes_dirty()` at the 6 sites that
   change the list or a size).
7. `reload_with_options`: the `root.is_dir()` probe only runs when the folder is empty (principle: no drive access).
8. **RAM budget setting** replaces `l1_cache_slots` (obsolete, removed; old JSON field is ignored): `performance.memory_budget_mb`
   (0 = automatic = half of usable RAM, usable = min(MemTotal, cgroup limit)), `src/settings/memory_budget.rs` (pure, tests;
   `MemoryPlan::resolve/summary/warning_text`, automatic thumbnail cache capped at 4 GB, manual honored exactly), startup log + warning in
   `main.rs`, live apply in `ui/bridge.rs` (`performance_cache_changed` -> `cache().set_budget`), settings page card
   "Memory (RAM) Budget" with live summary/warnings (`memory-budget-edited` callback). `ThumbnailManager::new` takes a byte budget.
   Schema checklist items 1-5 done, item 6 (apply) done in `ui/bridge.rs`.
   **No DB migration.** No LMDB/pack format change.

## Baseline (before the redesign; commit 362d290 + probe v3, llvmpipe software GL, 1600x900, 4 cores, 1000-image folder, 200 px, 1:1)
Raw lines: `docs/perf/baseline_v3_results.txt`. `nr*` = non-render tick time (ms), `rnd` = render ms (software GL, not representative).

| scenario | tick p50 | tick p95 | tick max | nr p50 | nr p95 | nr p99 | nr max | nr>25 | nr>50 | slices | slice avg | slice max | RSS peak |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| grid 3000 px/s (cold L1) | 26.6 | 57.9 | 134.6 | 9.6 | 17.5 | 68.9 | 86.7 | 38 | 38 | 90 | 22.3 ms | 68.8 ms | 375 MB |
| grid 3000 (warm) | 26.7 | 55.5 | 86.6 | 9.5 | 16.3 | 20.6 | 30.0 | 1 | 0 | 90 | 0.20 | 0.42 | 378 |
| grid 9000 | 36.7 | 65.2 | 88.9 | 10.7 | 18.9 | 21.5 | 24.9 | 0 | 0 | 89 | 0.21 | 0.82 | 376 |
| grid 1500 | 25.9 | 37.2 | 98.6 | 9.1 | 16.6 | 20.2 | 72.3 | 5 | 2 | 90 | 0.22 | 1.03 | 379 |
| table 3000 | 36.9 | 150.7 | 185.2 | 14.6 | 36.4 | 42.4 | 47.1 | 68 | 0 | 70 | 0.14 | 0.94 | 367 |
| table 9000 | 128.5 | 160.1 | 169.3 | 33.7 | 40.8 | 43.0 | 43.0 | 46 | 0 | 46 | 0.13 | 0.23 | 367 |

Idle (no probe, folder shown, 20 s): **0.75 % CPU, 98 context switches/s, 248 MB RSS** (the 16 ms drain timer never stops).
Jump scenario baseline is meaningless (the old handler decodes synchronously, so "complete" = next tick; the UI blocks instead); query
baseline: sync 0.6-0.7 ms, 16.5 ms to first screen (1000 items, thumbnails already cached). `switch:*` scenarios did NOT run in the
baseline (probe could not find the folder in the tree; v3 probe now logs the tree rows on failure, see below).

perf profile of the baseline main thread while scrolling a cold folder (callgraph): 61.8 % FemtoVG render (llvmpipe), **32 % inside
`handle_visible_chunk_changed` = `try_load_existing` = image-webp decode of thumbnails on the UI thread** (cold start restores the last
folder without starting the pipeline, so the first scroll decodes ~100 thumbnails per slice), 6-13 % Slint tile (re)creation
(`MediaTile::new`, repeater), 7.9 % memmove (120 KB copies), 4.9 % libc malloc/free.
Also found: `folder_thumbnails` (2048) + L1 (512) double caching with O(capacity) `retain` LRU; `update_metrics` O(N); 40 linear
`all_folder_media` scans (O(N^2) during generation of large folders); `reload` built `PreparedTarget` with a PathBuf clone for ALL items
on the UI thread; the 16 ms drain timer; both grid and table instantiated their tiles at the same time.

## Immediate next steps (in this order)
1. `git pull` state is this commit. Disk routine (CLAUDE.md): `rm -rf target/debug/incremental`; `cargo clean -p rustview` if < 6 GB free.
2. Targeted tests only: `CARGO_INCREMENTAL=0 cargo test --lib -- ram_cache view_loader browse_window media_store memory_budget interval_stats perf_probe`
   (about 15 min cold). Fix failures.
3. Probe fixes already coded but unbuilt: `switch:` tree-row logging, table `jump` completeness (`pool_complete` returns `rows > 0` in the table).
   If `switch:` still cannot find the folder: the tree rows are logged (`[Perf: Probe] Folder tree rows`); likely the root `lib` must be expanded
   (`folder-toggle-expand`) or names differ.
4. Rebuild (`cargo build --bin rustview --example perf_seed`, ~9 min), then rerun the SAME plan as the baseline against the new binary:
   `PLAN="grid:3000,grid:3000,grid:9000,grid:1500,jump:15,switch:small_100,switch:large_5000,grid:3000,jump:10,switch:main_1000,query:0007,query:,table:3000,table:9000,jump:10"`,
   `PERF_DIR=<scratch> scripts/perf/run_probe.sh new_a <bin> "$PLAN"`, `python3 scripts/perf/table.py <logs>`; idle with `idle_probe.sh`; perf profile with
   `profile_probe.sh`. Expected: cold nr max < 20 ms, warm nr p50 well below 9.5 ms, idle unchanged until step 6.
   Seeding for a fresh container: `python3 scripts/perf/gen_images.py <dir>/images1000` (needs PIL), build the 3-folder layout
   (`lib/main_1000`, `lib/small_100` = 100 hardlinks, `lib/large_5000` = 5 hardlink copies `cN_*`), settings.json with `root_folder=<dir>/lib`,
   `last_folder_path=<dir>/lib/main_1000`, `default_thumbnail_size=200`, `thumbnail_aspect_ratio="1:1"`, `thumbnail_db_size=200`, window maximized 1600x900,
   then `perf_seed <home_seed> 200 lib lib/main_1000 lib/small_100 lib/large_5000`.
5. Verify visually (Xvfb screenshot via `import -window root`): grid tiles positions, selection, hover animation, rubber band, keyboard navigation, view
   toggle grid<->table, query, folder switch, multiview follow, rating/tag updates. Watch for: stale/duplicate tiles at the list end (placeholder rows),
   `viewport-changed` double fills, the `for ... in root.active ? root.media-items : root.no-items` Slint expression (compiled, untested).
6. Next optimizations (each measured, tables to the user):
   - event-driven drain instead of the 16 ms timer (notifier on the ready queue + 100 ms housekeeping timer; target idle < 10 wake-ups/s);
   - pipeline start without cloning a PathBuf for every item on the UI thread (resolve paths for the missing ones on the worker);
   - `row_map` HashMap rebuild (use `IdBuildHasher` / reuse `MediaStore` positions), sort/filter cost for 100k/1M records;
   - loader decode speed: libwebp (`webp` crate) vs image-webp; more view-loader threads; velocity-aware loading (skip requests while the speed is
     above ~1 screen / 200 ms) and a per-frame upload cap if the probe shows upload hitches;
   - compact record store for millions of items (lazy formatted strings instead of ~10 SharedStrings per record; budget accounting for records);
   - scale tests with synthetic 100k / 1M records (thumbnails reused) incl. RSS; run `cargo bench --bench browse_data` once and show the
     before/after table (legacy_* vs new);
   - table view: same ring mechanism is wired (columns = 1, margin 12) but not yet measured.
7. Finish: update CLAUDE.md (Browse windowing architecture, perf probe usage, RAM budget setting, `MediaStore`, view loader; keep "Database
   compatibility" statement: **no migration**), bump 0.3.46 (Cargo.toml, Cargo.lock rustview entry, packaging/PKGBUILD) at the first user-testable
   build, final report: migration statement first ("no migration"), tables (baseline vs new), recommended settings for 64 GB RAM (planned:
   Memory budget = Automatic (32 GB budget, 4 GB thumbnail cache cap is plenty), worker threads = physical cores - 2, thumbnail size 200, keep
   storage format WebP q80; verify with the numbers), what was and was not verified (no clippy in the sandbox). Ask the user about merging to master
   and about stripping benches/probe scaffolding.

## Other facts worth remembering
- Sandbox: Ubuntu 24.04, 4 cores, X11 only (Xvfb :83 + openbox + xcompmgr), llvmpipe inflates render time; `perf` works via
  `/usr/lib/linux-tools/6.8.0-147-generic/perf` (`-e cpu-clock`, `--call-graph dwarf`), installed with `apt-get install linux-tools-generic`.
- A lib rebuild at opt-level 3 takes ~9 min; analysis errors show within ~1 min of a build; editing lib sources 1-2 min after the build started is
  safe, never edit `main.rs` while the bin is still queued.
- Research summary (details in `docs/perf/research_thumbnail_grids.md`): prior-art RAM budgets are small (darktable 12.5 % of RAM, digiKam 5 %,
  Shotwell 85 MB), Slint creates one GL texture per `Image` (no atlas), `set_row_data` on rows inside the window updates in place while
  insert/remove/reset dirty or drop instances; visible-first + cancel-on-scroll + velocity gating are the proven loading policies.
