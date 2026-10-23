//! Changing the selection: select all, clear, deselect, reveal and the synchronization with the views.

use crate::ui::MainWindow;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::media::{BridgeMedia, MediaReloadOptions};
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use crate::ui::browse_folders::{BrowseFolders, BrowseItem, SelectionDelta};
use crate::ui::browse_window::update_tile;
use crate::ui::multiview::handle::{self as multiview_handle, Pace};
use rustview_core::library::Library;
use rustview_core::library::models::MediaId;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::Model;
use std::sync::Arc;
use std::time::Instant;
use super::{BridgeFileSelection, FileClickEvent};

impl BridgeFileSelection {
    /// Programmatically selects a single media item in Browse Mode, synchronizing UI tiles,
    /// viewport windowing/slicing, Inspector, Tag & Rate panel, and status bar.
    pub fn select_and_reveal_media(
        media_id: MediaId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: Option<&Arc<ThumbnailManager>>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t_start = Instant::now();

        let (delta, target_visual_idx_opt) = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let delta = st.browse_click(BrowseItem::Media(media_id), false, false);
            let visual_idx = st.row_of(media_id).map(|row| row + st.folder_front_len());
            (delta, visual_idx)
        };
        let changed_ids = delta.media;
        if delta.folders_changed {
            BrowseFolders::refresh_folder_tiles(window, state);
        }

        if let Some(target_idx) = target_visual_idx_opt {
            let need_slice_reanchor = !state.read().ring.range().contains(&target_idx);

            if need_slice_reanchor {
                if let Some(thumb_mgr) = thumbnails {
                    BridgeMedia::reload_with_options(
                        window,
                        library,
                        thumb_mgr,
                        settings,
                        state,
                        MediaReloadOptions::with_anchor(false, false, target_idx),
                    );
                } else if !changed_ids.is_empty() {
                    Self::sync_selection_diff(window, state, &changed_ids);
                }
            } else if !changed_ids.is_empty() {
                Self::sync_selection_diff(window, state, &changed_ids);
            }

            window.invoke_scroll_media_item_into_view(target_idx as i32);
        } else if !changed_ids.is_empty() {
            Self::sync_selection_diff(window, state, &changed_ids);
        }

        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
        multiview_handle::on_user_selection_changed(Pace::Immediate);

        tracing::info!(
            media_id,
            visual_index = ?target_visual_idx_opt,
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: SelectAndReveal] Media item selected and revealed in Browse Mode"
        );
    }

    pub(super) fn mutate_selection(event: FileClickEvent, state: &Arc<RwLock<UiState>>) -> (SelectionDelta, usize) {
        let mut st = state.write();
        st.last_active_panel = LastActivePanel::Media;
        let delta = st.browse_click(BrowseItem::Media(event.media_id), event.is_ctrl, event.is_shift);
        let count = st.file_selection.count();
        (delta, count)
    }

    /// Redraws the tiles a selection change touched, media and folder rows.
    pub fn sync_selection_delta(window: &MainWindow, state: &RwLock<UiState>, delta: &SelectionDelta) {
        Self::sync_selection_diff(window, state, &delta.media);
        if delta.folders_changed {
            BrowseFolders::refresh_folder_tiles(window, state);
        }
    }

    /// Surgically modifies only changed rows in the Slint Model translating global indices to local viewport offsets.
    /// Employs an O(Viewport) threshold optimization when changed_ids is large (e.g. Select All / Deselect All)
    /// to eliminate CPU freezes on large datasets.
    pub fn sync_selection_diff(
        window: &MainWindow,
        state: &RwLock<UiState>,
        changed_ids: &[MediaId],
    ) {
        if changed_ids.is_empty() {
            return;
        }
        let media_model = window.get_media_items();
        let visible_count = media_model.row_count();
        let st = state.read();

        // O(Viewport) Optimization:
        // If changed_ids contains many elements (Select All or Clear Selection across 1k-100k items),
        // iterating directly over the rows of the tile pool (~60 to ~160 rows) is orders of magnitude
        // faster than running tens of thousands of hashmap lookups.
        if changed_ids.len() > visible_count * 2 {
            for slot in 0..visible_count {
                if let Some(mut item) = media_model.row_data(slot) {
                    let is_selected = st.file_selection.is_selected(item.id as u64);
                    if item.is_selected != is_selected {
                        item.is_selected = is_selected;
                        media_model.set_row_data(slot, item);
                    }
                }
            }
            return;
        }

        for &id in changed_ids {
            let is_selected = st.file_selection.is_selected(id);
            update_tile(window, &st, id, |item| item.is_selected = is_selected);
        }
    }

    /// Selects everything the Browse view lists: all media and, when folders are shown, all folder rows ("..." excepted).
    pub fn select_all(window: &MainWindow, library: &Arc<Library>, state: &Arc<RwLock<UiState>>) {
        let t_start = Instant::now();
        let (delta, folder_count, file_count) = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;

            let visible_ids: Vec<MediaId> = if !st.active_indices.is_empty() {
                st.active_indices
                    .iter()
                    .filter_map(|&idx| st.all_folder_media.get(idx))
                    .map(|m| m.record.id)
                    .collect()
            } else if st.is_query_active || !st.filter.is_empty() {
                // Active filter/query returned zero results; do not select inactive items
                Vec::new()
            } else {
                st.all_folder_media.iter().map(|m| m.record.id).collect()
            };

            let delta = st.browse_select_all(&visible_ids);
            let (folders, files) = st.browse_selection_counts();
            (delta, folders, files)
        };

        Self::sync_selection_delta(window, state, &delta);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
        multiview_handle::on_user_selection_changed(Pace::Immediate);

        if folder_count + file_count > 0 {
            let message = match (folder_count, file_count) {
                (0, files) => format!("Selected all {} media file(s).", files),
                (folders, 0) => format!("Selected all {} folder(s).", folders),
                (folders, files) => format!("Selected all: {} folder(s) and {} media file(s).", folders, files),
            };
            StatusBarBridge::info(window, message);
        } else {
            StatusBarBridge::info(window, "Nothing to select.");
        }

        tracing::info!(
            selected_files = file_count,
            selected_folders = folder_count,
            changed_rows = delta.media.len(),
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: SelectAll] Selected all visible items in Browse view"
        );
    }

    /// Deselects one media item (for example because the Multiview removed it) and brings the tiles, Inspector, Tag & Rate
    /// panel and status bar in line. Does nothing if the item is not selected.
    pub fn deselect_media(
        media_id: MediaId,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        {
            let mut st = state.write();
            if !st.file_selection.is_selected(media_id) {
                return;
            }
            st.file_selection
                .deselect_many(&std::collections::HashSet::from([media_id]));
        }
        Self::sync_selection_diff(window, state, &[media_id]);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
        tracing::info!(media_id, "[FileSelection: Deselect] Media item deselected");
    }

    /// Deselects everything in the current Browse View (Ctrl+Shift+A / Esc): media and folder rows.
    pub fn clear_selection(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let t_start = Instant::now();
        let (delta, prev_media, prev_folders) = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let (folders, files) = st.browse_selection_counts();
            (st.clear_browse_selection(), files, folders)
        };

        Self::sync_selection_delta(window, state, &delta);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);

        if prev_media + prev_folders > 0 {
            let message = match (prev_folders, prev_media) {
                (0, files) => format!("Deselected {} media file(s).", files),
                (folders, 0) => format!("Deselected {} folder(s).", folders),
                (folders, files) => format!("Deselected {} folder(s) and {} media file(s).", folders, files),
            };
            StatusBarBridge::info(window, message);
        }

        tracing::info!(
            cleared_files = prev_media,
            cleared_folders = prev_folders,
            elapsed_us = t_start.elapsed().as_micros(),
            "[FileSelection: ClearSelection] Cleared the selection in Browse view"
        );
    }
}
