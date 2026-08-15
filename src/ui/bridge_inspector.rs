use super::MainWindow;
use super::bridge_state::UiState;
use super::file_selection::FileSelectionSummary;
use super::models::format_filesize;
use crate::query::filters::IndexedMediaRecord;
use parking_lot::RwLock;

/// Dedicated subsystem responsible for formatting and updating the Metadata Inspector.
pub struct BridgeInspector;

impl BridgeInspector {
    /// Updates inspector fields on MainWindow according to current selection.
    pub fn update(window: &MainWindow, state: &RwLock<UiState>) {
        let st = state.read();
        let selected_ids = st.file_selection.selected_ids();
        let sel_count = selected_ids.len();

        if sel_count == 0 {
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
                .and_then(|&orig_idx| st.all_folder_media.get(orig_idx))
                .or_else(|| {
                    st.all_folder_media
                        .iter()
                        .find(|m| m.record.id == single_id)
                });

            if let Some(item) = item_opt {
                Self::render_single(window, item);
            }
            return;
        }

        // Multi-selection path: Zero-copy iterator aggregation
        let selected_records_iter = st
            .all_folder_media
            .iter()
            .filter(|m| selected_ids.contains(&m.record.id));

        let summary = FileSelectionSummary::from_indexed_records(selected_records_iter);
        Self::render_multiple(window, &summary);
    }

    fn render_empty(window: &MainWindow) {
        window.set_info_filename("- (No selection)".into());
        window.set_info_size("-".into());
        window.set_info_dimensions("-".into());
        window.set_info_format("-".into());
        window.set_info_hash("-".into());
        window.set_info_selection_count(0);
    }

    fn render_single(window: &MainWindow, item: &IndexedMediaRecord) {
        window.set_info_filename(item.filename_slint.clone());
        window.set_info_size(item.filesize_str.clone());
        window.set_info_dimensions(item.dimensions_str.clone());
        window.set_info_format(format!("{:?}", item.record.format).into());
        window.set_info_hash(format!("{:x?}", &item.record.file_hash[..8]).into());
        window.set_info_selection_count(1);
    }

    fn render_multiple(window: &MainWindow, summary: &FileSelectionSummary) {
        window.set_info_selection_count(summary.count as i32);
        window.set_info_filename(format!("{} multiple files selected", summary.count).into());
        window
            .set_info_size(format!("{} (total)", format_filesize(summary.total_size_bytes)).into());

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
