//! Keeps the O(N) work of building the Browse list (load, filter, sort, row map) off the UI thread for large lists.
//!
//! Lists up to `ASYNC_ABOVE_RECORDS` records keep the synchronous path: same behavior, no intermediate state. Above it a worker thread
//! computes against a snapshot of the records and the UI thread only swaps the finished result in (O(tile pool)).
//!
//! Two kinds of jobs exist, each with a sequence number; a result is applied only while its number is still the latest of its kind:
//! - a *relayout* (new query, filter or sort order of the records that are loaded) is superseded by a newer relayout, by a folder load
//!   and by any synchronous reload (which is always the newest truth);
//! - a *folder load* is superseded only by a newer folder load, never by a reload: its result replaces the whole list anyway.

mod worker;

pub use worker::LatestJobWorker;

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

/// Lists with more records than this are built on the worker thread (about 40 ms of work at ~2 microseconds per record).
pub const ASYNC_ABOVE_RECORDS: usize = 20_000;

static LATEST_RELAYOUT: AtomicU64 = AtomicU64::new(0);
static LATEST_FOLDER_LOAD: AtomicU64 = AtomicU64::new(0);
static WORKER: OnceLock<LatestJobWorker> = OnceLock::new();

/// Starts a relayout job; every older relayout is superseded.
pub fn begin_relayout() -> u64 {
    LATEST_RELAYOUT.fetch_add(1, Ordering::AcqRel) + 1
}

pub fn relayout_is_current(job: u64) -> bool {
    LATEST_RELAYOUT.load(Ordering::Acquire) == job
}

/// A synchronous reload just produced the newest list: relayouts that are still running must not overwrite it.
pub fn supersede_relayouts() {
    begin_relayout();
}

/// Starts a folder load; older folder loads and all pending relayouts are superseded.
pub fn begin_folder_load() -> u64 {
    supersede_relayouts();
    LATEST_FOLDER_LOAD.fetch_add(1, Ordering::AcqRel) + 1
}

pub fn folder_load_is_current(job: u64) -> bool {
    LATEST_FOLDER_LOAD.load(Ordering::Acquire) == job
}

pub fn worker() -> &'static LatestJobWorker {
    WORKER.get_or_init(|| LatestJobWorker::spawn("list-loader"))
}

/// Ends the worker thread (application shutdown).
pub fn shutdown() {
    if let Some(worker) = WORKER.get() {
        worker.shutdown();
    }
}
