//! Regenerates ONLY the thumbnails of the selected media items (videos at a random position).
//!
//! This is not a refresh: metadata, hashes, flags and viewer tabs are never touched (see
//! `crate::thumbnails::regeneration`). The Browse toolbar button and the context menu entry both call
//! `ThumbnailRegeneration::regenerate_selected`, and both take their wording from `wording`, so the flow and the
//! singular/plural text exist exactly once. While it runs, the "Gen" progress is shown and the same busy lock as
//! for refreshes is held, which disables every refresh control.

mod apply;
mod runner;
mod targets;
pub mod wording;

use super::bridge_state::{LastActivePanel, UiState};
use super::bridge_status_bar::StatusBarBridge;
use super::MainWindow;
use crate::library::Library;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::sync::Arc;

pub struct ThumbnailRegeneration;

impl ThumbnailRegeneration {
    /// Label of the context menu entry (singular for one file, plural with count otherwise).
    pub fn menu_label(selected_count: usize, has_video: bool) -> String {
        wording::menu_label(selected_count, has_video)
    }

    /// Pushes the selection dependent toolbar state (enabled flag and tooltip) to the window.
    /// The busy lock is applied by the toolbar itself via `is-busy`.
    pub fn sync_toolbar_state(window: &MainWindow, state: &RwLock<UiState>) {
        let (count, has_video) = {
            let st = state.read();
            (st.file_selection.count(), targets::selection_has_video(&st))
        };
        window.set_is_regenerate_thumbs_enabled(count > 0);
        window.set_regenerate_thumbs_tooltip(wording::tooltip(count, has_video).into());
    }

    /// The single entry point for the toolbar button and the context menu: regenerates the thumbnails of the
    /// current selection in the background, refusing to start while another operation is running.
    pub fn regenerate_selected(
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if super::bridge_media::BridgeMedia::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(window, wording::busy_message());
            return;
        }

        let targets = targets::selected_targets(library, &state.read());
        if targets.is_empty() {
            StatusBarBridge::info(window, wording::nothing_selected_message());
            return;
        }
        runner::start(window, thumbnails, settings, state, targets);
    }

    /// Connects the Browse toolbar button to the shared regeneration flow.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        window.on_regenerate_thumbs_clicked(move || {
            tracing::info!("[ThumbnailRegeneration: ToolbarButton] Regenerate requested for the selected media items");
            state.write().last_active_panel = LastActivePanel::Media;
            Self::regenerate_selected(&win, &library, &thumbnails, &settings, &state);
        });
    }
}
