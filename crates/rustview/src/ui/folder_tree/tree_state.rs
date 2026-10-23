//! The expansion state of the tree and what happens to the view when a folder disappears.

use super::BridgeFolders;
use crate::icons::Icon;
use crate::ui::MainWindow;
use crate::ui::favorites::BridgeFavorites;
use crate::ui::folder_tree::tree_sync::FolderTreeSync;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::virtual_folder_wording;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::folders::normalize_path;
use rustview_core::library::models::FolderId;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{ModelRc, VecModel};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

impl BridgeFolders {
    pub fn persist_expanded_folders(settings: &SettingsManager, expanded_set: &HashSet<FolderId>) {
        let mut cur_settings = settings.get();
        if !cur_settings.folders.persist_folder_tree_state {
            return;
        }

        let mut ids: Vec<u64> = expanded_set.iter().copied().collect();
        ids.sort_unstable();
        if cur_settings.folders.expanded_folder_ids != ids {
            cur_settings.folders.expanded_folder_ids = ids;
            settings.save_quiet_async(&cur_settings);
        }
    }

    pub fn toggle_expand(
        folder_id: FolderId,
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        {
            let mut st = state.write();
            if st.expanded_folder_ids.contains(&folder_id) {
                st.expanded_folder_ids.remove(&folder_id);
            } else {
                st.expanded_folder_ids.insert(folder_id);
            }
            Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
        }

        Self::reload_tree(window, library, settings, state);
    }

    /// Renders the folder tree from the in-memory Single Point of Truth snapshot with zero filesystem I/O.
    /// The Slint model is updated surgically (row-level insert/remove/move/update) instead of being replaced.
    /// Without a configured/registered root the snapshot is empty and an empty tree is shown.
    pub fn reload_tree(window: &MainWindow, library: &Library, settings: &SettingsManager, state: &RwLock<UiState>) {
        FolderTreeSync::refresh(window, library, settings, state);
    }

    /// A folder was restored in the database behind the cached snapshot: the snapshot is read again, the tree, the folder rows of the
    /// Browse view and the Inspector show the folder as present.
    pub fn reload_tree_after_restore(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        state.write().invalidate_folder_hierarchy();
        Self::reload_tree(window, library, settings, state);
        BridgeInspector::update(window, state);
    }

    /// Clears the whole browsing view for the "no root directory" situation.
    pub fn clear_view(window: &MainWindow, state: &Arc<RwLock<UiState>>, status_msg: &str) {
        state.write().invalidate_folder_hierarchy();
        Self::clear_view_with_empty_state(
            window,
            state,
            status_msg,
            "No Root Directory",
            Icon::Settings,
            "No root directory selected",
        );
    }

    /// Clears folder tree, media grid and selection state and shows an empty-state message.
    /// Does not touch the cached `SPoT` snapshot.
    pub fn clear_view_with_empty_state(
        window: &MainWindow,
        state: &Arc<RwLock<UiState>>,
        status_msg: &str,
        empty_title: &str,
        empty_icon: Icon,
        virtual_folder_disabled_reason: &str,
    ) {
        {
            let mut st = state.write();
            st.current_folder_id = None;
            st.current_folder_path = None;
            st.all_folder_media.clear();
            st.active_indices.clear();
            st.mark_list_bytes_dirty();
            st.file_selection.clear();
            st.media_row_map.clear();
            st.last_executed_query.clear();
            st.expanded_folder_ids.clear();
            st.is_query_active = false;
            st.selected_rating = 0;
            st.filter.rating = None;
            st.filter.tag_ids.clear();
            st.tag_filter_query.clear();
            st.is_virtual_folder_mode = false;
            st.virtual_folder_count = 0;
        }
        window.set_folders(ModelRc::new(VecModel::from(vec![])));
        window.set_media_items(ModelRc::new(VecModel::from(vec![])));
        window.set_tag_filter_text("".into());
        window.set_is_virtual_folder(false);
        window.set_is_virtual_folder_btn_enabled(false);
        window.set_virtual_folder_tooltip(
            virtual_folder_wording::tooltip(false, Some(virtual_folder_disabled_reason)).into(),
        );
        window.set_virtual_folder_count(0);

        window.set_empty_state_active(true);
        window.set_empty_state_title(empty_title.into());
        window.set_empty_state_description(status_msg.into());
        window.set_empty_state_icon(empty_icon.to_slint());

        BridgeFavorites::set_selected_path(None);
        StatusBarBridge::info(window, status_msg);
        StatusBarBridge::update_metrics(window, state);
        BridgeInspector::update(window, state);
    }

    pub fn prune_expanded_folders(deleted_ids: &[FolderId], settings: &SettingsManager, state: &RwLock<UiState>) {
        let mut st = state.write();
        for id in deleted_ids {
            st.expanded_folder_ids.remove(id);
        }
        st.invalidate_folder_hierarchy();
        Self::persist_expanded_folders(settings, &st.expanded_folder_ids);
    }

    pub fn find_visible_successor_folder(
        deleted_id: FolderId,
        affected_subtree_ids: &[FolderId],
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) -> Option<(FolderId, PathBuf)> {
        let snapshot = Self::ensure_snapshot(library, state);

        let affected_set: HashSet<FolderId> = affected_subtree_ids.iter().copied().collect();
        let target_node = snapshot.get_node(deleted_id)?;
        let parent_id = target_node.parent_id;

        let siblings = snapshot.get_children(parent_id);
        let current_pos = siblings.iter().position(|&id| id == deleted_id)?;

        for &sib_id in siblings.iter().skip(current_pos + 1) {
            if !affected_set.contains(&sib_id)
                && let Some(node) = snapshot.get_node(sib_id)
                && !node.is_deleted
            {
                return Some((node.id, node.path.clone()));
            }
        }

        for &sib_id in siblings[..current_pos].iter().rev() {
            if !affected_set.contains(&sib_id)
                && let Some(node) = snapshot.get_node(sib_id)
                && !node.is_deleted
            {
                return Some((node.id, node.path.clone()));
            }
        }

        if let Some(pid) = parent_id
            && !affected_set.contains(&pid)
            && let Some(node) = snapshot.get_node(pid)
        {
            return Some((node.id, node.path.clone()));
        }

        let app_settings = settings.get();
        let root_setting = app_settings.general.root_folder.as_ref()?;
        let normalized_root = normalize_path(root_setting);

        library
            .folders()
            .get_folder_by_path(&normalized_root)
            .ok()
            .flatten()
            .map(|f| (f.id, normalize_path(Path::new(&f.path))))
    }

    pub fn navigate_to_successor_if_affected(
        successor_target: Option<(FolderId, PathBuf)>,
        affected_folder_ids: &[FolderId],
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let is_current_affected = state
            .read()
            .current_folder_id
            .is_some_and(|id| affected_folder_ids.contains(&id));

        if is_current_affected {
            if let Some((succ_id, succ_path)) = successor_target {
                tracing::info!(
                    successor_id = succ_id,
                    successor_path = %LogObscurer::path(&succ_path),
                    "[FolderNavigation: Successor] Navigating active view to resolved successor folder."
                );
                Self::select_folder(succ_id, succ_path, window, library, thumbnails, settings, state);
            } else {
                Self::fallback_to_root_if_affected(affected_folder_ids, window, library, thumbnails, settings, state);
            }
        } else {
            Self::reload_tree(window, library, settings, state);
            Self::scroll_to_selected(window);
            StatusBarBridge::update_metrics(window, state);
        }
    }

    pub fn fallback_to_root_if_affected(
        affected_folder_ids: &[FolderId],
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let is_current = state
            .read()
            .current_folder_id
            .is_some_and(|id| affected_folder_ids.contains(&id));

        if !is_current {
            return;
        }

        let root_setting = settings.get().general.root_folder;
        let Some(ref root_p) = root_setting else {
            return;
        };

        if let Ok(Some(root_rec)) = library.folders().get_folder_by_path(root_p) {
            tracing::info!(
                root_path = %LogObscurer::path(root_p),
                "[FolderNavigation: RootFallback] Falling back to root directory after deletion"
            );
            Self::select_folder(
                root_rec.id,
                normalize_path(root_p),
                window,
                library,
                thumbnails,
                settings,
                state,
            );
        }
    }
}
