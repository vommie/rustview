//! The rating keys of the main window (Settings > Hotkeys; Ctrl+0 for no rating to Ctrl+5 by default).
//!
//! The Browse view rates the selected media (never folder rows), a viewer tab rates the selected medium of its Multiview, and
//! while a fullscreen Multiview is open the keys belong to it. The keys of the fullscreen window and of the video windows
//! arrive through `ui::multiview::rating`; every path ends in `BridgeTags::apply_rating_to_ids`, the one that the Tag & Rate
//! panel and the context menu use. Which key is a rating key, and key repeat, are decided by the keymap (`ui::hotkeys`).

mod targets;

pub use targets::{Standing, rateable_ids};

use crate::ui::MainWindow;
use crate::ui::multiview::{MultiviewHandle, tab_host};
use crate::ui::state::UiState;
use crate::ui::tags::{BridgeTags, TagActionContext};
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::MediaId;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use std::sync::Arc;

/// Everything the hotkey needs from the application.
pub struct RatingHotkeyParams<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub fullscreen: &'a MultiviewHandle,
}

/// What the application knows about a medium, from memory and the library only (no drive access).
pub fn standing_of(state: &UiState, library: &Library, id: MediaId) -> Standing {
    let deleted = state.all_folder_media.find(id).map_or_else(
        || library.media().get(id).ok().flatten().map(|record| record.is_deleted),
        |item| Some(item.record.is_deleted),
    );
    match deleted {
        Some(false) => Standing::Rateable,
        Some(true) => Standing::Deleted,
        None => Standing::Unknown,
    }
}

/// A rating key of the main window (`rating` 0 = no rating): the open fullscreen, else the view the main window shows.
pub fn rate(params: &RatingHotkeyParams<'_>, rating: u8) {
    if params.window.get_is_modal_locked() || params.window.get_is_fullscreen_locked() {
        tracing::debug!(
            rating,
            "[RatingHotkey] Ignored: a dialog or the fullscreen has the main window locked"
        );
        return;
    }
    if params.fullscreen.is_open() {
        params.fullscreen.rate_fullscreen(rating);
        return;
    }
    match params.window.get_active_view_mode() {
        0 => rate_browse_selection(params, rating),
        1 => {
            if let Some(instance) = tab_host::shown_instance() {
                params.fullscreen.rate_visible(&instance, rating);
            } else {
                tracing::debug!(rating, "[RatingHotkey] No Multiview is shown in the tab");
            }
        }
        mode => tracing::debug!(rating, mode, "[RatingHotkey] Ignored: this view has nothing to rate"),
    }
}

/// Rates the selected media of the Browse view. Folder rows are never part of that selection.
fn rate_browse_selection(params: &RatingHotkeyParams<'_>, rating: u8) {
    let ids = {
        let state = params.state.read();
        rateable_ids(state.file_selection.selected_ids().iter().copied(), |id| {
            standing_of(&state, params.library, id)
        })
    };
    if ids.is_empty() {
        tracing::debug!(rating, "[RatingHotkey] Nothing to rate in the Browse selection");
        return;
    }
    tracing::info!(rating, media = ids.len(), "[RatingHotkey] Rating the Browse selection");
    BridgeTags::apply_rating_to_ids(
        &ids,
        rating,
        &TagActionContext {
            window: params.window,
            library: params.library,
            thumbnails: params.thumbnails,
            tab_manager: params.tab_manager,
            settings: params.settings,
            state: params.state,
        },
    );
}
