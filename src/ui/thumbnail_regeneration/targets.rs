//! Resolves the current Browse selection into regeneration targets.

use super::eligibility;
use crate::thumbnails::regeneration::RegenerationTarget;
use crate::ui::bridge_state::UiState;

/// One target per selected valid video. Images and unavailable files (deleted, pending, corrupted) are skipped, and
/// no database or drive access happens: the stored path of the loaded record is used as is.
pub fn selected_targets(st: &UiState) -> Vec<RegenerationTarget> {
    let targets: Vec<RegenerationTarget> = eligibility::eligible_items(st)
        .map(|item| RegenerationTarget { media_id: item.record.id, path: item.full_path.clone() })
        .collect();
    tracing::debug!(
        selected = st.file_selection.count(),
        targets = targets.len(),
        "[ThumbnailRegeneration: Targets] Resolved regeneration targets (valid videos only)"
    );
    targets
}
