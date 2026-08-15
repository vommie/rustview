use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::{FolderNodeView, MainWindow};
use crate::library::Library;
use crate::library::models::FolderId;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{ModelRc, VecModel};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Subsystem managing filesystem folder hierarchy discovery and sidebar synchronization.
pub struct BridgeFolders;

impl BridgeFolders {
    /// Synchronizes the root folder from settings, updates database records, and restores last state.
    pub fn sync_root(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let s = settings.get();
        window.set_thumb_size(s.appearance.default_thumbnail_size as i32);

        let Some(ref root_path) = s.general.root_folder else {
            Self::clear_view(
                window,
                state,
                "No Root Directory configured. Open Settings to select a root folder.",
            );
            return;
        };

        if !root_path.is_dir() {
            Self::clear_view(
                window,
                state,
                "Configured Root Directory does not exist on disk.",
            );
            return;
        }

        tracing::info!(root = ?root_path, "Synchronizing root directory...");

        let folder_record = match library.folders().get_folder_by_path(root_path) {
            Ok(Some(f)) => f,
            _ => match library.folders().add_folder(root_path) {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!(error = %e, "Failed to register root directory");
                    return;
                }
            },
        };

        Self::scan_subdirectories(library, root_path);

        let target_folder = if s.general.remember_last_folder
            && let Some(ref last_path) = s.general.last_folder_path
            && last_path.is_dir()
            && last_path.starts_with(root_path)
            && let Ok(Some(f)) = library.folders().get_folder_by_path(last_path)
        {
            (f.id, last_path.clone())
        } else {
            (folder_record.id, root_path.clone())
        };

        let _ = library.refresher().refresh_folder(target_folder.0);

        let raw_media = library
            .media()
            .get_by_folder(target_folder.0)
            .unwrap_or_default();
        let indexed_media = BridgeMedia::preindex(library, raw_media, &target_folder.1);

        {
            let mut st = state.write();
            st.current_folder_id = Some(target_folder.0);
            st.current_folder_path = Some(target_folder.1);
            st.all_folder_media = indexed_media;
            st.folder_thumbnails.clear();
            st.active_indices.clear();
            st.file_selection.clear();
            st.last_executed_query.clear();
        }

        window.set_status_text("Ready.".into());

        Self::reload_tree(window, library, settings, state);
        BridgeMedia::reload(window, library, thumbnails, state, true, false);
        BridgeInspector::update(window, state);
    }

    /// Shallow scan of direct child directories for registration in LMDB.
    pub fn scan_subdirectories(library: &Library, root_path: &Path) {
        if let Ok(entries) = std::fs::read_dir(root_path) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type()
                    && file_type.is_dir()
                {
                    let sub_path = entry.path();
                    if library
                        .folders()
                        .get_folder_by_path(&sub_path)
                        .unwrap_or(None)
                        .is_none()
                    {
                        let _ = library.folders().add_folder(&sub_path);
                    }
                }
            }
        }
    }

    /// Handles explicit folder selection by the user.
    pub fn select_folder(
        folder_id: FolderId,
        path: PathBuf,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        tracing::info!(folder_id = folder_id, path = ?path, "User selected folder in sidebar");

        match library.refresher().refresh_folder(folder_id) {
            Ok(report) => {
                tracing::info!(
                    folder_id = folder_id,
                    scanned = report.total_scanned,
                    new = report.new_media,
                    updated = report.updated_media,
                    unchanged = report.unchanged_media,
                    deleted = report.deleted_media,
                    "Folder scan complete"
                );
            }
            Err(e) => {
                tracing::error!(folder_id = folder_id, error = %e, "Failed to scan folder");
            }
        }

        Self::scan_subdirectories(library, &path);

        let raw_media = library.media().get_by_folder(folder_id).unwrap_or_default();
        let indexed_media = BridgeMedia::preindex(library, raw_media, &path);

        tracing::info!(
            folder_id = folder_id,
            count = indexed_media.len(),
            "Cached preformatted folder media records into RAM."
        );

        {
            let mut st = state.write();
            st.current_folder_id = Some(folder_id);
            st.current_folder_path = Some(path.clone());
            st.all_folder_media = indexed_media;
            st.folder_thumbnails.clear();
            st.active_indices.clear();
            st.file_selection.clear();
            st.last_executed_query.clear();
        }

        {
            let mut s = settings.get();
            if s.general.remember_last_folder {
                s.general.last_folder_path = Some(path);
                settings.save_quiet(&s);
            }
        }

        Self::reload_tree(window, library, settings, state);
        BridgeMedia::reload(window, library, thumbnails, state, true, true);
        BridgeInspector::update(window, state);
    }

    /// Reconstructs the folder hierarchy tree model for the Slint left sidebar.
    pub fn reload_tree(
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let root_setting = settings.get().general.root_folder;
        if let Some(ref root_dir) = root_setting {
            let folders = library.folders().list_folders().unwrap_or_default();
            let current_sel = state.read().current_folder_id;

            let mut matching_folders: Vec<_> = folders
                .into_iter()
                .filter(|f| Path::new(&f.path).starts_with(root_dir))
                .collect();

            matching_folders.sort_by(|a, b| a.path.cmp(&b.path));

            let folder_views: Vec<FolderNodeView> = matching_folders
                .into_iter()
                .map(|f| {
                    let p = Path::new(&f.path);
                    let depth = p
                        .strip_prefix(root_dir)
                        .map(|rel| rel.components().count())
                        .unwrap_or(0);

                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| f.path.clone());

                    FolderNodeView {
                        id: f.id as i32,
                        name: name.into(),
                        path: f.path.into(),
                        depth: depth as i32,
                        is_expanded: true,
                        is_selected: current_sel == Some(f.id),
                    }
                })
                .collect();
            window.set_folders(ModelRc::new(VecModel::from(folder_views)));
        } else {
            window.set_folders(ModelRc::new(VecModel::from(vec![])));
        }
    }

    /// Clears active folder data from state and UI when no root directory is set.
    pub fn clear_view(window: &MainWindow, state: &Arc<RwLock<UiState>>, status_msg: &str) {
        {
            let mut st = state.write();
            st.current_folder_id = None;
            st.current_folder_path = None;
            st.all_folder_media.clear();
            st.folder_thumbnails.clear();
            st.active_indices.clear();
            st.file_selection.clear();
            st.media_row_map.clear();
            st.last_executed_query.clear();
        }
        window.set_folders(ModelRc::new(VecModel::from(vec![])));
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_items_metric_text("Items: 0".into());
        window.set_status_text(status_msg.into());
        BridgeInspector::update(window, state);
    }
}
