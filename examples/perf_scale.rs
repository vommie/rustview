//! Scale test of the in-memory record list (scaffolding of the performance work): builds N synthetic library records the way the
//! application does and reports the resident memory per record plus the cost of the operations that run over the whole list.
//!
//! Usage: `cargo run --release --example perf_scale -- [count]...` (default: 100000 1000000). Thumbnails are not part of this test:
//! they live in the byte-budgeted `ThumbnailRamCache` and do not grow with the folder size.

#[path = "../benches/support/mod.rs"]
mod support;

use memchr::memmem;
use rustview::query::filters::{IndexedMediaRecord, MediaTypeFilter, PreparedFilter, QueryFilter};
use rustview::query::{MediaSorter, MediaStore, SortColumn, SortOrder};
use rustview::thumbnails::ram_cache::IdBuildHasher;
use std::collections::HashMap;
use std::time::Instant;
use support::synthetic_record;

fn rss_mb() -> f64 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| s.split_whitespace().nth(1).and_then(|v| v.parse::<f64>().ok()))
        .map_or(0.0, |pages| pages * 4096.0 / (1024.0 * 1024.0))
}

fn timed<T>(label: &str, f: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let value = f();
    println!("    {label:<34} {:>9.1} ms", start.elapsed().as_secs_f64() * 1000.0);
    value
}

fn run(count: usize) {
    println!("N = {count}");
    let rss0 = rss_mb();
    let records: Vec<IndexedMediaRecord> = timed("build records", || (0..count).map(synthetic_record).collect());
    let rss1 = rss_mb();
    println!(
        "    resident memory                     {:>9.1} MB  ({:.0} bytes per record)",
        rss1 - rss0,
        (rss1 - rss0) * 1024.0 * 1024.0 / count as f64
    );

    let mut store = MediaStore::new();
    timed("MediaStore::replace (id index)", || store.replace(records));
    println!("    resident memory incl. index         {:>9.1} MB", rss_mb() - rss0);

    let map = timed("row map (id -> position)", || {
        let mut map: HashMap<u64, usize, IdBuildHasher> = HashMap::with_capacity_and_hasher(store.len(), IdBuildHasher::default());
        for (position, item) in store.iter().enumerate() {
            map.insert(item.record.id, position);
        }
        map
    });
    std::hint::black_box(&map);

    let mut refs: Vec<&IndexedMediaRecord> = store.iter().collect();
    for (label, column, order) in [
        ("sort by filename", SortColumn::Filename, SortOrder::Ascending),
        ("sort by modified date, descending", SortColumn::ModifiedDate, SortOrder::Descending),
        ("sort by file size", SortColumn::FileSize, SortOrder::Ascending),
    ] {
        timed(label, || MediaSorter::sort_indexed_refs(&mut refs, column, order));
    }

    let filter = QueryFilter {
        search_query: "holiday_photo_0007".to_string(),
        media_type_filter: MediaTypeFilter::All,
        ..QueryFilter::default()
    };
    let prepared = PreparedFilter::from_filter(&filter);
    let finder = memmem::Finder::new(prepared.query_lower.as_bytes());
    let matched = timed("filter by text (every record)", || {
        store.iter().filter(|item| prepared.matches_indexed(item, &[], Some(&finder))).count()
    });
    println!("    matches                             {matched:>9}");
    println!("    resident memory at the end          {:>9.1} MB\n", rss_mb() - rss0);
}

fn main() {
    let counts: Vec<usize> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let counts = if counts.is_empty() { vec![100_000, 1_000_000] } else { counts };
    for count in counts {
        run(count);
    }
}
