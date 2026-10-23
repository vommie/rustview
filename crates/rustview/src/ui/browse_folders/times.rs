//! The dates of folders (created, modified) for the table of the Browse view and the Inspector.
//!
//! The library does not store them and the rows of the view never touch the drive, so a worker thread reads them: a row that has no
//! date yet asks for it (a channel send), the worker stats the folders in batches and tells the UI thread, which fills the dates of
//! the tiles that are showing. A date stays cached; every reload of the list marks the cache stale, and a stale date is shown until
//! the fresh one has arrived.

use parking_lot::Mutex;
use rustview_core::library::models::FolderId;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

/// Created and modified time of a folder in unix seconds; `None` where the file system has none (or the folder is gone).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderStamp {
    pub created: Option<i64>,
    pub modified: Option<i64>,
}

impl FolderStamp {
    /// Reads the times from the drive (blocking; the worker and explicit user actions only).
    pub fn read(path: &Path) -> Self {
        let Ok(metadata) = std::fs::metadata(path) else {
            return Self::default();
        };
        let secs = |time: std::io::Result<SystemTime>| {
            time.ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
        };
        Self {
            created: secs(metadata.created()),
            modified: secs(metadata.modified()),
        }
    }
}

struct Entry {
    stamp: FolderStamp,
    generation: u64,
}

type Notifier = Box<dyn Fn() + Send + Sync>;

pub struct FolderTimes {
    entries: Mutex<HashMap<FolderId, Entry>>,
    generation: AtomicU64,
    in_flight: Mutex<HashSet<FolderId>>,
    sender: Mutex<Option<Sender<(FolderId, PathBuf)>>>,
    notifier: OnceLock<Notifier>,
}

impl Default for FolderTimes {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            in_flight: Mutex::new(HashSet::new()),
            sender: Mutex::new(None),
            notifier: OnceLock::new(),
        }
    }
}

impl FolderTimes {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// What the worker calls (once per batch) when new dates are in: the UI thread then fills them into the tiles.
    pub fn set_notifier(&self, notifier: Notifier) {
        let _ = self.notifier.set(notifier);
    }

    /// The list was reloaded: every date is shown until a fresh one replaces it.
    pub fn mark_stale(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// The cached date, without asking for a fresh one.
    pub fn peek(&self, id: FolderId) -> Option<FolderStamp> {
        self.entries.lock().get(&id).map(|entry| entry.stamp)
    }

    /// The date of a folder for a row that is being built: the cached one (stale ones too); a missing or stale date is requested.
    pub fn get(self: &Arc<Self>, id: FolderId, path: &Path) -> Option<FolderStamp> {
        let generation = self.generation.load(Ordering::Relaxed);
        let cached = self
            .entries
            .lock()
            .get(&id)
            .map(|entry| (entry.stamp, entry.generation));
        if cached.is_none_or(|(_, entry_generation)| entry_generation != generation) {
            self.request(id, path);
        }
        cached.map(|(stamp, _)| stamp)
    }

    fn request(self: &Arc<Self>, id: FolderId, path: &Path) {
        if !self.in_flight.lock().insert(id) {
            return;
        }
        let mut sender = self.sender.lock();
        let tx = sender.get_or_insert_with(|| self.spawn_worker());
        if tx.send((id, path.to_path_buf())).is_err() {
            self.in_flight.lock().remove(&id);
            *sender = None;
        }
    }

    fn spawn_worker(self: &Arc<Self>) -> Sender<(FolderId, PathBuf)> {
        let (tx, rx) = mpsc::channel::<(FolderId, PathBuf)>();
        let this = Arc::clone(self);
        let spawned = std::thread::Builder::new().name("folder-times".into()).spawn(move || {
            while let Ok(first) = rx.recv() {
                let mut batch = vec![first];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                    if batch.len() >= 512 {
                        break;
                    }
                }
                let generation = this.generation.load(Ordering::Relaxed);
                for (id, path) in batch {
                    let stamp = FolderStamp::read(&path);
                    this.entries.lock().insert(id, Entry { stamp, generation });
                    this.in_flight.lock().remove(&id);
                }
                if let Some(notify) = this.notifier.get() {
                    notify();
                }
            }
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[FolderTimes: Worker] The date reader thread could not be started; folder dates stay empty");
        }
        tx
    }
}
