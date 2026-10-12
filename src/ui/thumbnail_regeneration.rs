//! Browse toolbar button and wording for "regenerate thumbnail(s)" of the selected media items.
//!
//! The actual work lives in exactly one place, `BridgeMedia::regenerate_selected_thumbnails_random`. The toolbar
//! button and the context menu entry both call it, and both take their wording from this module, so singular/plural
//! text can never drift apart.

use super::bridge_media::BridgeMedia;
use super::bridge_state::{LastActivePanel, UiState};
use super::MainWindow;
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::sync::Arc;

pub struct ThumbnailRegeneration;

impl ThumbnailRegeneration {
    /// Label of the context menu entry (singular for one file, plural with count otherwise).
    pub fn menu_label(selected_count: usize, has_video: bool) -> String {
        match (selected_count == 1, has_video) {
            (true, true) => "Thumbnail (Random Pos.)".to_string(),
            (true, false) => "Thumbnail".to_string(),
            (false, true) => format!("Thumbnails (Random Pos.) ({} Files)", selected_count),
            (false, false) => format!("Thumbnails ({} Files)", selected_count),
        }
    }

    /// Tooltip of the Browse toolbar button; mirrors the singular/plural wording of the context menu entry.
    pub fn tooltip(selected_count: usize, has_video: bool) -> String {
        match (selected_count, has_video) {
            (0, _) => "Regenerate thumbnails: select one or more files first".to_string(),
            (1, true) => "Regenerate thumbnail at a random video position".to_string(),
            (1, false) => "Regenerate thumbnail".to_string(),
            (n, true) => format!(
                "Regenerate thumbnails of {} selected files (videos at random positions)",
                n
            ),
            (n, false) => format!("Regenerate thumbnails of {} selected files", n),
        }
    }

    /// True if at least one selected item is a video (single pass, early exit).
    pub fn selection_has_video(st: &UiState) -> bool {
        let selected = st.file_selection.selected_ids();
        !selected.is_empty()
            && st
                .all_folder_media
                .iter()
                .any(|m| matches!(m.record.media_type, MediaType::Video) && selected.contains(&m.record.id))
    }

    /// Pushes the selection dependent toolbar state (enabled flag and tooltip) to the window.
    /// The busy lock is applied by the toolbar itself via `is-busy`.
    pub fn sync_toolbar_state(window: &MainWindow, state: &RwLock<UiState>) {
        let (count, has_video) = {
            let st = state.read();
            (st.file_selection.count(), Self::selection_has_video(&st))
        };
        window.set_is_regenerate_thumbs_enabled(count > 0);
        window.set_regenerate_thumbs_tooltip(Self::tooltip(count, has_video).into());
    }

    /// Connects the Browse toolbar button to the shared regeneration flow.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        tab_manager: Arc<ViewerTabManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        window.on_regenerate_thumbs_clicked(move || {
            tracing::info!("[ThumbnailRegeneration: ToolbarButton] Regenerate requested for the selected media items");
            state.write().last_active_panel = LastActivePanel::Media;
            BridgeMedia::regenerate_selected_thumbnails_random(
                &win,
                &library,
                &thumbnails,
                &tab_manager,
                &settings,
                &state,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::ThumbnailRegeneration as Regen;

    #[test]
    fn menu_label_is_singular_for_one_file() {
        assert_eq!(Regen::menu_label(1, true), "Thumbnail (Random Pos.)");
        assert_eq!(Regen::menu_label(1, false), "Thumbnail");
    }

    #[test]
    fn menu_label_is_plural_with_count_for_many_files() {
        assert_eq!(Regen::menu_label(3, true), "Thumbnails (Random Pos.) (3 Files)");
        assert_eq!(Regen::menu_label(3, false), "Thumbnails (3 Files)");
    }

    #[test]
    fn tooltip_follows_selection_size_and_content() {
        assert!(Regen::tooltip(0, false).contains("select"));
        assert_eq!(Regen::tooltip(1, true), "Regenerate thumbnail at a random video position");
        assert_eq!(Regen::tooltip(1, false), "Regenerate thumbnail");
        assert_eq!(
            Regen::tooltip(4, true),
            "Regenerate thumbnails of 4 selected files (videos at random positions)"
        );
        assert_eq!(Regen::tooltip(4, false), "Regenerate thumbnails of 4 selected files");
    }
}
