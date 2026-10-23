//! UI-thread completion of a thumbnail regeneration: releases the busy lock, shows the new thumbnails and reports.

use super::wording::{self, RegenerationSummary};
use crate::ui::MainWindow;
use crate::ui::browse_window::update_tile;
use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use parking_lot::RwLock;
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::thumbnails::regeneration::{RegeneratedThumbnail, RegenerationBatchResult};
use rustview_core::util::cast::SaturatingAs;
use slint::Image;
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
    let st = state.read();
    let is_table = window.get_is_table_view();

    for item in succeeded {
        let buffer = item.thumbnail.pixels.clone();
        st.thumbs.insert(item.media_id, buffer.clone());

        if !is_table {
            update_tile(window, &st, item.media_id, |view| {
                view.thumbnail = Image::from_rgb8(buffer);
            });
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
        elapsed_ms = summary.elapsed_ms.saturating_as::<u64>(),
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
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}
