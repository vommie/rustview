pub mod dnd_autoscroller;
pub mod move_sync;
pub mod projection;
pub mod tree_sync;

use crate::ui::state::UiState;
use crate::ui::MainWindow;
use rustview_core::library::models::{FolderId, MediaRecord};
use rustview_core::library::Library;
use rustview_core::query::IndexedMediaRecord;
use rustview_core::settings::SettingsManager;
use rustview_core::telemetry::FolderLoadBenchmark;
use rustview_core::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

mod callbacks;
mod snapshot;
mod refresh;
mod selection;
mod tree_state;

pub struct FolderNavigationContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
}

/// What a folder selection carries from the click to the moment its list is shown.
struct FolderSelection {
    folder_id: FolderId,
    normalized_path: PathBuf,
    folder_name: String,
    is_virtual_folder: bool,
    virtual_count: usize,
    target_folder_ids: Vec<FolderId>,
    cancel_flag: Arc<AtomicBool>,
    bench: Arc<FolderLoadBenchmark>,
    is_explicit_refresh: bool,
    request_token: usize,
}

struct LoadedFolder {
    indexed: Vec<IndexedMediaRecord>,
    is_empty_in_db: bool,
    /// The records tagged with an external source application: the only ones the external stub completion has to look at.
    stub_candidates: Vec<MediaRecord>,
}

pub struct BridgeFolders;

impl BridgeFolders {

}
