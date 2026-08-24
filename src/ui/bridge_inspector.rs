use super::MainWindow;
use super::bridge_state::UiState;
use super::file_selection::FileSelectionSummary;
use super::models::format_filesize;
use crate::media::MediaHasher;
use crate::query::filters::IndexedMediaRecord;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Dedicated subsystem responsible for formatting and updating the Metadata Inspector.
pub struct BridgeInspector;

impl BridgeInspector {
    /// Updates inspector fields on MainWindow according to current selection.
    pub fn update(window: &MainWindow, state: &RwLock<UiState>) {
        let st = state.read();
        let selected_ids = st.file_selection.selected_ids();
        let sel_count = selected_ids.len();

        if sel_count == 0 || st.active_indices.is_empty() {
            Self::render_empty(window);
            return;
        }

        // Fast O(1) single-item path (zero string allocations, zero scans)
        if sel_count == 1 {
            let single_id = *selected_ids.iter().next().unwrap();
            let item_opt = st
                .media_row_map
                .get(&single_id)
                .and_then(|&row_idx| st.active_indices.get(row_idx))
                .and_then(|&orig_idx| st.all_folder_media.get(orig_idx));

            if let Some(item) = item_opt {
                Self::render_single(window, item);
            } else {
                Self::render_empty(window);
            }
            return;
        }

        // Multi-selection path: Zero-copy iterator aggregation over visible items only
        let selected_items: Vec<&IndexedMediaRecord> = st
            .active_indices
            .iter()
            .filter_map(|&idx| st.all_folder_media.get(idx))
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
        window.set_info_path_label("Path:".into());
        window.set_info_path("-".into());
        window.set_info_size("-".into());
        window.set_info_dimensions("-".into());
        window.set_info_format("-".into());
        window.set_info_hash("-".into());
        window.set_info_selection_count(0);
    }

    fn render_single(window: &MainWindow, item: &IndexedMediaRecord) {
        window.set_info_filename(item.filename_slint.clone());
        window.set_info_path_label("Path:".into());
        let parent_path_str = item
            .full_path
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "-".to_string());
        window.set_info_path(parent_path_str.into());

        window.set_info_size(format_filesize(item.record.file_size).into());
        let dim_str = if item.record.width > 0 && item.record.height > 0 {
            format!("{} × {}", item.record.width, item.record.height)
        } else {
            "-".to_string()
        };
        window.set_info_dimensions(dim_str.into());
        window.set_info_format(format!("{:?}", item.record.format).into());
        window.set_info_hash(MediaHasher::to_hex(&item.record.file_hash).into());
        window.set_info_selection_count(1);
    }

    fn render_multiple(
        window: &MainWindow,
        summary: &FileSelectionSummary,
        selected_items: &[&IndexedMediaRecord],
        root_folder_opt: Option<&Path>,
    ) {
        window.set_info_selection_count(summary.count as i32);
        window.set_info_filename(format!("{} multiple files selected", summary.count).into());
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
        // Sort: Root folder on the far left (is_root == true), then remaining subfolders in ascending alphabetical order
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

        window.set_info_size(format!("{} (total)", format_filesize(summary.total_size_bytes)).into());

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
            .map(|(fmt, count)| format!("{:?} ({})", fmt, count))
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
        window.set_info_hash(detail_str.into());
    }
}
