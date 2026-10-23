use crate::ui::MainWindow;
use crate::ui::browse_folders::{FolderStamp, stamp_labels};
use crate::ui::models::{format_bytes_with_locale, format_filesize};
use crate::ui::selection::model::FileSelectionSummary;
use crate::ui::state::UiState;
use parking_lot::RwLock;
use rustview_core::media::MediaHasher;
use rustview_core::query::filters::IndexedMediaRecord;
use rustview_core::util::cast::SaturatingAs;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Dedicated subsystem responsible for formatting and updating the Metadata Inspector.
pub struct BridgeInspector;

impl BridgeInspector {
    /// Updates inspector fields on `MainWindow` according to current selection or active viewer tab.
    pub fn update(window: &MainWindow, state: &RwLock<UiState>) {
        let is_tab_mode = window.get_active_view_mode() == 1;
        let st = state.read();

        // When in Tab Mode, inspect the active tab's media (one item, or the tiles of a Multiview) directly without
        // overwriting the browse selection
        if is_tab_mode && !st.active_tab_media_ids.is_empty() {
            let tab_items: Vec<&IndexedMediaRecord> = st
                .all_folder_media
                .iter()
                .filter(|m| st.active_tab_media_ids.contains(&m.record.id))
                .collect();
            if Self::render_group(window, &st, &tab_items) {
                return;
            }
        }

        let (folder_count, file_count) = st.browse_selection_counts();
        if folder_count > 0 {
            Self::render_folders(window, &st, folder_count, file_count);
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
            let item_opt = st.all_folder_media.find(single_id);

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

        Self::render_group(window, &st, &selected_items);
    }

    /// The labels of the rows below the path and the badge of the title; "" = the default of files.
    fn set_labels(window: &MainWindow, size: &str, dimensions: &str, format: &str, hash: &str, badge: &str) {
        window.set_info_size_label(size.into());
        window.set_info_dimensions_label(dimensions.into());
        window.set_info_format_label(format.into());
        window.set_info_hash_label(hash.into());
        window.set_info_badge_text(badge.into());
    }

    fn plural(count: usize, one: &str, many: &str) -> String {
        format!("{} {}", count, if count == 1 { one } else { many })
    }

    /// A selection with folders in it: one folder in detail, anything else as a summary.
    fn render_folders(window: &MainWindow, st: &UiState, folder_count: usize, file_count: usize) {
        let snapshot = st.folder_hierarchy.as_deref();
        let ids = st.folder_rows.selected_folders_ordered();

        if folder_count == 1 && file_count == 0 {
            let Some(node) = ids.first().and_then(|&id| snapshot.and_then(|s| s.get_node(id))) else {
                Self::render_empty(window);
                return;
            };
            // The date of a selected folder is in the cache from its tile; a drive read only fills a gap (one folder, user action).
            let stamp = st
                .folder_times
                .peek(node.id)
                .unwrap_or_else(|| FolderStamp::read(&node.path));
            let (modified, created) = stamp_labels(Some(stamp));
            let dash = |text: slint::SharedString| {
                if text.is_empty() {
                    slint::SharedString::from("-")
                } else {
                    text
                }
            };

            Self::set_labels(window, "Contents:", "Modified:", "Created:", "Type:", "Folder");
            window.set_info_filename(node.name.as_str().into());
            window.set_info_filename_tooltip(node.path.to_string_lossy().to_string().into());
            window.set_info_path_label("Path:".into());
            let parent = node
                .path
                .parent()
                .map_or_else(|| "-".to_string(), |p| p.to_string_lossy().to_string());
            window.set_info_path(parent.clone().into());
            window.set_info_path_tooltip(parent.into());
            window.set_info_size(
                format!(
                    "{}, {}",
                    Self::plural(node.subfolder_count as usize, "folder", "folders"),
                    Self::plural(node.media_count as usize, "file", "files")
                )
                .into(),
            );
            window.set_info_size_tooltip(
                format!(
                    "{} in total, subfolders included",
                    Self::plural(node.recursive_media_count as usize, "file", "files")
                )
                .into(),
            );
            window.set_info_dimensions(dash(modified));
            window.set_info_format(dash(created));
            let kind = if node.is_deleted {
                "Folder (missing on disk)"
            } else {
                "Folder"
            };
            window.set_info_hash(kind.into());
            window.set_info_hash_tooltip(node.path.to_string_lossy().to_string().into());
            window.set_info_selection_count(1);
            return;
        }

        // Several items, folders among them.
        let title = match (folder_count, file_count) {
            (folders, 0) => format!("{} selected", Self::plural(folders, "folder", "folders")),
            (folders, files) => format!(
                "{}, {} selected",
                Self::plural(folders, "folder", "folders"),
                Self::plural(files, "file", "files")
            ),
        };
        let size_label = if file_count > 0 { "Size:" } else { "Contents:" };
        Self::set_labels(
            window,
            size_label,
            "Dimensions:",
            "Format:",
            "Details:",
            &title.replace(" selected", ""),
        );
        window.set_info_selection_count((folder_count + file_count).saturating_as::<i32>());
        window.set_info_filename(title.clone().into());
        window.set_info_filename_tooltip(title.into());

        window.set_info_path_label("Path:".into());
        let here = st
            .current_folder_path
            .as_deref()
            .map_or_else(|| "-".to_string(), |p| p.to_string_lossy().to_string());
        window.set_info_path(here.clone().into());
        window.set_info_path_tooltip(here.into());

        if file_count > 0 {
            let items = st.get_selected_media_ordered();
            let summary = FileSelectionSummary::from_indexed_records(items.iter().copied());
            window.set_info_size(format!("{} (files)", format_filesize(summary.total_size_bytes)).into());
            window.set_info_size_tooltip(format_bytes_with_locale(summary.total_size_bytes).into());
        } else {
            let (subfolders, files) = ids
                .iter()
                .filter_map(|&id| snapshot.and_then(|s| s.get_node(id)))
                .fold((0usize, 0usize), |(sub, media), node| {
                    (sub + node.subfolder_count as usize, media + node.media_count as usize)
                });
            window.set_info_size(
                format!(
                    "{}, {}",
                    Self::plural(subfolders, "subfolder", "subfolders"),
                    Self::plural(files, "file", "files")
                )
                .into(),
            );
            window.set_info_size_tooltip("The contents of the selected folders".into());
        }
        window.set_info_dimensions("-".into());
        window.set_info_format("-".into());
        window.set_info_hash("- (Mixed selection)".into());
        window.set_info_hash_tooltip("".into());
    }

    /// Shows one item in detail or several as an aggregate. Returns `false` (and shows nothing) for an empty group.
    fn render_group(window: &MainWindow, st: &UiState, items: &[&IndexedMediaRecord]) -> bool {
        match items {
            [] => false,
            [item] => {
                Self::render_single(window, item);
                true
            }
            _ => {
                let summary = FileSelectionSummary::from_indexed_records(items.iter().copied());
                Self::render_multiple(window, &summary, items, st.current_folder_path.as_deref());
                true
            }
        }
    }

    fn render_empty(window: &MainWindow) {
        Self::set_labels(window, "Size:", "Dimensions:", "Format:", "", "");
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
        Self::set_labels(window, "Size:", "Dimensions:", "Format:", "", "");
        let full_filename = item.record.filename.clone();
        window.set_info_filename(item.record.filename.as_str().into());
        window.set_info_filename_tooltip(full_filename.into());

        window.set_info_path_label("Path:".into());
        let parent_path_str = item.folder.path().to_string_lossy().to_string();
        window.set_info_path(parent_path_str.clone().into());
        window.set_info_path_tooltip(parent_path_str.into());

        window.set_info_size(format_filesize(item.record.file_size).into());
        window.set_info_size_tooltip(format_bytes_with_locale(item.record.file_size).into());

        let dim_str = if item.is_pending {
            "Pending (Render Queue)".to_string()
        } else if item.record.width > 0 && item.record.height > 0 {
            format!("{} x {}", item.record.width, item.record.height)
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
                format!("Pending [Source: {src}]")
            } else {
                "Pending (External API Queue)".to_string()
            };
            (src_text.clone(), src_text)
        } else {
            let hash_hex = MediaHasher::to_hex(&item.record.file_hash);
            let mut tt = hash_hex.clone();
            if let Some(ref src) = item.record.source {
                tt = format!("MD5: {hash_hex}\nSource: {src}");
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
        Self::set_labels(window, "Size:", "Dimensions:", "Format:", "", "");
        window.set_info_selection_count(summary.count.saturating_as::<i32>());
        let multi_title = format!("{} multiple files selected", summary.count);
        window.set_info_filename(multi_title.clone().into());
        window.set_info_filename_tooltip(multi_title.into());
        window.set_info_path_label("Subfolders:".into());

        // Group counts by folder path
        let mut folder_counts: HashMap<PathBuf, (String, usize, bool)> = HashMap::new();
        for item in selected_items {
            let parent = item.folder.path();
            let is_root = root_folder_opt.is_some_and(|r| r == parent);
            let folder_name = parent.file_name().map_or_else(
                || parent.to_string_lossy().to_string(),
                |n| n.to_string_lossy().to_string(),
            );

            let entry = folder_counts
                .entry(parent.to_path_buf())
                .or_insert((folder_name, 0, is_root));
            entry.1 += 1;
        }

        let mut sorted_folders: Vec<(String, usize, bool)> = folder_counts.into_values().collect();
        sorted_folders.sort_by(|a, b| match (a.2, b.2) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
        });

        let subfolders_str = if sorted_folders.is_empty() {
            "-".to_string()
        } else {
            sorted_folders
                .into_iter()
                .map(|(name, count, _)| format!("{name} ({count})"))
                .collect::<Vec<_>>()
                .join(", ")
        };

        window.set_info_path(subfolders_str.into());
        let bare_root_path = root_folder_opt.map_or_else(|| "-".to_string(), |p| p.to_string_lossy().to_string());
        window.set_info_path_tooltip(bare_root_path.into());

        window.set_info_size(format!("{} (total)", format_filesize(summary.total_size_bytes)).into());
        window.set_info_size_tooltip(format_bytes_with_locale(summary.total_size_bytes).into());

        let dim_str = if summary.is_uniform_dimensions {
            if let Some((w, h)) = summary.min_dimensions {
                format!("{w} x {h} (Uniform)")
            } else {
                "-".to_string()
            }
        } else if let (Some(min), Some(max)) = (summary.min_dimensions, summary.max_dimensions) {
            format!("{} x {} - {} x {}", min.0, min.1, max.0, max.1)
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
            format!("Avg Rating: {:.1} ({} rated)", summary.avg_rating, summary.rated_count)
        } else {
            "- (Multi-selection)".to_string()
        };
        window.set_info_hash(detail_str.clone().into());
        window.set_info_hash_tooltip(detail_str.into());
    }
}
