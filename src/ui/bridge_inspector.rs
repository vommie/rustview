use super::models::{format_bytes_with_locale, format_filesize};
use super::MainWindow;
use super::bridge_state::UiState;
use super::file_selection::FileSelectionSummary;
use crate::media::MediaHasher;
use crate::query::filters::IndexedMediaRecord;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Dedicated subsystem responsible for formatting and updating the Metadata Inspector.
pub struct BridgeInspector;

impl BridgeInspector {
    /// Updates inspector fields on MainWindow according to current selection or active viewer tab.
    pub fn update(window: &MainWindow, state: &RwLock<UiState>) {
        let is_tab_mode = window.get_active_view_mode() == 1;
        let st = state.read();

        // When in Tab Mode, inspect the active tab's media item directly without overwriting browse selection
        if is_tab_mode
            && let Some(media_id) = st.active_tab_media_id
            && let Some(item) = st.all_folder_media.iter().find(|m| m.record.id == media_id)
        {
            Self::render_single(window, item);
            return;
        }

        let selected_ids = st.file_selection.selected_ids();
        let sel_count = selected_ids.len();

        if sel_count == 0 {
            Self::render_empty(window);
            return;
        }

        // Fast O(1) single-item path (zero string allocations, direct lookup in in-memory dataset)
        if sel_count == 1 {
            let single_id = *selected_ids.iter().next().unwrap();
            let item_opt = st.all_folder_media.iter().find(|m| m.record.id == single_id);

            if let Some(item) = item_opt {
                Self::render_single(window, item);
            } else {
                Self::render_empty(window);
            }
            return;
        }

        // Multi-selection path: Zero-copy iterator aggregation over all matching records
        let selected_items: Vec<&IndexedMediaRecord> = st
            .all_folder_media
            .iter()
            .filter(|m| selected_ids.contains(&m.record.id))
            .collect();

        if selected_items.is_empty() {
            Self::render_empty(window);
            return;
        }

        let summary = FileSelectionSummary::from_indexed_records(selected_items.iter().copied());
        Self::render_multiple(
            window,
            &summary,
            &selected_items,
            st.current_folder_path.as_deref(),
        );
    }

    fn render_empty(window: &MainWindow) {
        window.set_info_filename("- (No selection)".into());
        window.set_info_filename_tooltip("".into());
        window.set_info_path_label("Path:".into());
        window.set_info_path("-".into());
        window.set_info_path_tooltip("".into());
        window.set_info_size("-".into());
        window.set_info_size_tooltip("".into());
        window.set_info_dimensions("-".into());
        window.set_info_format("-".into());
        window.set_info_hash("-".into());
        window.set_info_hash_tooltip("".into());
        window.set_info_selection_count(0);
    }

    fn render_single(window: &MainWindow, item: &IndexedMediaRecord) {
        let full_filename = item.record.filename.clone();
        window.set_info_filename(item.filename_slint.clone());
        window.set_info_filename_tooltip(full_filename.into());

        window.set_info_path_label("Path:".into());
        let parent_path_str = item
            .full_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "-".to_string());
        window.set_info_path(parent_path_str.clone().into());
        window.set_info_path_tooltip(parent_path_str.into());

        window.set_info_size(format_filesize(item.record.file_size).into());
        window.set_info_size_tooltip(format_bytes_with_locale(item.record.file_size).into());

        let dim_str = if item.is_pending {
            "Pending (Render Queue)".to_string()
        } else if item.record.width > 0 && item.record.height > 0 {
            format!("{} × {}", item.record.width, item.record.height)
        } else {
            "-".to_string()
        };
        window.set_info_dimensions(dim_str.into());

        let format_display = if item.is_pending {
            format!("{} (Pending)", item.record.format.display_name())
        } else {
            item.record.format.display_name().to_string()
        };
        window.set_info_format(format_display.into());

        let (hash_str, tooltip_str) = if item.is_pending {
            let src_text = if let Some(ref src) = item.record.source {
                format!("Pending [Source: {}]", src)
            } else {
                "Pending (External API Queue)".to_string()
            };
            (src_text.clone(), src_text)
        } else {
            let hash_hex = MediaHasher::to_hex(&item.record.file_hash);
            let mut tt = hash_hex.clone();
            if let Some(ref src) = item.record.source {
                tt = format!("MD5: {}\nSource: {}", hash_hex, src);
            }
            (hash_hex, tt)
        };

        window.set_info_hash(hash_str.into());
        window.set_info_hash_tooltip(tooltip_str.into());
        window.set_info_selection_count(1);
    }

    fn render_multiple(
        window: &MainWindow,
        summary: &FileSelectionSummary,
        selected_items: &[&IndexedMediaRecord],
        root_folder_opt: Option<&Path>,
    ) {
        window.set_info_selection_count(summary.count as i32);
        let multi_title = format!("{} multiple files selected", summary.count);
        window.set_info_filename(multi_title.clone().into());
        window.set_info_filename_tooltip(multi_title.into());
        window.set_info_path_label("Subfolders:".into());

        // Group counts by folder path
        let mut folder_counts: HashMap<PathBuf, (String, usize, bool)> = HashMap::new();
        for item in selected_items {
            if let Some(parent) = item.full_path.parent() {
                let is_root = root_folder_opt.map(|r| r == parent).unwrap_or(false);
                let folder_name = parent
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| parent.to_string_lossy().to_string());

                let entry = folder_counts.entry(parent.to_path_buf()).or_insert((folder_name, 0, is_root));
                entry.1 += 1;
            }
        }

        let mut sorted_folders: Vec<(String, usize, bool)> = folder_counts.into_values().collect();
        sorted_folders.sort_by(|a, b| {
            match (a.2, b.2) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
            }
        });

        let subfolders_str = if sorted_folders.is_empty() {
            "-".to_string()
        } else {
            sorted_folders
                .into_iter()
                .map(|(name, count, _)| format!("{} ({})", name, count))
                .collect::<Vec<_>>()
                .join(", ")
        };

        window.set_info_path(subfolders_str.into());
        let bare_root_path = root_folder_opt
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "-".to_string());
        window.set_info_path_tooltip(bare_root_path.into());

        window.set_info_size(format!("{} (total)", format_filesize(summary.total_size_bytes)).into());
        window.set_info_size_tooltip(format_bytes_with_locale(summary.total_size_bytes).into());

        let dim_str = if summary.is_uniform_dimensions {
            if let Some((w, h)) = summary.min_dimensions {
                format!("{} × {} (Uniform)", w, h)
            } else {
                "-".to_string()
            }
        } else if let (Some(min), Some(max)) = (summary.min_dimensions, summary.max_dimensions) {
            format!("{} × {} – {} × {}", min.0, min.1, max.0, max.1)
        } else {
            "-".to_string()
        };
        window.set_info_dimensions(dim_str.into());

        let format_str = summary
            .format_breakdown
            .iter()
            .map(|(fmt, count)| format!("{} ({})", fmt.display_name(), count))
            .collect::<Vec<_>>()
            .join(", ");
        window.set_info_format(format_str.into());

        let detail_str = if summary.rated_count > 0 {
            format!(
                "Avg Rating: {:.1} ★ ({} rated)",
                summary.avg_rating, summary.rated_count
            )
        } else {
            "— (Multi-selection)".to_string()
        };
        window.set_info_hash(detail_str.clone().into());
        window.set_info_hash_tooltip(detail_str.into());
    }
}
