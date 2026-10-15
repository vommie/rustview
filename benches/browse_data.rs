//! Micro benchmarks of the data structures behind the Browse view: thumbnail caches, record lookups, tile windowing, row
//! building and sorting.
//!
//! Run: `cargo bench --bench browse_data -- <filter>`. The sizes mirror the application: a 200 px thumbnail is 120 KB of RGB8,
//! a grid window is ~100 tiles, folders reach 100k records and more. The `legacy_*` groups are verbatim copies of the
//! structures that the ring/budget redesign replaced, kept here so that one run shows before and after.

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use rustview::library::models::MediaRecord;
use rustview::media::{MediaFormat, MediaType};
use rustview::query::filters::IndexedMediaRecord;
use rustview::query::{MediaSorter, MediaStore, SortColumn, SortOrder};
use rustview::thumbnails::ThumbnailRamCache;
use rustview::ui::MediaItemView;
use rustview::ui::browse_window::{RingWindow, ViewGeometry, build_view};
use slint::{Image, Rgb8Pixel, SharedPixelBuffer, SharedString};
use std::collections::{HashMap, VecDeque};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::Arc;

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

fn synthetic_record(i: usize) -> IndexedMediaRecord {
    let filename = format!("IMG_{:07}_holiday_photo.webp", (i * 7919) % 10_000_000);
    let record = MediaRecord {
        id: i as u64 + 1,
        folder_id: 1,
        filename: filename.clone(),
        file_hash: [0u8; 16],
        file_size: 10_000 + (i as u64 * 31) % 5_000_000,
        modification_time: 1_600_000_000 + i as i64 * 17,
        media_type: MediaType::Image,
        format: MediaFormat::Webp,
        width: 300,
        height: 200,
        rating: (i % 6) as u8,
        last_seen: 0,
        is_deleted: false,
        creation_time: 1_600_000_000 + i as i64 * 13,
        duration_ms: 0,
        is_corrupted: false,
        source: None,
    };
    IndexedMediaRecord {
        index: i,
        full_path: PathBuf::from("/media/library/photos/2024").join(&filename),
        folder_path_str: SharedString::from("/media/library/photos/2024"),
        folder_path_lower: "/media/library/photos/2024".to_string(),
        filename_lower: filename.to_lowercase(),
        filename_slint: SharedString::from(filename.as_str()),
        modified_date_str: SharedString::from("2024-05-17 12:34"),
        created_date_str: SharedString::from("2024-05-17 12:30"),
        filesize_mb_str: SharedString::from("1.23 MB"),
        rating_str: SharedString::from("3"),
        tags_count: 0,
        tags_count_str: SharedString::from("0"),
        width_str: SharedString::from("300"),
        height_str: SharedString::from("200"),
        ar_str: SharedString::from("3:2"),
        ar_value: 1.5,
        duration_str: SharedString::default(),
        duration_badge_str: SharedString::default(),
        format_str: SharedString::from("webp"),
        flags_str: SharedString::from("OK"),
        is_corrupted: false,
        is_deleted: false,
        is_pending: false,
        has_tags: false,
        record,
    }
}

fn view_of(item: &IndexedMediaRecord, thumbnail: Image) -> MediaItemView {
    MediaItemView {
        id: item.record.id as i32,
        index: item.index as i32,
        filename: item.filename_slint.clone(),
        path_str: item.folder_path_str.clone(),
        modified_date_str: item.modified_date_str.clone(),
        created_date_str: item.created_date_str.clone(),
        filesize_str: item.filesize_mb_str.clone(),
        rating: item.record.rating as i32,
        rating_str: item.rating_str.clone(),
        tags_count: item.tags_count as i32,
        tags_str: item.tags_count_str.clone(),
        width_str: item.width_str.clone(),
        height_str: item.height_str.clone(),
        ar_str: item.ar_str.clone(),
        duration_str: item.duration_str.clone(),
        duration_badge_str: item.duration_badge_str.clone(),
        format_str: item.format_str.clone(),
        flags_str: item.flags_str.clone(),
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
    let shared = pixels();
    c.bench_function("slice/build_120_item_views", |b| {
        b.iter(|| {
            let views: Vec<MediaItemView> = records[200..200 + SLICE]
                .iter()
                .map(|item| view_of(item, Image::from_rgb8(shared.clone())))
                .collect();
            black_box(views)
        });
    });
    c.bench_function("slice/build_one_item_view", |b| {
        b.iter(|| black_box(view_of(&records[300], Image::from_rgb8(shared.clone()))));
    });
    c.bench_function("slice/build_view_one_row(browse_window)", |b| {
        b.iter(|| black_box(build_view(&records[300], 300, false, Image::from_rgb8(shared.clone()))));
    });
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

criterion_group!(benches, bench_ui_store, bench_l1, bench_ram_cache, bench_media_store, bench_ring, bench_slice_build, bench_sort);
criterion_main!(benches);
