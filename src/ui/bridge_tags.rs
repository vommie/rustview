use super::bridge_inspector::BridgeInspector;
use super::bridge_media::BridgeMedia;
use super::bridge_state::UiState;
use super::{MainWindow, TagNodeView};
use crate::library::Library;
use crate::library::models::{MediaId, TagId};
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::{ModelRc, VecModel};
use std::sync::Arc;

/// Subsystem managing tags, query modes, and rating assignments.
pub struct BridgeTags;

impl BridgeTags {
    pub fn reload_tags(window: &MainWindow, library: &Library) {
        let tags = library.tags().list_all_tags().unwrap_or_default();
        let views: Vec<TagNodeView> = tags
            .into_iter()
            .map(|t| TagNodeView {
                id: t.id as i32,
                name: t.name.into(),
                depth: if t.parent_id.is_some() { 1 } else { 0 },
                is_selected: false,
                is_partial: false,
                is_match: false,
            })
            .collect();
        window.set_tags(ModelRc::new(VecModel::from(views)));
    }

    pub fn filter_tags(
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
        filter_text: &str,
    ) {
        let filter_lower = filter_text.to_lowercase();
        let tags = library.tags().list_all_tags().unwrap_or_default();
        let selected_tags = state.read().filter.tag_ids.clone();

        let views: Vec<TagNodeView> = tags
            .into_iter()
            .map(|t| {
                let is_match =
                    !filter_lower.is_empty() && t.name.to_lowercase().contains(&filter_lower);
                let is_selected = selected_tags.contains(&t.id);
                TagNodeView {
                    id: t.id as i32,
                    name: t.name.into(),
                    depth: if t.parent_id.is_some() { 1 } else { 0 },
                    is_selected,
                    is_partial: false,
                    is_match,
                }
            })
            .collect();
        window.set_tags(ModelRc::new(VecModel::from(views)));
    }

    pub fn toggle_tag(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (selected_ids, is_assign) = {
            let s = state.read();
            (s.file_selection.selected_ids().clone(), s.is_assign_mode)
        };

        if is_assign {
            for id in selected_ids {
                let _ = library.tags().assign_tag(id, tag_id);
            }
            let current_folder = state.read().current_folder_id;
            let current_path = state.read().current_folder_path.clone();
            if let (Some(f_id), Some(path)) = (current_folder, current_path) {
                let raw = library.media().get_by_folder(f_id).unwrap_or_default();
                state.write().all_folder_media = BridgeMedia::preindex(library, raw, &path);
            }
        } else {
            let mut st = state.write();
            if st.filter.tag_ids.contains(&tag_id) {
                st.filter.tag_ids.remove(&tag_id);
            } else {
                st.filter.tag_ids.insert(tag_id);
            }
        }

        BridgeInspector::update(window, state);
        BridgeMedia::reload(window, library, thumbnails, state, false, false);
    }

    pub fn apply_rating(
        rating: Option<u8>,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let (selected_ids, is_assign) = {
            let s = state.read();
            (s.file_selection.selected_ids().clone(), s.is_assign_mode)
        };

        if is_assign {
            if let Some(r) = rating {
                for id in selected_ids {
                    let _ = library.media().set_rating(id, r);
                }
                let current_folder = state.read().current_folder_id;
                let current_path = state.read().current_folder_path.clone();
                if let (Some(f_id), Some(path)) = (current_folder, current_path) {
                    let raw = library.media().get_by_folder(f_id).unwrap_or_default();
                    state.write().all_folder_media = BridgeMedia::preindex(library, raw, &path);
                }
            }
        } else {
            state.write().filter.rating = rating;
        }

        BridgeInspector::update(window, state);
        BridgeMedia::reload(window, library, thumbnails, state, false, false);
    }

    pub fn apply_rating_to_ids(
        ids: &std::collections::HashSet<MediaId>,
        rating: u8,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        for &id in ids {
            let _ = library.media().set_rating(id, rating);
        }
        let current_folder = state.read().current_folder_id;
        let current_path = state.read().current_folder_path.clone();
        if let (Some(f_id), Some(path)) = (current_folder, current_path) {
            let raw = library.media().get_by_folder(f_id).unwrap_or_default();
            state.write().all_folder_media = BridgeMedia::preindex(library, raw, &path);
        }
        BridgeInspector::update(window, state);
        BridgeMedia::reload(window, library, thumbnails, state, false, false);
    }
}
