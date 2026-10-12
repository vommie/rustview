//! Resolves the current Browse selection into regeneration targets.

use crate::library::models::MediaId;
use crate::library::Library;
use crate::media::MediaType;
use crate::thumbnails::regeneration::RegenerationTarget;
use crate::ui::bridge_state::UiState;
use std::collections::HashSet;

/// True if at least one selected item is a video (single pass, early exit).
pub fn selection_has_video(st: &UiState) -> bool {
    let selected = st.file_selection.selected_ids();
    !selected.is_empty()
        && st
            .all_folder_media
            .iter()
            .any(|m| matches!(m.record.media_type, MediaType::Video) && selected.contains(&m.record.id))
}

/// Collects one target per selected media item in a single pass over the loaded items.
/// Ids that are not part of the loaded list are resolved through the library (relative to the current folder).
pub fn selected_targets(library: &Library, st: &UiState) -> Vec<RegenerationTarget> {
    let selected = st.file_selection.selected_ids();
    let mut targets: Vec<RegenerationTarget> = Vec::with_capacity(selected.len());
    let mut found: HashSet<MediaId> = HashSet::with_capacity(selected.len());

    for item in &st.all_folder_media {
        if selected.contains(&item.record.id) {
            found.insert(item.record.id);
            targets.push(RegenerationTarget { media_id: item.record.id, path: item.full_path.clone() });
        }
    }

    if found.len() < selected.len() {
        resolve_unloaded(library, st, &found, &mut targets);
    }
    targets
}

fn resolve_unloaded(library: &Library, st: &UiState, found: &HashSet<MediaId>, targets: &mut Vec<RegenerationTarget>) {
    let Some(folder_path) = st.current_folder_path.as_ref() else {
        return;
    };
    for &id in st.file_selection.selected_ids() {
        if found.contains(&id) {
            continue;
        }
        match library.media().get(id) {
            Ok(Some(record)) => {
                targets.push(RegenerationTarget { media_id: id, path: folder_path.join(&record.filename) });
            }
            Ok(None) => tracing::warn!(media_id = id, "[ThumbnailRegeneration: Targets] Selected item not in library"),
            Err(e) => tracing::warn!(media_id = id, error = %e, "[ThumbnailRegeneration: Targets] Library lookup failed"),
        }
    }
}
