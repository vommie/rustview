use super::models::format_filesize;
use super::thumbnail_regeneration::ThumbnailRegeneration;
use super::MainWindow;
use crate::settings::LogObscurer;
use crate::thumbnails::ThumbnailProgressSnapshot;
use crate::ui::bridge_state::UiState;
use parking_lot::RwLock;

/// Semantic message severity level for status bar notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum StatusMessageLevel {
    #[default]
    Info = 0,
    Success = 1,
    Warning = 2,
    Error = 3,
}

/// Centralized, modular Status Bar Controller & API Service.
/// Coordinates status message notifications, task progress updates, and live dataset & selection metrics.
pub struct StatusBarBridge;

impl StatusBarBridge {
    /// Sets a status message on the main window with explicit severity level.
    pub fn set_message(
        window: &MainWindow,
        message: impl Into<String>,
        level: StatusMessageLevel,
    ) {
        let msg = message.into();
        tracing::debug!(
            level = ?level,
            message = %LogObscurer::query(&msg),
            "[StatusBar: Message] Updated status bar message"
        );
        window.set_status_level(level as i32);
        window.set_status_text(msg.into());
    }

    /// Convenience endpoint for informational status messages.
    pub fn info(window: &MainWindow, message: impl Into<String>) {
        Self::set_message(window, message, StatusMessageLevel::Info);
    }

    /// Convenience endpoint for success confirmation status messages.
    pub fn success(window: &MainWindow, message: impl Into<String>) {
        Self::set_message(window, message, StatusMessageLevel::Success);
    }

    /// Convenience endpoint for warning status messages.
    pub fn warn(window: &MainWindow, message: impl Into<String>) {
        Self::set_message(window, message, StatusMessageLevel::Warning);
    }

    /// Convenience endpoint for error status messages.
    pub fn error(window: &MainWindow, message: impl Into<String>) {
        Self::set_message(window, message, StatusMessageLevel::Error);
    }

    /// Synchronizes selection counts, visible items, total items, and formatted byte sizes to the status bar.
    pub fn update_metrics(window: &MainWindow, state: &RwLock<UiState>) {
        let st = state.read();

        let total_items = st.all_folder_media.len();
        let visible_count = if !st.active_indices.is_empty() {
            st.active_indices.len()
        } else {
            total_items
        };

        let selected_ids = st.file_selection.selected_ids();
        let sel_count = selected_ids.len();
        let folder_sel_count = st.folder_rows.selection_count();

        let total_bytes = st.visible_total_bytes();
        let selected_bytes: u64 = selected_ids
            .iter()
            .filter_map(|&id| st.record_by_id(id))
            .map(|item| item.record.file_size)
            .sum();

        let is_query_active = st.is_query_active;
        let is_virtual = st.is_virtual_folder_mode;
        let virtual_count = st.virtual_folder_count;

        drop(st);

        let metric_text = if is_query_active {
            if visible_count == total_items {
                format!("Query Items: {}", visible_count)
            } else {
                format!("Query Filter: {} / {}", visible_count, total_items)
            }
        } else if is_virtual {
            format!("Recursive Items: {} / {}", visible_count, total_items)
        } else if visible_count != total_items {
            format!("Items: {} / {}", visible_count, total_items)
        } else {
            format!("Items: {}", visible_count)
        };

        let total_size_str = if total_bytes > 0 {
            format_filesize(total_bytes)
        } else {
            String::new()
        };

        let sel_size_str = if selected_bytes > 0 {
            format_filesize(selected_bytes)
        } else {
            String::new()
        };

        window.set_items_metric_text(metric_text.into());
        window.set_status_total_items(total_items as i32);
        window.set_status_visible_items(visible_count as i32);
        window.set_status_total_size(total_size_str.into());
        window.set_status_selection_count(sel_count as i32);
        window.set_status_selection_folders(folder_sel_count as i32);
        window.set_status_selection_size(sel_size_str.into());
        window.set_is_virtual_folder(is_virtual);
        window.set_virtual_folder_count(virtual_count as i32);

        ThumbnailRegeneration::sync_toolbar_state(window, state);
    }

    /// Updates the folder structure scan progress section.
    pub fn update_folder_scan_progress(
        window: &MainWindow,
        is_refreshing: bool,
        scanned_count: usize,
    ) {
        window.set_is_refreshing_folders(is_refreshing);
        window.set_folder_scan_count(scanned_count as i32);
    }

    /// Updates the media files discovery/indexing progress section.
    pub fn update_media_index_progress(
        window: &MainWindow,
        is_indexing: bool,
        current: usize,
        total: usize,
    ) {
        window.set_is_indexing_media(is_indexing);
        window.set_media_index_current(current as i32);
        window.set_media_index_total(total as i32);
    }

    /// Updates the thumbnail pipeline progress bars.
    pub fn update_thumbnail_progress(
        window: &MainWindow,
        progress: &ThumbnailProgressSnapshot,
    ) {
        window.set_is_preloading(progress.is_preloading);
        window.set_preload_current(progress.preload_current as i32);
        window.set_preload_total(progress.preload_total as i32);

        window.set_is_generating(progress.is_generating);
        window.set_generate_current(progress.generate_current as i32);
        window.set_generate_total(progress.generate_total as i32);
        window.set_active_workers(progress.active_workers as i32);
    }

    /// Updates the virtual folder recursive mode indicator.
    pub fn update_virtual_folder(window: &MainWindow, is_virtual: bool, count: usize) {
        window.set_is_virtual_folder(is_virtual);
        window.set_virtual_folder_count(count as i32);
    }
}
