//! Regenerates ONLY the thumbnails of the selected valid videos at a random position ("Thumbnail (Random Pos.)").
//!
//! This is not a refresh: metadata, hashes, flags and viewer tabs are never touched (see
//! `rustview_core::thumbnails::regeneration`). Images have no duration and are never handled here; the general thumbnail
//! refresh for every media type is "Refresh > File (Full)". The Browse toolbar button and the context menu entry
//! share one state source (`eligibility`, in-memory only, no drive access), one entry point
//! (`ThumbnailRegeneration::regenerate_selected`) and one wording module (`wording`). While a regeneration runs, the
//! "Gen" progress is shown and the same busy lock as for refreshes is held, which disables every refresh control.

mod apply;
pub mod eligibility;
mod runner;
mod targets;
pub mod wording;

use super::MainWindow;
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::status_bar::StatusBarBridge;
use parking_lot::RwLock;
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::ThumbnailManager;
use slint::ComponentHandle;
use std::sync::Arc;

pub struct ThumbnailRegeneration;

impl ThumbnailRegeneration {
    /// Evaluates the current selection (in-memory only). Shared by the toolbar and the context menu.
    pub fn eligibility(state: &RwLock<UiState>) -> eligibility::RegenerationEligibility {
        eligibility::evaluate(&state.read())
    }

    /// Label of the context menu entry; only valid videos are counted.
    pub fn menu_label(eligibility: &eligibility::RegenerationEligibility) -> String {
        wording::menu_label(eligibility.videos)
    }

    /// Tooltip shared by the toolbar button and the context menu entry.
    pub fn hint(eligibility: &eligibility::RegenerationEligibility) -> String {
        wording::hint(eligibility)
    }

    /// Tooltip of a context menu entry that is disabled because another operation is running.
    pub fn busy_hint() -> &'static str {
        wording::busy_hint()
    }

    /// Pushes the selection dependent toolbar state (enabled flag and tooltip) to the window.
    /// The busy lock is applied by the toolbar itself via `is-busy`.
    pub fn sync_toolbar_state(window: &MainWindow, state: &RwLock<UiState>) {
        let e = Self::eligibility(state);
        tracing::trace!(
            selected = e.selected,
            videos = e.videos,
            enabled = e.is_actionable(),
            "[ThumbnailRegeneration: State] Toolbar state synchronized"
        );
        window.set_is_regenerate_thumbs_enabled(e.is_actionable());
        window.set_regenerate_thumbs_tooltip(wording::hint(&e).into());
    }

    /// The single entry point for the toolbar button and the context menu: regenerates the thumbnails of the
    /// valid videos of the current selection in the background, refusing to start while another operation is running.
    pub fn regenerate_selected(
        window: &MainWindow,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        if crate::ui::media::BridgeMedia::is_system_busy(window, thumbnails, state) {
            StatusBarBridge::warn(window, wording::busy_message());
            return;
        }

        let targets = targets::selected_targets(&state.read());
        if targets.is_empty() {
            tracing::info!("[ThumbnailRegeneration: Start] Ignored: no valid video in the selection");
            StatusBarBridge::info(window, wording::nothing_selected_message());
            return;
        }
        runner::start(window, thumbnails, settings, state, targets);
    }

    /// Connects the Browse toolbar button to the shared regeneration flow.
    pub fn register_callbacks(
        window: &MainWindow,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let win = window.clone_strong();
        window.on_regenerate_thumbs_clicked(move || {
            tracing::info!("[ThumbnailRegeneration: ToolbarButton] Regenerate requested for the selected media items");
            state.write().last_active_panel = LastActivePanel::Media;
            Self::regenerate_selected(&win, &thumbnails, &settings, &state);
        });
    }
}
