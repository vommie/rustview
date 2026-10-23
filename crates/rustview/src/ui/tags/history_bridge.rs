use crate::ui::media::inspector::BridgeInspector;
use crate::ui::state::UiState;
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tags::history::{TagHistoryItem, TagHistoryLayoutEngine, TagHistoryManager};
use crate::ui::tags::{TagActionHandler, TagSelectionStats, TagTreeSync};
use crate::ui::{MainWindow, TagHistoryItemModel, TagHistoryRowModel};
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::TagId;
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
use slint::{Model, ModelRc, VecModel};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

/// Central coordinator bridge for Assigned Tags History lifecycle, flow layout calculation,
/// persistence, height restoration, and interactive toggles.
pub struct BridgeTagHistory;

impl BridgeTagHistory {
    /// Validates and initializes Tag History from settings and database, synchronizing with UI.
    pub fn init(window: &MainWindow, library: &Library, settings: &SettingsManager, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let app_settings = settings.get();
        let mut history_ids = app_settings.tags.history_tag_ids.clone();
        let max_items = app_settings.tags.max_history_tags;
        let is_collapsed = app_settings.tags.is_history_collapsed;
        let history_height = app_settings.tags.history_height;

        if let Ok(all_tags) = library.tags().list_all_tags() {
            let valid_ids: HashSet<TagId> = all_tags.into_iter().map(|t| t.id).collect();
            TagHistoryManager::prune_invalid(&mut history_ids, &valid_ids);
        }
        history_ids.truncate(max_items);

        {
            let mut st = state.write();
            st.tag_history_ids = history_ids.clone();
            st.max_history_tags = max_items;
            st.is_tag_history_collapsed = is_collapsed;
            st.tag_history_height = history_height;
        }

        if app_settings.tags.history_tag_ids != history_ids {
            let mut updated_settings = app_settings;
            updated_settings.tags.history_tag_ids = history_ids;
            settings.save_quiet_async(&updated_settings);
        }

        window.set_tag_history_height(history_height);
        window.set_tag_history_is_collapsed(is_collapsed);

        Self::sync_ui(window, library, state);

        tracing::info!(
            history_count = state.read().tag_history_ids.len(),
            is_collapsed,
            history_height,
            elapsed_us = t0.elapsed().as_micros(),
            "[BridgeTagHistory: Init] Initialized and validated Assigned Tags History with persistent height"
        );
    }

    /// Synchronizes tag history items, formatting, and row packing to the Slint `MainWindow`.
    pub fn sync_ui(window: &MainWindow, library: &Library, state: &RwLock<UiState>) {
        let t0 = Instant::now();
        let is_tab_mode = window.get_active_view_mode() == 1;

        let (history_ids, is_collapsed, history_height, available_w, selected_file_ids, has_selection) = {
            let s = state.read();
            let file_ids = if is_tab_mode {
                s.active_tab_selection().unwrap_or_default()
            } else {
                s.file_selection.selected_ids().clone()
            };

            let has_sel = !file_ids.is_empty();

            (
                s.tag_history_ids.clone(),
                s.is_tag_history_collapsed,
                s.tag_history_height,
                s.tag_history_available_width,
                file_ids,
                has_sel,
            )
        };

        if history_ids.is_empty() {
            window.set_tag_history_rows(ModelRc::new(VecModel::from(vec![])));
            window.set_tag_history_count(0);
            window.set_tag_history_is_collapsed(is_collapsed);
            window.set_tag_history_height(history_height);
            return;
        }

        let all_tags_map = library
            .tags()
            .list_all_tags()
            .unwrap_or_default()
            .into_iter()
            .map(|t| (t.id, t))
            .collect::<HashMap<_, _>>();

        let stats = TagSelectionStats::compute(&selected_file_ids, library);

        let mut items = Vec::with_capacity(history_ids.len());

        for tag_id in history_ids {
            let Some(tag_rec) = all_tags_map.get(&tag_id) else {
                continue;
            };

            let tag_name = tag_rec.name.trim().to_string();

            let (is_assigned, is_partial, is_bold, is_italic, display_text) = if !has_selection {
                (false, false, false, false, tag_name.clone())
            } else if stats.selected_files_count == 1 {
                let assigned = stats.is_fully_assigned(tag_id);
                (assigned, false, assigned, false, tag_name.clone())
            } else {
                let count = stats.assigned_count(tag_id);
                let assigned = stats.is_fully_assigned(tag_id);
                let partial = stats.is_partially_assigned(tag_id);
                let is_any_assigned = count > 0;
                let text = if is_any_assigned {
                    format!("{tag_name} ({count})")
                } else {
                    tag_name.clone()
                };
                // In multi-selection: bold if fully assigned, italic if partial or assigned
                (assigned, partial, assigned, is_any_assigned, text)
            };

            let has_icon = is_assigned || is_partial;
            let pill_width = TagHistoryLayoutEngine::estimate_stable_pill_width(&display_text, has_icon);
            let tooltip = TagHistoryManager::build_hierarchy_tooltip(tag_id, &all_tags_map);

            items.push(TagHistoryItem {
                id: tag_id,
                name: tag_name,
                display_text,
                is_assigned,
                is_partial,
                is_bold,
                is_italic,
                width: pill_width,
                tooltip,
            });
        }

        let total_valid_count = items.len();
        let (rows, _) = TagHistoryLayoutEngine::pack_into_rows(items, available_w);

        let slint_rows: Vec<TagHistoryRowModel> = rows
            .into_iter()
            .map(|r| {
                let slint_items: Vec<TagHistoryItemModel> = r
                    .items
                    .into_iter()
                    .map(|it| TagHistoryItemModel {
                        id: it.id as i32,
                        name: it.name.into(),
                        display_text: it.display_text.into(),
                        is_assigned: it.is_assigned,
                        is_partial: it.is_partial,
                        is_bold: it.is_bold,
                        is_italic: it.is_italic,
                        width: it.width,
                        tooltip: it.tooltip.into(),
                    })
                    .collect();

                TagHistoryRowModel {
                    items: ModelRc::new(VecModel::from(slint_items)),
                }
            })
            .collect();

        window.set_tag_history_rows(ModelRc::new(VecModel::from(slint_rows)));
        window.set_tag_history_count(total_valid_count as i32);
        window.set_tag_history_is_collapsed(is_collapsed);
        window.set_tag_history_height(history_height);

        tracing::debug!(
            total_items = total_valid_count,
            selected_files = stats.selected_files_count,
            row_count = window.get_tag_history_rows().row_count(),
            history_height,
            elapsed_us = t0.elapsed().as_micros(),
            "[BridgeTagHistory: Sync] Tag history items synchronized and packed into rows"
        );
    }

    /// Handles clicking a tag pill in the Tag History:
    /// - If currently fully assigned across all selected files, unassigns it without changing history position.
    /// - If mixed (partial) or unassigned, assigns it to all selected files and moves the tag to position 0 (front).
    pub fn handle_tag_history_clicked(
        tag_id: TagId,
        window: &MainWindow,
        library: &Arc<Library>,
        _thumbnails: &Arc<ThumbnailManager>,
        tab_manager: &Arc<ViewerTabManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let is_tab_active = window.get_active_view_mode() == 1 || tab_manager.active_tab_id().is_some();
        let selected_ids = {
            let s = state.read();
            if is_tab_active {
                if let Some(ids) = s.active_tab_selection() {
                    ids
                } else if let Some(m_id) = tab_manager.get_active_media_id() {
                    HashSet::from([m_id])
                } else {
                    s.file_selection.selected_ids().clone()
                }
            } else {
                s.file_selection.selected_ids().clone()
            }
        };

        if selected_ids.is_empty() {
            StatusBarBridge::info(window, "Select one or more files to assign/unassign tags.");
            return;
        }

        let tag_name = library
            .tags()
            .get_tag(tag_id)
            .ok()
            .flatten()
            .map_or_else(|| format!("Tag #{tag_id}"), |t| t.name);

        let stats = TagSelectionStats::compute(&selected_ids, library);
        let total_selected = selected_ids.len();

        // Tri-state logic: Only unassign if ALL selected files currently have the tag!
        // If mixed/partial or none have it, assign to all selected files.
        let should_unassign = stats.is_fully_assigned(tag_id);

        if should_unassign {
            tracing::info!(
                tag = %LogObscurer::tag(Some(tag_id), &tag_name),
                affected_files = total_selected,
                action = "UnassignAll",
                "[BridgeTagHistory: Toggle] Fully assigned tag clicked: Unassigning from all selected files (position preserved)"
            );
            for &id in &selected_ids {
                let _ = library.tags().remove_tag_from_media(id, tag_id);
            }
            StatusBarBridge::success(
                window,
                format!("Removed tag '{tag_name}' from {total_selected} file(s)"),
            );
            // Position in history is strictly preserved on unassign
        } else {
            tracing::info!(
                tag = %LogObscurer::tag(Some(tag_id), &tag_name),
                affected_files = total_selected,
                previously_assigned = stats.assigned_count(tag_id),
                action = "AssignAll",
                "[BridgeTagHistory: Toggle] Partial or unassigned tag clicked: Assigning to all selected files and moving to front"
            );
            for &id in &selected_ids {
                let _ = library.tags().assign_tag(id, tag_id);
            }
            StatusBarBridge::success(window, format!("Assigned tag '{tag_name}' to {total_selected} file(s)"));

            // Tag moves to position 0 ONLY when assigned
            let max_items = state.read().max_history_tags;
            let mut st = state.write();
            TagHistoryManager::record_tag_used(&mut st.tag_history_ids, tag_id, max_items);
            let mut app_settings = settings.get();
            app_settings.tags.history_tag_ids = st.tag_history_ids.clone();
            settings.save_quiet_async(&app_settings);
        }

        TagActionHandler::update_grid_tag_badges(&selected_ids, window, library, state);
        TagTreeSync::sync_in_place(window, library, state);
        BridgeInspector::update(window, state);
        BridgeViewerTabs::sync_ui(window, tab_manager, library, settings, state);
        StatusBarBridge::update_metrics(window, state);
        Self::sync_ui(window, library, state);
    }

    /// Handles toggling the collapsed state of the Tag History panel.
    pub fn handle_collapsed_toggled(is_collapsed: bool, settings: &SettingsManager, state: &RwLock<UiState>) {
        {
            let mut st = state.write();
            st.is_tag_history_collapsed = is_collapsed;
        }
        let mut app_settings = settings.get();
        app_settings.tags.is_history_collapsed = is_collapsed;
        settings.save_quiet_async(&app_settings);

        tracing::info!(
            is_collapsed,
            history_height = state.read().tag_history_height,
            "[BridgeTagHistory: CollapseToggle] Collapsed state persisted; height restored/kept at last configured size"
        );
    }

    /// Records that a tag was assigned (e.g. from Tag Tree or shortcut) and puts it at the front of history.
    pub fn record_tag_assigned(tag_id: TagId, settings: &SettingsManager, state: &RwLock<UiState>) {
        let max_items = state.read().max_history_tags;
        let mut st = state.write();
        TagHistoryManager::record_tag_used(&mut st.tag_history_ids, tag_id, max_items);

        let mut app_settings = settings.get();
        app_settings.tags.history_tag_ids = st.tag_history_ids.clone();
        settings.save_quiet_async(&app_settings);
    }

    /// Prunes deleted tags from the history and persists changes.
    pub fn prune_deleted_tags(deleted_ids: &HashSet<TagId>, settings: &SettingsManager, state: &RwLock<UiState>) {
        let mut st = state.write();
        TagHistoryManager::prune_tags(&mut st.tag_history_ids, deleted_ids);

        let mut app_settings = settings.get();
        app_settings.tags.history_tag_ids = st.tag_history_ids.clone();
        settings.save_quiet_async(&app_settings);
    }

    /// Updates available width for flow wrapping upon container resize.
    pub fn handle_geometry_changed(
        available_width: f32,
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
    ) {
        let diff = (state.read().tag_history_available_width - available_width).abs();
        if diff > 4.0 {
            state.write().tag_history_available_width = available_width;
            Self::sync_ui(window, library, state);
        }
    }
}
