//! UI-thread completion of a thumbnail regeneration: releases the busy lock, shows the new thumbnails and reports.

use super::wording::{self, RegenerationSummary};
use crate::thumbnails::regeneration::{RegeneratedThumbnail, RegenerationBatchResult};
use crate::thumbnails::ThumbnailManager;
use crate::ui::bridge_inspector::BridgeInspector;
use crate::ui::bridge_media::BridgeMedia;
use crate::ui::bridge_state::UiState;
use crate::ui::bridge_status_bar::StatusBarBridge;
use crate::ui::models::rgb8_to_shared_buffer;
use crate::ui::MainWindow;
use parking_lot::RwLock;
use slint::{Image, Model};
use std::time::Duration;

/// Ends the "Gen" progress and the refresh lock. Only the generate counters are cleared; the preload counters
/// belong to the folder pipeline and are never touched here.
pub fn release_busy(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>) {
    thumbnails.tracker().set_generate_total(0);
    StatusBarBridge::update_thumbnail_progress(window, &thumbnails.progress());
    state.write().is_refreshing_media = false;
    window.set_is_system_busy(BridgeMedia::is_system_busy(window, thumbnails, state));
}

pub fn finish(
    window: &MainWindow,
    thumbnails: &ThumbnailManager,
    state: &RwLock<UiState>,
    total: usize,
    batch: RegenerationBatchResult,
    elapsed: Duration,
) {
    release_busy(window, thumbnails, state);
    show_new_thumbnails(window, state, &batch.succeeded);
    report(window, total, &batch, elapsed);
    BridgeInspector::update(window, state);
}

/// Replaces the in-memory thumbnails and the visible grid rows. Metadata strings are intentionally left alone.
fn show_new_thumbnails(window: &MainWindow, state: &RwLock<UiState>, succeeded: &[RegeneratedThumbnail]) {
    if succeeded.is_empty() {
        return;
    }
    let mut st = state.write();
    let model = window.get_media_items();
    let start_window = window.get_visible_start_index() as usize;
    let visible_count = model.row_count();
    let is_table = window.get_is_table_view();

    for item in succeeded {
        let thumb = &item.thumbnail;
        let buffer = rgb8_to_shared_buffer(&thumb.pixels, thumb.width, thumb.height);
        st.folder_thumbnails.insert(item.media_id, buffer.clone());

        if is_table {
            continue;
        }
        let Some(&global_row) = st.media_row_map.get(&item.media_id) else {
            continue;
        };
        if global_row < start_window || global_row >= start_window + visible_count {
            continue;
        }
        let local_row = global_row - start_window;
        if let Some(mut view) = model.row_data(local_row)
            && view.id == item.media_id as i32
        {
            view.thumbnail = Image::from_rgb8(buffer);
            model.set_row_data(local_row, view);
        }
    }
}

fn report(window: &MainWindow, total: usize, batch: &RegenerationBatchResult, elapsed: Duration) {
    let summary = RegenerationSummary {
        total,
        succeeded: batch.succeeded.len(),
        elapsed_ms: elapsed.as_millis(),
        first_success: batch.succeeded.first().map(|r| (file_name(&r.path), r.offset_percent)),
        first_failure: batch.failed.first().map(|f| (file_name(&f.path), f.reason.clone())),
    };
    let message = wording::result_message(&summary);
    tracing::info!(
        total,
        succeeded = summary.succeeded,
        failed = summary.failed(),
        elapsed_ms = summary.elapsed_ms as u64,
        "[ThumbnailRegeneration: Done] {}",
        message
    );

    if summary.failed() == 0 {
        StatusBarBridge::success(window, message);
    } else if summary.succeeded == 0 {
        StatusBarBridge::error(window, message);
    } else {
        StatusBarBridge::warn(window, message);
    }
}

fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}
