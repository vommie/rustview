//! Micro benchmarks of the data structures behind the Browse view: thumbnail caches, record lookups, tile windowing, row
//! building and sorting.
//!
//! Run: `cargo bench --bench browse_data -- <filter>`. The sizes mirror the application: a 200 px thumbnail is 120 KB of RGB8,
//! a grid window is ~100 tiles, folders reach 100k records and more. The `legacy_*` groups are verbatim copies of the
//! structures that the ring/budget redesign replaced, kept here so that one run shows before and after.

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use rustview::query::filters::IndexedMediaRecord;
use rustview::query::{MediaSorter, MediaStore, SortColumn, SortOrder};
use rustview::thumbnails::{ThumbnailFormat, ThumbnailGenerator, ThumbnailRamCache};
use rustview::ui::MediaItemView;
use rustview::ui::browse_window::{RecordLabels, RingWindow, ViewGeometry, build_view};
use slint::{Image, Rgb8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, VecDeque};
use std::hint::black_box;
use std::sync::Arc;

mod support;
use support::synthetic_record;

const THUMB: u32 = 200;
const SLICE: usize = 120;

fn pixels() -> SharedPixelBuffer<Rgb8Pixel> {
    SharedPixelBuffer::<Rgb8Pixel>::new(THUMB, THUMB)
}

/// The UI-side store before the redesign: count-capped LRU whose recency list is a `VecDeque` scanned with `retain`.
struct LegacyLruStore {
    capacity: usize,
    map: HashMap<u64, SharedPixelBuffer<Rgb8Pixel>>,
    order: VecDeque<u64>,
}

impl LegacyLruStore {
    fn new(capacity: usize) -> Self {
        Self { capacity, map: HashMap::new(), order: VecDeque::new() }
    }

    fn insert(&mut self, id: u64, buffer: SharedPixelBuffer<Rgb8Pixel>) {
        if self.map.len() >= self.capacity
            && !self.map.contains_key(&id)
            && let Some(evicted) = self.order.pop_front()
        {
            self.map.remove(&evicted);
        }
        self.map.insert(id, buffer);
        self.order.retain(|&i| i != id);
        self.order.push_back(id);
    }

    fn get(&self, id: &u64) -> Option<&SharedPixelBuffer<Rgb8Pixel>> {
        self.map.get(id)
    }
}

/// The scheduler-side L1 cache before the redesign: stores raw bytes, every hit moves the id in an O(capacity) list under a
/// write lock and the caller copies the 120 KB into a fresh buffer.
struct LegacyL1 {
    capacity: usize,
    inner: parking_lot::RwLock<(HashMap<u64, Arc<Vec<u8>>>, VecDeque<u64>)>,
}

impl LegacyL1 {
    fn new(capacity: usize) -> Self {
        Self { capacity, inner: parking_lot::RwLock::new((HashMap::new(), VecDeque::new())) }
    }

    fn get(&self, id: u64) -> Option<Arc<Vec<u8>>> {
        let mut state = self.inner.write();
        let item = state.0.get(&id).cloned()?;
        state.1.retain(|&i| i != id);
        state.1.push_back(id);
        Some(item)
    }

    fn insert(&self, id: u64, rgb: &[u8]) {
        let item = Arc::new(rgb.to_vec());
        let mut state = self.inner.write();
        if state.0.len() >= self.capacity
            && !state.0.contains_key(&id)
            && let Some(evicted) = state.1.pop_front()
        {
            state.0.remove(&evicted);
        }
        state.0.insert(id, item);
        state.1.retain(|&i| i != id);
        state.1.push_back(id);
    }
}

/// The row builder before the record compaction: the formatted strings were stored per record and only cloned into the row.
fn legacy_view_of(item: &IndexedMediaRecord, labels: &RecordLabels, thumbnail: Image) -> MediaItemView {
    MediaItemView {
        id: item.record.id as i32,
        index: item.index as i32,
        filename: labels.filename.clone(),
        path_str: item.folder.path_str().clone(),
        modified_date_str: labels.modified_date.clone(),
        created_date_str: labels.created_date.clone(),
        filesize_str: labels.filesize.clone(),
        rating: i32::from(item.record.rating),
        rating_str: labels.rating.clone(),
        tags_count: item.tags_count as i32,
        tags_str: labels.tags.clone(),
        width_str: labels.width.clone(),
        height_str: labels.height.clone(),
        ar_str: labels.aspect_ratio.clone(),
        duration_str: labels.duration.clone(),
        duration_badge_str: labels.duration_badge.clone(),
        format_str: labels.format.clone(),
        flags_str: labels.flags.clone(),
        has_tags: item.has_tags,
        thumbnail,
        is_selected: false,
        is_video: false,
        is_corrupted: item.is_corrupted,
        is_deleted: item.is_deleted,
        is_pending: item.is_pending,
    }
}

/// A window that moves by one grid row (8 items) per iteration over a folder of `total` thumbnails.
fn bench_ui_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("legacy_ui_store(LruThumbnailStore, cap 2048)");
    let total = 4096u64;
    let mut store = LegacyLruStore::new(2048);
    for id in 1..=total {
        store.insert(id, pixels());
    }
    group.bench_function("slice_of_120_hits", |b| {
        let mut start = 1u64;
        b.iter(|| {
            start = if start + 8 + SLICE as u64 > total { 1 } else { start + 8 };
            for id in start..start + SLICE as u64 {
                black_box(store.get(&id));
            }
        });
    });
    group.bench_function("insert_with_eviction", |b| {
        let mut next = total + 1;
        b.iter(|| {
            store.insert(next, pixels());
            next += 1;
        });
    });
    group.finish();
}

fn bench_l1(c: &mut Criterion) {
    let mut group = c.benchmark_group("legacy_l1_cache(L1ThumbnailCache, cap 512)");
    let cache = LegacyL1::new(512);
    let raw = vec![0u8; (THUMB * THUMB * 3) as usize];
    for id in 1..=512u64 {
        cache.insert(id, &raw);
    }
    group.bench_function("get_hit", |b| {
        let mut id = 0u64;
        b.iter(|| {
            id = id % 512 + 1;
            black_box(cache.get(id));
        });
    });
    group.bench_function("get_hit_and_copy_into_shared_buffer", |b| {
        let mut id = 0u64;
        b.iter(|| {
            id = id % 512 + 1;
            if let Some(hit) = cache.get(id) {
                let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(THUMB, THUMB);
                buffer.make_mut_bytes().copy_from_slice(&hit);
                black_box(buffer);
            }
        });
    });
    group.bench_function("insert_with_eviction", |b| {
        let mut id = 513u64;
        b.iter(|| {
            cache.insert(id, &raw);
            id += 1;
        });
    });
    group.finish();
}

fn bench_ram_cache(c: &mut Criterion) {
    let mut group = c.benchmark_group("ram_cache(ThumbnailRamCache, 4096 tiles)");
    let budget = 4096 * (THUMB * THUMB * 3) as usize;
    let cache = ThumbnailRamCache::new(budget);
    let total = 4096u64;
    for id in 1..=total {
        cache.insert(id, pixels());
    }
    group.bench_function("slice_of_120_hits", |b| {
        let mut start = 1u64;
        b.iter(|| {
            start = if start + 8 + SLICE as u64 > total { 1 } else { start + 8 };
            for id in start..start + SLICE as u64 {
                black_box(cache.get(id));
            }
        });
    });
    group.bench_function("contains", |b| {
        let mut id = 0u64;
        b.iter(|| {
            id = id % total + 1;
            black_box(cache.contains(id));
        });
    });
    group.bench_function("insert_with_eviction", |b| {
        let mut next = total + 1;
        b.iter(|| {
            cache.insert(next, pixels());
            next += 1;
        });
    });
    group.finish();
}

fn bench_media_store(c: &mut Criterion) {
    let mut group = c.benchmark_group("record_lookup_by_id");
    for &n in &[1_000usize, 100_000] {
        let records: Vec<IndexedMediaRecord> = (0..n).map(synthetic_record).collect();
        let wanted = n as u64 - 7;
        group.bench_with_input(BenchmarkId::new("legacy_linear_scan", n), &records, |b, records| {
            b.iter(|| black_box(records.iter().find(|m| m.record.id == wanted)));
        });
        let mut store = MediaStore::new();
        store.replace(records);
        group.bench_with_input(BenchmarkId::new("media_store", n), &store, |b, store| {
            b.iter(|| black_box(store.find(wanted)));
        });
    }
    group.finish();
}

fn bench_ring(c: &mut Criterion) {
    let geometry = ViewGeometry::new(8, 6, 3);
    let mut group = c.benchmark_group("tile_window");
    group.bench_function("advance_one_row", |b| {
        let mut ring = RingWindow::new(geometry.pool_size(1_000_000));
        let mut row = 3usize;
        ring.advance(geometry.window(row, 1_000_000));
        b.iter(|| {
            row += 1;
            black_box(ring.advance(geometry.window(row, 1_000_000)))
        });
    });
    group.finish();
}

fn bench_slice_build(c: &mut Criterion) {
    let records: Vec<IndexedMediaRecord> = (0..1000).map(synthetic_record).collect();
    let stored_strings: Vec<RecordLabels> = records.iter().map(RecordLabels::of).collect();
    let shared = pixels();
    c.bench_function("legacy_slice/build_120_rows(stored strings)", |b| {
        b.iter(|| {
            let views: Vec<MediaItemView> = (200..200 + SLICE)
                .map(|i| legacy_view_of(&records[i], &stored_strings[i], Image::from_rgb8(shared.clone())))
                .collect();
            black_box(views)
        });
    });
    c.bench_function("slice/build_120_rows(derived labels)", |b| {
        b.iter(|| {
            let views: Vec<MediaItemView> = records[200..200 + SLICE]
                .iter()
                .enumerate()
                .map(|(i, item)| build_view(item, 200 + i, false, Image::from_rgb8(shared.clone())))
                .collect();
            black_box(views)
        });
    });
    c.bench_function("legacy_slice/build_one_row(stored strings)", |b| {
        b.iter(|| black_box(legacy_view_of(&records[300], &stored_strings[300], Image::from_rgb8(shared.clone()))));
    });
    c.bench_function("slice/build_one_row(derived labels)", |b| {
        b.iter(|| black_box(build_view(&records[300], 300, false, Image::from_rgb8(shared.clone()))));
    });
}

/// A 200 px thumbnail the way the library stores it (lossy WebP, ~8 KB for detailed content).
fn stored_thumbnail() -> Vec<u8> {
    let (width, height) = (200u32, 200u32);
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let noise = (state & 0x1f) as u32;
            rgb.extend_from_slice(&[((x * 255 / width) + noise) as u8, ((y * 255 / height) + noise) as u8, (((x + y) / 2) + noise) as u8]);
        }
    }
    ThumbnailGenerator::new().compress(&rgb, width, height, ThumbnailFormat::Webp, 80).expect("compress")
}

fn bench_thumbnail_decode(c: &mut Criterion) {
    let stored = stored_thumbnail();
    let generator = ThumbnailGenerator::new();
    let mut group = c.benchmark_group("thumbnail_decode_200px");
    group.bench_function("legacy_image_crate(image-webp)", |b| {
        b.iter(|| {
            let img = image::load_from_memory_with_format(black_box(&stored), image::ImageFormat::WebP).expect("decode");
            black_box(img.into_rgb8().into_raw())
        });
    });
    group.bench_function("libwebp(ThumbnailGenerator::decompress)", |b| {
        b.iter(|| black_box(generator.decompress(black_box(&stored)).expect("decode")));
    });
    group.finish();
}

fn bench_sort(c: &mut Criterion) {
    let mut group = c.benchmark_group("sort_filename");
    group.sample_size(10);
    for &n in &[1_000usize, 100_000] {
        let records: Vec<IndexedMediaRecord> = (0..n).map(synthetic_record).collect();
        group.bench_with_input(BenchmarkId::from_parameter(n), &records, |b, records| {
            b.iter_batched(
                || records.iter().collect::<Vec<_>>(),
                |mut refs| {
                    MediaSorter::sort_indexed_refs(&mut refs, SortColumn::Filename, SortOrder::Ascending);
                    black_box(refs)
                },
                BatchSize::LargeInput,
            );
        });
    }
    group.finish();
}

criterion_group!(benches, bench_ui_store, bench_l1, bench_ram_cache, bench_media_store, bench_ring, bench_slice_build, bench_thumbnail_decode, bench_sort);
criterion_main!(benches);
