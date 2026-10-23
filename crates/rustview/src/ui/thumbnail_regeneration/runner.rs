//! Background execution of a thumbnail regeneration with "Gen" progress and the shared busy lock.

use super::{apply, wording};
use rustview_core::settings::SettingsManager;
use rustview_core::thumbnails::regeneration::{regenerate_batch, RegenerationBatchResult, RegenerationTarget};
use rustview_core::thumbnails::{ThumbnailConfig, ThumbnailManager};
use crate::ui::bridge_state::UiState;
use crate::ui::bridge_status_bar::StatusBarBridge;
use crate::ui::MainWindow;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;
use std::time::Instant;

/// Locks the UI (same flags the refresh operations use), starts the "Gen" progress and regenerates in the background.
pub fn start(
    window: &MainWindow,
    thumbnails: &Arc<ThumbnailManager>,
    settings: &Arc<SettingsManager>,
    state: &Arc<RwLock<UiState>>,
    targets: Vec<RegenerationTarget>,
) {
    let total = targets.len();
    state.write().is_refreshing_media = true;
    window.set_is_system_busy(true);
    thumbnails.tracker().set_generate_total(total);
    StatusBarBridge::update_thumbnail_progress(window, &thumbnails.progress());
    StatusBarBridge::info(window, wording::start_message(total));
    tracing::info!(total, "[ThumbnailRegeneration: Start] Regenerating thumbnails only (videos at random positions)");

    let config = ThumbnailConfig::from_performance_settings(&settings.get().performance);
    let weak = window.as_weak();
    let thumb = thumbnails.clone();
    let st = state.clone();

    let spawned = std::thread::Builder::new().name("thumbnail-regeneration".into()).spawn(move || {
        let t_start = Instant::now();
        let batch = run_batch(&thumb, &config, targets);
        let elapsed = t_start.elapsed();
        let _ = weak.upgrade_in_event_loop(move |w| apply::finish(&w, &thumb, &st, total, batch, elapsed));
    });

    if let Err(e) = spawned {
        tracing::error!(error = %e, "[ThumbnailRegeneration: Start] Failed to spawn worker thread");
        apply::release_busy(window, thumbnails, state);
        StatusBarBridge::error(window, format!("Could not start thumbnail regeneration: {}", e));
    }
}

/// A panic inside the decoders must never leave the UI locked, so it is converted into an empty (all failed) result.
fn run_batch(thumbnails: &ThumbnailManager, config: &ThumbnailConfig, targets: Vec<RegenerationTarget>) -> RegenerationBatchResult {
    let tracker = thumbnails.tracker();
    let on_item_done = move || tracker.increment_generate_done();
    catch_unwind(AssertUnwindSafe(|| regenerate_batch(thumbnails, config, targets, &on_item_done))).unwrap_or_else(|_| {
        tracing::error!("[ThumbnailRegeneration: Run] Worker panicked; releasing the UI lock");
        RegenerationBatchResult::default()
    })
}
