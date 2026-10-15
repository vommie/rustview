# Prior art: thumbnail caching, virtual scrolling, RAM budgets (research for RustView)

Date: 2026-10-15. Method: WebSearch (standard, one extended) + WebFetch.

Verification legend (used on every finding):
- [SRC] = I fetched and read the primary source (code or official doc) in this session.
- [SEARCH] = taken from a search-result summary only (secondary; could not open the page). Treat as lower confidence.
- [INFER] = my own inference/calculation, not stated by a source.
- [MEM] = from my background knowledge, NOT re-verified here.

Access limits (be aware): WebFetch only resolved github.com, raw.githubusercontent.com, developer.apple.com and
developer.android.com. docs.rs, arxiv, wikipedia, doc.qt.io, web.dev, darktable.org, freedesktop.org, chromium.googlesource.com,
xnview.com, photoprism docs etc. were unreachable (DNS), so those topics rely on [SEARCH] summaries or on GitHub mirrors of the code.
No fabricated numbers: where I found no number, it says so.

---------------------------------------------------------------------------------------------------

## Q1. How existing apps cache thumbnails (store format, RAM budget, eviction, prefetch, fast scroll)

### darktable (mipmap cache) - the most relevant design
- Three separate caches: `mip_thumbs` (levels mip0..mip10, 8-bit RGBA), `mip_f`, `mip_full`. Level sizes: mip0 180x110, mip1 360x225,
  mip2 720x450, mip3 1440x900 ... mip9 7680x4320. [SRC] https://raw.githubusercontent.com/darktable-org/darktable/master/src/common/mipmap_cache.c
- Budget: `mipmap_memory`, clamped to 100 MB..8 GB; thumbnail cache cost per entry = its buffer bytes (w*h*4 + header). [SRC] same file.
- Default budget is a FRACTION of total RAM, not absolute: resource level "default" = 128/1024 = 12.5 % of total RAM for the mipmap cache
  (small = 64/1024, large/unrestricted = 128/1024); general processing memory "default" = 512/1024 = 50 % of total RAM
  (total first reduced by 1 GB when < 8 GB; floor 512 MB). Total comes from `MemTotal` in /proc/meminfo; no MemAvailable, no cgroup
  handling in that file. [SRC] https://raw.githubusercontent.com/darktable-org/darktable/master/src/common/darktable.c
  (Note: darktable's own default for general memory is 50 %, the same as RustView's planned default.)
- Cache core (`cache.c`): GLib hash table + GList LRU (oldest at head) + ONE global mutex + per-entry rwlock. Eviction runs only on a miss
  (`dt_cache_get`): if cost > 0.8*quota, `dt_cache_gc` walks from the oldest entry until cost < 0.8*quota; entries that are locked by a
  reader/writer are skipped ("best-effort, never blocks, never fails" - the cache can exceed the quota). [SRC]
  https://raw.githubusercontent.com/darktable-org/darktable/master/src/common/cache.c
- "Stand-in" lookup: best-effort get tries the requested mip level, then smaller levels down to mip0, then larger ones, so something is
  always drawn while the right size loads. [SRC] mipmap_cache.c
- Persistent store: one JPEG file per thumbnail `<cachedir>.d/<mip>/<imgid>.jpg`, quality from `database_cache_quality` (10-100); not written
  if < 100 MB free disk; optional (`cache_disk_backend`). [SRC] mipmap_cache.c. Prefetch requests run as background jobs (`DT_JOB_QUEUE_SYSTEM_FG`).
- Lighttable grid (`thumbtable.c`): creates widgets only for tiles intersecting the view, NO look-ahead margin; leaving tiles go to an
  "invalid" list and are recycled; scroll events are batched with a 10 ms timer; loading is synchronous in the move handler; the
  background thumbnail crawler only starts if available memory > 3000 MB. [SRC]
  https://raw.githubusercontent.com/darktable-org/darktable/master/src/dtgtk/thumbtable.c
- Design history: 2011 post "a new caching backend" describes LRU + hash table + rw locks. [SEARCH] https://darktable.org/2011/10/a-new-caching-backend

### digiKam
- Persistent thumbnails are in a SEPARATE SQLite database (`thumbnails-digikam.db`, schema v3) so the main DB stays small/backup-friendly; stored
  encoding is PGF (compression level 4, code comment: "do not use a value > 4, else image is blurred"); lookup by unique hash + file size,
  fallback to file path; a record is valid only if its mtime is not older than the file. Alternative backend: freedesktop PNG files. [SRC]
  https://raw.githubusercontent.com/KDE/digikam/master/core/libs/threadimageio/thumb/thumbnailcreator.cpp ,
  https://raw.githubusercontent.com/KDE/digikam/master/core/libs/threadimageio/thumb/thumbnailcreator.h
- DB size reports (anecdotal): 1.3 GB for a 650 GB collection; 8.5 GB; one user 36.4 GB even after cleanup/VACUUM. [SEARCH]
  https://bugs.kde.org/show_bug.cgi?id=166564 , https://mail.kde.org/pipermail/digikam-users/2026-August/038338.html
- RAM cache is Qt `QCache` with BYTE cost: pixmap cache max cost = 100 (count) x maxThumbsSize^2 x depth/8; QImage thumbnail cache = 5 images;
  full-image cache = clamp(5 % of RAM, 60..200 MB). Code comment says the pixmap count "should not be based on system memory, it's graphics
  memory". Path->cache-key hash is garbage-collected when it grows beyond 5x the cache count. [SRC]
  https://raw.githubusercontent.com/KDE/digikam/master/core/libs/threadimageio/fileio/loadingcache.cpp

### KDE Dolphin / KIO (freedesktop cache)
- KIO `PreviewJob`: cache lookups run in a thread pool of 4 threads in batches of 32; hits are emitted immediately and removed from the
  generation list; generation is capped by `maxWorkers("thumbnail")`; cache key = URI + mtime + size. [SRC]
  https://raw.githubusercontent.com/KDE/kio/master/src/gui/previewjob.cpp
- Dolphin `KFileItemModelRolesUpdater`: `ReadAheadPages = 5` (visible area +/- 5 pages are resolved), `ResolveAllItemsLimit = 500`, 100 ms debounce
  for changed items, 200 ms max blocking time; queue order = visible files, visible dirs, pages after, pages before (reverse), first/last page;
  on every visible-range change `startUpdating()` KILLS the running preview job, clears pending indexes and restarts with the new ones; preview
  cache size 256x256 if icon size > 128 else 128x128. [SRC]
  https://raw.githubusercontent.com/KDE/dolphin/master/src/kitemviews/kfileitemmodelrolesupdater.cpp

### freedesktop thumbnail spec / Nautilus / GNOME
- Buckets normal 128, large 256, x-large 512, xx-large 1024 (x-large/xx-large added in spec 0.9.0, Dec 2020); PNG files named MD5(URI).png in
  `$XDG_CACHE_HOME/thumbnails/<bucket>/`; tEXt keys Thumb::URI / Thumb::MTime; failures in `fail/<app>/` as 1x1 PNG. [SRC]
  https://raw.githubusercontent.com/GNOME/gnome-desktop/master/libgnome-desktop/gnome-desktop-thumbnail.c ; [SEARCH] https://specifications.freedesktop.org/thumbnail-spec/latest/x127.html
- Weakness for millions of files: one inode per thumbnail in flat directories (2007 xdg thread proposed nested dirs because lookups got slow). [SEARCH]
  https://lists.freedesktop.org/archives/xdg/2007-August/008750.html . A pack file + offset index (RustView) avoids this.
- Nautilus: single FIFO hash queue with dedupe of duplicate requests (callbacks merged), drained by a LOW-priority idle callback, thread cap
  ceil(ncpu/2) (+1 because of `<=`), files modified < 3 s ago are deferred; per-request GCancellable; NO visible-first ordering in that file. [SRC]
  https://raw.githubusercontent.com/GNOME/nautilus/main/src/nautilus-thumbnails.c

### Shotwell
- RAM cache is a byte-weighted LRU per size: 128 px = 30 MiB, 360 px = 40 MiB, 512 px = 15 MiB; items > 512 KiB are not cached; disk store = JPEG
  files per size (`thumbs128/360/512`); worker pool = threads_per_cpu(1); fetch jobs of NEAREST/TILES interpolation get HIGH priority. [SRC]
  https://raw.githubusercontent.com/GNOME/shotwell/master/src/ThumbnailCache.vala
- Grid tile: on expose shows low-quality (smallest size) first, schedules the high-quality fetch 100 ms later (`HQ_IMPROVEMENT_MSEC`), and cancels
  outstanding fetches on unexpose/resize/destroy (Cancellable). [SRC] https://raw.githubusercontent.com/GNOME/shotwell/master/src/Thumbnail.vala

### Geeqie / gThumb / nomacs
- Geeqie: thumbnails as freedesktop-style PNG (or own cache) with mtime check; zero-byte file = failure marker; NO separate RAM cache (only
  `fd->thumb_pixbuf` per file entry); loaders run at G_PRIORITY_LOW. [SRC] https://raw.githubusercontent.com/BestImageViewer/geeqie/master/src/thumb.cc
- gThumb: file URL 404, not verified. nomacs: no documentation of its thumbnail cache found (only an anecdote that nomacs does not mass-cache a
  folder as Gwenview does). [SEARCH]

### XnView MP, Windows Explorer, Lightroom, Capture One, ACDSee, FastStone, PhotoPrism, Immich/Google Photos
- XnView MP: thumbnails in its own database file (xnview.db/Thumb.db); wiki: WebP compression gives a smaller DB "so more of it stays in memory";
  forum reports show the configured memory limit NOT being honored (10 GB RAM at 60k photos with a 2 GiB setting; 20 GB DB with a 100 MB cap)
  and "loads all thumbnails at once" in old versions. Lesson: enforce the budget inside the cache, not by policy. [SEARCH]
  https://newsgroup.xnview.com/viewtopic.php?p=137102 , https://forum.xnview.com/viewtopic.php?p=184143 (could not open the official wiki)
- Windows Explorer: `thumbcache_32/96/256/1024.db` (+ `_sr`, `_idx`): per-size pack files plus an index mapping thumbnail IDs to offsets; 32/96 bitmap,
  256/1024 JPEG-based (Win7 forensics deck; some PNG entries seen); no path stored in the data files. This is the same architecture as RustView's
  pack file + offsets. [SEARCH] https://www.slideshare.net/ctin/windows-7-forensics-thumbnaildtlr4 , https://forensics.wiki/windows_thumbcache/
- Lightroom Classic: preview pyramid in `Previews.lrdata` (standard previews at monitor resolution, 1:1, smart previews = lossy DNG ~2560 px); folder can
  be huge (reports of 245 GB; 250k preview files for 70k images); safe to delete, rebuilt on demand. [SEARCH] Adobe community threads
  (https://community.adobe.com/t5/lightroom-classic-discussions/good-way-to-clean-up-old-previews/m-p/14026919). Capture One: nothing usable found.
- ACDSee: per-catalog `Thumb1-3.fpt` files + CDX index; FastStone 7.6: new DB engine + "pre-scan folders into thumbnail database" tool. No cache
  size/eviction details published. [SEARCH] https://support.acdsee.com/en/support/solutions/articles/60000475101 , https://www.neowin.net/news/faststone-image-viewer-76/
- PhotoPrism: sizes `tile_224` and `tile_500` always generated, larger sizes on demand; thumbnails dominate the storage folder. [SEARCH]
  https://docs.photoprism.app/developer-guide/media/thumbnails
- Immich web timeline: `TimelineManager` -> `TimelineMonth` -> `TimelineDay`; month height derived from the bucket's asset COUNT (so the scrollbar
  covers the whole library before anything loads); sliding window/visibility proximity flags (`isInViewport`, `isInOrNearViewport`); month loads are
  `CancellableTask`s with an AbortSignal; no debounce in the manager file; fixed 60 px bottom section. Their doc "how to scroll like Google
  Photos" estimates scrollbar height assuming square 235 px thumbnails. [SRC] https://raw.githubusercontent.com/immich-app/immich/main/web/src/lib/managers/timeline-manager/timeline-manager.svelte.ts ;
  [SEARCH] https://deepwiki.com/immich-app/immich/3.5-timeline-and-asset-display . Google Photos web: all sections present with estimated heights,
  only visible photos in DOM (secondary write-up only). [SEARCH] https://dev.to/shaileshpandit/how-to-implement-google-photos-grid-in-javascript-575i

### Q1 take-aways
- Persistent formats that scale: SQLite BLOB DB (digiKam, XnView, FastStone), per-size pack files + index (Windows), per-file JPEG/PNG (darktable,
  Shotwell, freedesktop, Geeqie). Per-file stores are the ones that hurt at millions of items. [INFER from the above]
- Nobody on this list uses a decoded-RGB cache bigger than a few hundred MB by default (darktable 12.5 % RAM, digiKam 100 pixmaps, Shotwell 85 MiB total).
  A 50 % default is far more aggressive than all prior art; it only makes sense if the budget is enforced hard and shared with the metadata columns. [INFER]
- Common scroll behaviour: visible first, read-ahead of a few pages (Dolphin 5), cancel/restart the queue on every scroll step (Dolphin, Shotwell, iOS,
  Android), show a cheap stand-in (darktable smaller mip, Shotwell low quality) instead of blanks.

---------------------------------------------------------------------------------------------------

## Q2. Virtual scrolling / windowing best practice

### Prefetch, priority, cancellation (mobile and browser engines)
- Android RecyclerView `GapWorker` (source read): posts a Runnable after a traversal; predicts next vsync = latest drawing time + frame interval; per view
  `velocity = |dx|+|dy|`; a task is `neededNextFrame = distanceToItem <= velocity`; sort order = (still tied to a view) > neededNextFrame > higher view
  velocity > smaller distance; tasks needed next frame get a FOREVER deadline (do it now even if it does not fit), others must fit in the idle gap;
  an item that fails to bind in time goes to the recycled pool, only successfully bound items enter the view cache. [SRC]
  https://raw.githubusercontent.com/androidx/androidx/androidx-main/recyclerview/recyclerview/src/main/java/androidx/recyclerview/widget/GapWorker.java
  Secondary: create/bind durations are averaged per view type to decide whether a prefetch fits the gap. [SEARCH] https://medium.com/google-developers/prefetch-text-layout-in-recyclerview-4acf9103f438
  `LayoutPrefetchRegistry.addPosition(position, pixelDistance)` = priority is pixel distance. [SEARCH] https://developer.android.com/reference/androidx/recyclerview/widget/RecyclerView.LayoutManager.LayoutPrefetchRegistry
- iOS `UICollectionViewDataSourcePrefetching`: `prefetchItemsAt` = start async loading (not necessarily called for every cell), optional
  `cancelPrefetchingForItemsAt` = cancel in-flight loads for items that scrolled away; sample uses OperationQueue (cancellation) and, in `cellForItemAt`,
  shows an empty cell until data arrives and re-checks `representedIdentifier` on completion because cells are recycled. [SRC]
  https://developer.apple.com/documentation/uikit/prefetching-collection-view-data.md . "Order of index paths = priority": only via third party quote. [SEARCH]
- Chromium compositor tiles: `PriorityBin {NOW, SOON, EVENTUALLY}` then `distance_to_visible`; `AssignGpuMemoryToTiles` walks the queue highest priority first,
  stops at the first tile that violates the memory policy, NOW tiles may use the hard memory limit while the rest use the soft limit, and a per-frame
  cap on scheduled raster tasks exists. [SRC] https://raw.githubusercontent.com/chromium/chromium/main/cc/tiles/tile_priority.h ,
  https://raw.githubusercontent.com/chromium/chromium/main/cc/tiles/tile_manager.cc . Skewport (velocity-extrapolated rect gets SOON) and
  checkerboarding (draw background/stale content rather than block the frame) are described only in third-party summaries. [SEARCH]
  https://grida.co/docs/wg/research/chromium/interaction-and-quality
- Pause while flinging: Picasso `pauseTag/resumeTag` from `onScrollStateChanged` (pause on fling, resume on idle) because otherwise "a request for every
  row is started and cancelled again"; same pattern in older AbsListView image loaders. [SEARCH] https://futurestud.io/tutorials/request-management-by-grouping-of-images-via-tag
- LIFO / priority: an Android image loader uses a LIFO pool so newest (= currently visible) requests load first; KPhotoAlbum loads visible thumbs at higher
  priority than the preloader; QuickMedia removes a thumbnail's pending load from the queue when it is no longer visible. [SEARCH]
  https://github.com/flyfire/android-http-image-manager , https://git.plan9.rocks/cat/QuickMedia/commit/88cd57714af6bfc307d41f9dfa74da45600d51e0

### Qt Quick
- `cacheBuffer` default 320 (content units, i.e. pixels in practice) beyond the viewport on both sides; `displayMarginBeginning/End` = range that must be
  filled immediately; the cache buffer is filled incrementally AFTER a 16 ms pause and only when the fill pass created nothing, and `reuseItems` keeps a
  delegate pool. [SRC] https://raw.githubusercontent.com/qt/qtdeclarative/dev/src/quick/items/qquickitemview.cpp . Docs wording "smoother scrolling at the
  expense of memory": [SEARCH] https://doc.qt.io/qt-4.8/qml-gridview.html . Image: `sourceSize` bounds decoded size, `asynchronous: true` for local files. [SEARCH]
  https://wiki.qt.io/Performance_tip_Images

### Slint (RustView's toolkit)
- `ListView` is "virtualized: elements are only instantiated if they are visible"; requires a `for` child and assumes uniform item height. [SRC]
  https://raw.githubusercontent.com/slint-ui/slint/master/docs/astro/src/content/docs/reference/std-widgets/views/listview.mdx
- Repeater internals (`internal/core/model/repeater.rs`): in list-view mode only the visible rows get instances; item height cached
  (`content_height = cached_item_height * row_count`); if the view moved more than 1.5 screens all instances are cleared and re-seeded at the new offset,
  otherwise it walks/prepends; instances are filled until `y >= listview_height` (no overscan margin in the algorithm itself); `row_changed` on a row inside
  the instance window updates that instance in place (no re-creation), outside the window it is ignored; `row_added/row_removed` shift the offset or splice dirty
  placeholders and mark all TRAILING instances dirty; `reset()` drops every instance. The non-listview path treats counts > 2^20 as "presumably a bug". [SRC]
  https://raw.githubusercontent.com/slint-ui/slint/master/internal/core/model/repeater.rs
- Slint `ListView` is a single-column vertical list; a grid therefore needs a model of ROWS (each row a small model of N tiles) or a custom Flickable. [INFER]
  Per-row `set_row_data` is the targeted update; model resets or inserts/removes during a scroll dirty everything below. [SRC for mechanics / INFER for advice]

### GPU texture upload / atlases (Slint femtovg)
- femtovg: `draw_image_impl` caches the rendered texture PER ITEM in `graphics_cache.get_or_update_cache_entry(item_rc, ...)`; a shared `TextureCache`
  (key = ImageCacheKey + target size for SVG + rendering + tiling) is DRAINED of entries without outside users after every frame; each `Texture` owns one
  femtovg `ImageId` (GL texture) deleted on drop; NO atlas packing in Slint's code; no max texture count; graphics cache cleared when the scale factor changes.
  RGB8/RGBA8/RGBA8-premultiplied go to femtovg without CPU conversion (only RGB565 is converted). [SRC]
  https://raw.githubusercontent.com/slint-ui/slint/master/internal/renderers/femtovg/images.rs , .../itemrenderer.rs , .../lib.rs
- `Image::from_rgb8/rgba8` produce `ImageCacheKey::Invalid` ("programmatically created images"), so they never use the shared `TextureCache` lookup; `Image` clones
  are cheap (shared buffer) but `Image` is not `Send` (thread-local caches) - build the `SharedPixelBuffer` on workers, create the `Image` on the UI thread. [SRC]
  https://raw.githubusercontent.com/slint-ui/slint/master/internal/core/graphics/image.rs
- Consequences [INFER]: one GL texture per visible tile instance; assigning a new `source` re-uploads that tile (200x200x4 = 160 KB); the same pixel buffer shown
  in two item instances (grid + table) uploads twice; a scale-factor change (DPI/monitor move) forces re-upload of every visible tile in one frame.
- Upload cost evidence is weak/old: forum reports that PBOs were not faster for many small tiles (~10 % slower for 20-hundreds of tiles/frame), that
  asynchrony is the only benefit, and one DXT1 2048x2048 upload took 2-5 ms on old Intel; no authoritative numbers for 200x200. [SEARCH]
  https://community.khronos.org/t/pbos-slow-looking-for-the-reason/56385 , https://community.khronos.org/t/pbo-glcompressedteximage2d-async-is-slow-dxt/77132
  => measure per-frame upload time in RustView with a counter; do not assume. [INFER]

### Placeholders
- Evidence on skeleton/placeholder vs blank is MIXED: Viget 2017 found spinner users reported shorter perceived waits than skeleton users; Mejtoft et al. 2018
  (ECCE) reported better perceived speed with skeletons (via secondary summary). Robust benefit = layout stability (no shifting). [SEARCH]
  https://viget.com/articles/a-bone-to-pick-with-skeleton-screens , https://nngroup.com/articles/skeleton-screens/
- Engines all prefer "draw something cheap and never block": Chromium checkerboard, darktable stand-in mip, Shotwell low-quality first, iOS empty cell. [SRC/SEARCH above]

---------------------------------------------------------------------------------------------------

## Q3. Cache replacement for thumbnail caches

- S3-FIFO (SOSP'23, Yang et al.): three static FIFO queues (small probation, main, ghost); "quick demotion" of one-hit wonders. Claims: evaluated on 6,594
  traces from 14 datasets, lowest mean miss ratio on 10 of 14, ~6x the throughput of optimized LRU at 16 threads. Parameters (small = 10 %, ghost ~ main size,
  freq cap 3) are from my memory of the paper [MEM]. [SEARCH/README] https://github.com/Thesys-lab/sosp23-s3fifo , https://junchengyang.com/publication/sosp23-s3fifo.pdf
  libCacheSim implements FIFO, LRU, Clock, SLRU, ARC, TwoQ, LIRS-like variants, WTinyLFU, S3-FIFO, SIEVE for simulation on your own trace. [SRC] https://github.com/cacheMon/libCacheSim
- W-TinyLFU / Caffeine: no ghost keys, 4-bit CountMin sketch (~8 B/entry), window vs main adapted by hill climbing; "near optimal, competitive with ARC and LIRS, big
  improvement over LRU". ARC needs 2x capacity of metadata and is IBM-patented; LIRS needs ~3x. LRU "may have a poor hit rate in cases like full scans". [SRC]
  https://github.com/ben-manes/caffeine/wiki/Efficiency
- Rust crates:
  - `moka`: default TinyLFU ADMISSION (LFU) + LRU eviction; `EvictionPolicy::lru()` selectable; `weigher` returns u32 and bounds `max_capacity` by total weight,
    but "weighted sizes are not used when making eviction selections"; `entry_count()` is approximate until `run_pending_tasks()`; Window-TinyLFU is only on the
    roadmap. A test comment shows that a new key can be REJECTED because its frequency is too low. [SRC] https://github.com/moka-rs/moka ,
    https://raw.githubusercontent.com/moka-rs/moka/main/src/sync/cache.rs
  - `quick_cache`: "modified Clock-PRO, very similar to S3-FIFO", scan resistant, claims hit rate between LRU and W-TinyLFU class; `Weighter` trait + weight capacity
    (zero-weight items never evicted, weight must not change), ~21 bytes/entry overhead, ghost allocation default 0.5, pinned items via `Lifecycle::is_pinned`,
    `sync` and faster `unsync` variants. Author's mokabench table (i9-12900H): usually the best or near-best hit ratio and 2-4x faster than mini-moka,
    "benchmarks can be misleading". [SRC] https://raw.githubusercontent.com/arthurprs/quick-cache/master/src/lib.rs , https://github.com/arthurprs/quick-cache
  - `clru`: LRU with weight scale; invariant `len + weight <= capacity`; an insert may evict the whole cache, or FAIL if the item is too heavy. [SEARCH] https://docs.rs/clru
  - `lru`: count-based only [MEM]; `schnellru` has a memory-usage limiter [MEM, not verified].
- What prior apps use: darktable = own LRU with a 0.8 GC watermark (see Q1), Shotwell = byte-weighted LRU, digiKam = Qt QCache (byte cost). [SRC]
- Fit for scrolling [INFER]: a million-item folder browsed top to bottom is almost all compulsory misses; re-references are mainly scroll-back within a few pages and
  repeated visits to the same folder. Frequency-based admission (TinyLFU) adds little and can reject the very tile that is about to be drawn (moka test comment).
  What matters: (1) viewport and near-viewport entries are pinned/never evicted (darktable skips locked entries; quick_cache has `is_pinned`), (2) byte-weighted budget,
  (3) scan resistance so a background bulk pass (e.g. "generate all", sorting by a column that touches everything) does not flush the working set (S3-FIFO small queue / SLRU).
  Alternative that needs no library: evict by DISTANCE from the viewport (farthest first), which is optimal for linear scrolling, with LRU only as a tie-breaker.

---------------------------------------------------------------------------------------------------

## Q4. Memory-budget strategies

### Default as a fraction of RAM, MemAvailable, cgroups, hardware change
- Prior-art defaults: darktable mipmap 12.5 % of RAM (clamp 100 MB..8 GB), darktable general processing 50 % of (RAM - 1 GB if < 8 GB) with 512 MB floor [SRC darktable.c];
  digiKam image cache 5 % of RAM clamp 60..200 MB [SRC]; Firefox memory cache = quadratic in log2(RAM), capped at 32 MB (e.g. 1 GB -> 18 MB, 4 GB -> 30 MB, >= 8 GB -> 32 MB)
  with entries > 1/8 of capacity refused [SRC] https://raw.githubusercontent.com/mozilla/gecko-dev/master/netwerk/cache2/CacheObserver.cpp .
- darktable stores FRACTIONS per resource level (val/1024 of total memory) plus fixed reference levels, so a hardware change rescales automatically; XnView's absolute
  setting was reported as not honored (Q1). [SRC/SEARCH] => store "auto + percent", compute absolute at startup, clamp. [INFER]
- `MemAvailable` (kernel estimate "how much memory is available for starting new applications, without swapping"): free memory minus low watermarks, plus file-LRU pages
  minus min(half of the file pages, low watermark), plus reclaimable slab; clamped at 0; formula may change. Page cache (including RustView's own mmap'd pack/LMDB pages)
  therefore counts as available. [SRC] https://raw.githubusercontent.com/torvalds/linux/master/Documentation/filesystems/proc.rst ,
  https://github.com/torvalds/linux/commit/34e431b0ae398fc54ea69ff85ec700722c9da773
- cgroup v2: `memory.max` = hard limit, OOM killer when usage reaches it and cannot be reduced; `memory.high` = throttle + heavy reclaim, never OOM; "max" token = unlimited on read/write;
  page cache AND anonymous memory are charged to the cgroup that instantiated them (file pages stay charged until released); own cgroup path from `/proc/self/cgroup` (`0::$PATH`). [SRC]
  https://raw.githubusercontent.com/torvalds/linux/master/Documentation/admin-guide/cgroup-v2.rst . The Rust `sysinfo` crate delegates to `cgroup::limits_for_system()`
  (I could not open that file), and uses `MemAvailable` with a fallback for kernels < 3.14. [SRC partial] https://raw.githubusercontent.com/GuillaumeGomez/sysinfo/main/src/unix/linux/system.rs
- Proposed formula [INFER]: `effective_total = min(MemTotal, memory.max of own cgroup and ancestors if numeric)`; `budget = min(user_cap, pct * effective_total, max(floor, MemAvailable_now - reserve))`;
  recompute at startup and when the user changes the setting; log the three inputs; treat `memory.high` as a soft ceiling too (KDE session scopes can set it).

### mmap stores vs private heap
- LMDB returns data directly from the map: "no malloc's or memcpy's occur during data fetches"; pointers valid until the end of the txn or the next update; `MDB_NORDAHEAD` "may help
  random read performance when the DB is larger than RAM and system RAM is full". [SRC] https://raw.githubusercontent.com/LMDB/lmdb/mdb.master/libraries/liblmdb/lmdb.h
- Page-cache pages of an mmap are reclaimable (so they do not need to be in the budget) but are charged to the cgroup (above). The pack file read through mmap/page cache is
  already a "compressed RAM cache" managed by the kernel; a second compressed heap cache would duplicate it. [INFER]. memmap2 `advise` docs (Random/WillNeed/DontNeed) I could not open [MEM].

### Compressed vs decoded thumbnails, decode speed
- Size math for 200x200 [INFER, arithmetic]: RGB8 = 120,000 B, RGBA8 = 160,000 B. 8 GiB holds ~72k RGB8 or ~54k RGBA8 tiles; 1M decoded tiles = 120-160 GB. A visible screen (e.g. 9x5 = 45 tiles)
  plus +/-5 pages is only a few hundred to a few thousand tiles (~0.1-0.5 GB). So the decoded L1 rarely needs to be huge; scroll-back is served from the pack file.
- Measured (only primary numbers found): Google `decoder-benchmarks-for-rust`, one 2.6 GHz 6-core i7 MacBook, single image set "venice": libjpeg-turbo (turbojpeg) 100x150 = 0.297 ms,
  200x300 = 0.986 ms, 500x750 = 5.63 ms, 1000x1500 = 22.7 ms; Rust `jpeg-decoder` is 194 % / 167 % / 151 % / 140 % of that time; PNG (`png` crate) 200x300 = 1.24 ms. [SRC]
  https://github.com/google/decoder-benchmarks-for-rust . Interpolating, a 200x200 JPEG is ~0.7 ms per core (~1,400 tiles/s/core) [INFER]; a 45-tile screen is ~30 ms on one core,
  ~4 ms on 8 threads. The bottleneck during a fling is queue flooding and upload, not raw decode throughput. [INFER]
- zune-jpeg: crate page claims 1.5x faster than libjpeg-turbo and 2x faster than jpeg-decoder (self-reported) [SEARCH], but the repo's own benchmark file shows mozjpeg (libjpeg-turbo derived) FASTER
  than zune-jpeg by 1.1-1.34x on baseline/subsampled decodes (28.6 vs 38.0 ms grayscale, 50.0 vs 54.8 ms baseline) and zune faster on progressive. Treat as parity. [SRC]
  https://raw.githubusercontent.com/etemesi254/zune-image/dev/benchmarks/benchmark-results.md . The `image` crate switched JPEG decoding to zune-jpeg in 0.25.0. [SRC] https://raw.githubusercontent.com/image-rs/image/main/CHANGES.md
- libjpeg-turbo can decode at 1/2, 1/4, 1/8 scale inside the IDCT (buffer shrinks by the square); artifacts possible for non-MCU-multiple sizes. [SEARCH] https://bugzilla.mozilla.org/show_bug.cgi?id=2033250
  Not needed if thumbnails are stored at exactly 200 px (RustView) - useful only if stored larger.
- WebP / AVIF / JPEG XL: NO reliable per-thumbnail decode numbers found. One uncontrolled 2026 comparison: decode time ~1.2x JPEG (WebP), ~1.5x (AVIF), ~1.1x (JXL); another: AVIF container overhead
  erases the gain for payloads < ~8 KB. Pure-Rust WebP decoder ~70 % of libwebp speed. [SEARCH] https://lpic.cc/en/blog/webp-avif-jpegxl-format-comparison ,
  https://blog.blazingcdn.com/en-us/webp-vs-avif-vs-heif-choosing-an-image-format-for-delivery , https://docs.rs/crate/zenwebp/0.1.0 . => JPEG (or lossy WebP) is the safe pack format; benchmark on your own thumbs.
- XnView offers WebP as DB compression "so more of the database stays in memory". [SEARCH]
- GPU formats [INFER/MEM]: RGBA8 4 B/px, RGB8 3 B/px, BC1 0.5 B/px (20 KB per 200x200), BC7/ETC2-RGBA 1 B/px. Slint's femtovg path accepts RGB8/RGBA8 buffers without CPU conversion [SRC] but exposes
  no compressed-texture upload API (no such path in the code I read) [INFER], so compressed textures are not usable from Slint without a custom GL underlay.

---------------------------------------------------------------------------------------------------

## Q5. LMDB read patterns and low per-record RAM

### LMDB / heed
- `mdb_get` returns pointers into the map (zero-copy, valid until end of txn or next update); `MDB_NOTLS` ties reader slots to txn objects instead of threads; `mdb_txn_reset` + `mdb_txn_renew` reuse a
  read-txn handle (renew takes a new reader lock); default 126 reader slots; long-lived read txns pin old pages so the DB grows while a writer is active ("avoid long-lived transactions");
  `MDB_APPEND` requires sorted keys (else `MDB_KEYEXIST`); max key 511 B; `MDB_WRITEMAP` uses fewer mallocs but loses protection. [SRC] lmdb.h (URL above)
- heed: `read_txn_without_tls` makes `RoTxn` Send and cursors movable between threads; "LMDB disallows sharing cursors among threads"; value pointers valid until txn end;
  `lazily_decode_data()` to decode only needed entries; `range`/`prefix_iter`; big-endian key encoding for sortable integer keys; opening is `unsafe` (mmap UB if file modified). [SRC]
  https://raw.githubusercontent.com/meilisearch/heed/main/heed/src/cookbook.rs , https://raw.githubusercontent.com/meilisearch/heed/main/heed/src/envs/env_open_options.rs
- Batched lookups [INFER from B+tree layout]: sort the requested ids/keys first and walk with a cursor (`range` / `MDB_SET_RANGE`) so each leaf page is touched once; one reusable NOTLS read txn per worker,
  `reset` between batches; copy the few bytes you need out and drop the txn quickly (writer page reuse). Value alignment is not guaranteed by LMDB [MEM] - decode with unaligned reads/bytemuck `pod_read_unaligned`.
- Throughput reference (third-party Python binding, not the C library): ~4 M random lookups/s and ~6 M sequential reads/s single process. [SEARCH] https://zlmdb.readthedocs.io/en/stable/lmdb/performance.html

### Per-record RAM with millions of records [INFER unless stated]
- Struct-of-arrays: one `Vec<T>` per column (u32 ids, i64 mtime, u64 size, u16 flags, u32 folder_id, interned-string ids). Only the columns used by the active sort/filter are touched; a 1M-row record of
  ~100 B AoS = 100 MB, columnar with 4-5 hot columns is ~20-30 MB. Sort a `Vec<u32>` permutation, never move records.
- Interned strings: `lasso` (`Rodeo` -> `RodeoReader`/`RodeoResolver`; resolver has the lowest memory, README rates it "Least"); keys are small integer `Spur`s [SRC partial] https://github.com/Kixiron/lasso .
  File names can stay in LMDB/pack and be fetched lazily for visible rows only.
- Sorting: `rayon::par_sort_unstable` = pdqsort, in place, partitioning sequential, recursion parallel; `par_sort_by_cached_key` evaluates the key once per element, in parallel, using u8/u16/u32 indices
  when possible; `par_sort` = stable merge sort needing n extra memory. [SRC] https://github.com/rayon-rs/rayon/blob/main/src/slice/mod.rs
  Rust's std sorts (driftsort stable / ipnsort unstable): ~2.4x over the previous stable sort for random u64 at 1e4-1e6 (authors' own benchmark, Zen 3), much more on low-cardinality inputs; smaller gains for Strings. [SRC]
  https://github.com/Voultapher/sort-research-rs/blob/main/writeup/driftsort_introduction/text.md . `radsort` (LSB radix, stable, O(n) extra): docs say "several times faster" than std sorts on large slices,
  worse on short slices / 16-byte keys. [SEARCH] https://docs.rs/radsort . => numeric keys (date, size, rating, duration): radix on extracted u64 keys; string keys: sort_by_cached_key on collation keys.

---------------------------------------------------------------------------------------------------

## Q6. Perception thresholds and measurements

- Apple (current Xcode Organizer doc): "Human perception is very sensitive to interruptions in motion ... even a couple of milliseconds can be noticeable"; hitch rate (ms of pause per second) <= 10 ms/s good,
  <= 25 warning, <= 50 critical, > 50 act now; vsync interval 16.7 ms at 60 Hz, 8.3 ms at 120 Hz; hangs in discrete interaction become noticeable at ~100 ms or more. [SRC]
  https://developer.apple.com/documentation/xcode/understanding-hitches-in-your-app.md . WWDC20 (session 10077/10076) used < 5 ms/s good, 5-10 notice, > 10 poor. [SEARCH] https://developer.apple.com/videos/play/wwdc2020/10077
- RAIL: respond within 100 ms (work in <= 50 ms chunks), animation frame <= 16 ms (browser needs ~6 ms, so ~10 ms for app work), idle work in <= 50 ms chunks, load ~1 s. [SEARCH, secondary summaries of web.dev]
  https://dev.to/rijultp/the-rail-model-making-web-apps-feel-lightning-fast-o7p
- Nielsen/Miller: 0.1 s = feels instantaneous, 1 s = flow of thought kept, 10 s = attention limit. [SEARCH] https://www.nngroup.com/videos/3-response-time-limits-interaction-design/
- Placeholders vs blank: mixed evidence (Q2). No controlled study found that compares blank cells vs placeholders in a THUMBNAIL GRID.
- Studies/benchmarks of thumbnail-grid scrolling: I found none. The closest quantitative artifacts are implementation constants (Dolphin ReadAheadPages 5, Qt cacheBuffer 320, Shotwell 100 ms HQ delay,
  darktable 10 ms scroll batching, Android velocity/deadline logic) and Apple's hitch-rate metric. Scrolling "latency" research I found (Annett et al. 2014: ~50 ms inking vs 2-7 ms) concerns touch/ink, not scrolling. [SEARCH]

---------------------------------------------------------------------------------------------------

## Actionable ideas ranked by expected impact for RustView

1. Never do decode, disk read or texture creation inside the scroll/frame path: draw a fixed-size placeholder instantly, decode on a worker pool, hand finished `SharedPixelBuffer`s to the UI thread, and
   cap UI-thread uploads per frame (e.g. a handful of 160 KB tiles, measured with a counter). Why: every engine above does this; Slint creates one GL texture per assigned `Image` on the UI thread (no atlas).
2. Epoch/generation-based cancellation: every scroll step (or visible-range change) bumps an epoch; queued requests from older epochs are dropped without running, in-flight ones are ignored on completion.
   Why: Dolphin kills and restarts its preview job on each range change, Shotwell/iOS/Android cancel off-screen work; queue flooding, not decode speed, is the fling killer (decode ~0.7 ms/tile/core).
3. Velocity-aware loading: above a speed threshold (e.g. > ~1 screen/200 ms) enqueue nothing but placeholders; when velocity drops (~50-100 ms settled) enqueue the visible set. Why: Picasso pause-on-fling,
   Shotwell 100 ms deferral, Android `neededNextFrame` uses velocity; verified pattern. Show a position label (scrubber) while scrubbing 1M items like Immich/Google Photos.
4. Three-bin priority queue like Chromium: NOW (visible) > SOON (read-ahead in scroll direction, ~1-2 pages) > EVENTUALLY (+/- ~5 pages like Dolphin), then distance to viewport, newest first (LIFO) within a bin.
   Why: Chromium `TilePriority`, Android sort order, KPhotoAlbum; cheap to implement with a binary heap or three deques.
5. Budgeted L1 of DECODED tiles with hard byte cap, pinned visible entries, and distance-from-viewport eviction (farthest first, LRU tie-break). Why: linear scrolling is a sliding window; LRU wastes budget on
   the far past, darktable skips locked entries, quick_cache supports pins. Count only L1 + metadata columns in the user budget; leave the pack-file page cache to the OS.
6. Do not build a second compressed in-RAM cache on top of an mmap'd pack file; let the kernel page cache serve scroll-back (microsecond reads) and use `MDB_NORDAHEAD`/`MADV_RANDOM` for the random access
   pattern. Why: duplicates page cache; saves budget for decoded tiles. (Verify page-fault latency on cold pages with a trace.)
7. Budget resolution at startup: store "auto + percent" (default 50 %) not bytes; `effective_total = min(MemTotal, cgroup memory.max)`; clamp by `MemAvailable - reserve`; log inputs; re-evaluate on settings change.
   Why: handles hardware downgrade and containers/systemd scopes; darktable uses fractions, XnView's absolute setting was reported as unenforced. Note a 50 % default is ~4x darktable's mipmap default.
8. Pick the replacement policy by byte weight and scan resistance: `quick_cache` (S3-FIFO-like, Weighter, pinning) or own distance-based eviction; avoid moka's TinyLFU admission for L1 (can reject the tile
   about to be drawn); simulate on a recorded scroll trace with libCacheSim before committing.
9. Store thumbnails as JPEG (or lossy WebP) at exactly 200x200 in the pack file; decode with libjpeg-turbo or zune-jpeg (parity per its own benchmarks), RGB8 into `SharedPixelBuffer` (Slint passes RGB8
   through unconverted). Why: 10-20x smaller than RGB in RAM/disk [MEM: typical q80 200x200 JPEG ~6-12 KB]; ~0.7 ms/decode/core measured class. Skip AVIF/JXL (no thumbnail-scale advantage found).
10. Targeted model updates only: keep a fixed-size row model for the visible window and call `set_row_data` for rows inside it; avoid `reset`, `insert`, `remove` during scroll (they dirty all trailing instances,
    reset drops all). Reuse the same `Image` clone for grid and table only if one instance suffices; remember each item instance uploads its own texture. Why: verified repeater behaviour.
11. Guard DPI/scale-factor changes and theme changes: they clear femtovg's graphics cache and re-upload every visible tile in one frame; spread re-uploads over frames (budget from idea 1).
12. LMDB access discipline: one `read_txn_without_tls` environment, a reusable read txn per worker with reset/renew per batch, sort batched ids and walk with a cursor, copy out and drop txns before long
    work; never hold a read txn across UI frames while the writer is active (DB growth, stale pages).
13. Library metadata as columns: SoA vectors with u32 ids, interned strings (lasso resolver or LMDB-resident names fetched only for visible rows), sort a `Vec<u32>` permutation with
    `par_sort_unstable_by_key`/radix on numeric keys, filter into bitsets. Why: leaves most of the 50 % budget for L1 and keeps sort/filter on 1M rows in tens of ms (estimate, measure).
14. Stand-in strategy: if the 200 px tile is not ready show the previous/neighbouring size or a flat dominant-color/blurhash-like swatch stored in the library DB (no media access); reserve exact tile size to
    avoid layout shift. Why: darktable stand-in, Shotwell low-quality-first, Chromium checkerboard; placeholder benefit is mainly layout stability (evidence mixed).
15. Instrument like Apple's hitch metric: log frame-time histogram and "ms of hitch per second of scrolling" (target <= 5-10 ms/s), uploads/frame, queue depth, epoch cancels, L1 hit rate, in debug logs with the
    project's `[Tag]` convention. Why: no published thumbnail-grid benchmarks exist, so RustView must produce its own evidence.
