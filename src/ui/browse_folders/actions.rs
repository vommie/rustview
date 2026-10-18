//! What the user does with folders in the Browse view: clicking a folder row, opening it (double click, Enter), the "Show Folders"
//! toggle, the "New Folder" button state and what follows a created folder. Everything works on the in-memory folder snapshot.

use super::rows::FolderRowRef;
use super::BrowseFolders;
use crate::library::Library;
use crate::library::models::{FolderId, FolderHierarchySnapshot};
use crate::library::folder_create::FolderCreateReport;
use crate::settings::{LogObscurer, SettingsManager};
use crate::thumbnails::ThumbnailManager;
use crate::ui::bridge_file_selection::BridgeFileSelection;
use crate::ui::bridge_folder_dialogs::FolderDialogContext;
use crate::ui::bridge_folder_tree_sync::FolderTreeSync;
use crate::ui::bridge_folders::BridgeFolders;
use crate::ui::bridge_inspector::BridgeInspector;
use crate::ui::bridge_media::BridgeMedia;
use crate::ui::bridge_state::{LastActivePanel, UiState};
use crate::ui::bridge_status_bar::StatusBarBridge;
use crate::ui::bridge_tags::BridgeTags;
use crate::ui::browse_window::{DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, ViewGeometry, margin_rows_for, sync_window};
use crate::ui::MainWindow;
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle, Model};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// Two clicks on the same folder row within this time open it (the same limit the media tiles use).
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

static LAST_CLICK: Mutex<Option<(i32, Instant)>> = Mutex::new(None);
static THUMBNAILS: OnceLock<Arc<ThumbnailManager>> = OnceLock::new();

impl BrowseFolders {
    /// Wires the callbacks of the folder rows and of the toolbar buttons.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
    ) {
        let _ = THUMBNAILS.set(thumbnails.clone());

        let (win, lib, thumb, set, st) = (window.clone_strong(), library.clone(), thumbnails.clone(), settings.clone(), state.clone());
        window.on_folder_row_clicked(move |id| Self::handle_click(id, &win, &lib, &thumb, &set, &st));

        let (win, lib, thumb, set, st) = (window.clone_strong(), library.clone(), thumbnails.clone(), settings.clone(), state.clone());
        window.on_folder_row_open_requested(move || {
            let selected = st.read().folder_rows.selected();
            if let Some(row) = selected {
                Self::open_row(row, &win, &lib, &thumb, &set, &st);
            }
        });

        let (win, lib, thumb, set, st) = (window.clone_strong(), library, thumbnails, settings, state);
        window.on_show_folders_toggled(move |on| Self::set_show_folders(on, &win, &lib, &thumb, &set, &st));
    }

    /// A single click on a folder row selects it (and deselects the media); the second click of a double click opens it.
    pub fn handle_click(
        id: i32,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let Some(row) = FolderRowRef::from_view_id(id) else {
            return;
        };

        let now = Instant::now();
        let is_double = {
            let mut last = LAST_CLICK.lock();
            let double = last.is_some_and(|(last_id, at)| last_id == id && now.duration_since(at) < DOUBLE_CLICK);
            *last = if double { None } else { Some((id, now)) };
            double
        };
        if is_double {
            Self::open_row(row, window, library, thumbnails, settings, state);
            return;
        }

        let cleared_media = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let previous: Vec<_> = st.file_selection.selected_ids().iter().copied().collect();
            st.file_selection.clear();
            previous
        };
        BridgeFileSelection::sync_selection_diff(window, state, &cleared_media);

        let previous_row = {
            let mut st = state.write();
            let previous = st.folder_rows.selected();
            st.folder_rows.select(Some(row));
            previous
        };
        let st = state.read();
        for changed in previous_row.into_iter().chain(std::iter::once(row)) {
            Self::update_row_tile(window, &st, changed);
        }
        drop(st);

        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
    }

    /// Opens the folder behind a row: the folder is shown like a click in the folder tree would show it, and the tree reveals it.
    pub fn open_row(
        row: FolderRowRef,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let target = {
            let st = state.read();
            let id = match row {
                FolderRowRef::Up => st.folder_rows.up_target(),
                FolderRowRef::Folder(id) => Some(id),
            };
            id.and_then(|id| st.folder_hierarchy.as_ref().and_then(|s| s.get_node(id)).map(|n| (id, n.path.clone())))
        };
        let Some((folder_id, path)) = target else {
            tracing::debug!(?row, "[BrowseFolders: Open] The folder is not in the snapshot; nothing to open");
            return;
        };
        tracing::info!(folder = %LogObscurer::folder(Some(folder_id), &path), up = row == FolderRowRef::Up, "[BrowseFolders: Open] Opening a folder from the media view");

        if row != FolderRowRef::Up {
            // The tree shows the folder below its parent: the parent must be expanded for the new selection to be visible.
            Self::expand_chain_to(state, folder_id);
        }
        BridgeFolders::select_folder(folder_id, path, window, library, thumbnails, settings, state);
    }

    /// Expands every ancestor of `folder_id` in the folder tree (not the folder itself).
    pub fn expand_chain_to(state: &RwLock<UiState>, folder_id: FolderId) {
        let mut st = state.write();
        let chain: Vec<FolderId> = match st.folder_hierarchy.as_ref() {
            Some(snapshot) => Self::ancestors_of(snapshot, folder_id),
            None => return,
        };
        st.expanded_folder_ids.extend(chain);
    }

    fn ancestors_of(snapshot: &FolderHierarchySnapshot, folder_id: FolderId) -> Vec<FolderId> {
        let mut chain = Vec::new();
        let mut current = snapshot.get_node(folder_id).and_then(|n| n.parent_id);
        while let Some(id) = current {
            if chain.contains(&id) {
                break;
            }
            let Some(node) = snapshot.get_node(id) else { break };
            chain.push(id);
            current = node.parent_id;
        }
        chain
    }

    /// Updates the tile of a folder row in place when the pool holds it.
    fn update_row_tile(window: &MainWindow, st: &UiState, row: FolderRowRef) {
        let Some(position) = st.folder_rows.position_of(row, st.active_indices.len()) else {
            return;
        };
        let Some(slot) = st.ring.slot_if_held(position) else {
            return;
        };
        let model = window.get_media_items();
        let Some(mut view) = model.row_data(slot) else {
            return;
        };
        if view.id != row.view_id() {
            return;
        }
        view.is_selected = st.folder_rows.selected() == Some(row);
        model.set_row_data(slot, view);
    }

    /// Drops the highlight of the selected folder row (a medium was selected, or the selection was cleared).
    pub fn clear_folder_selection(window: &MainWindow, state: &RwLock<UiState>) {
        let previous = {
            let mut st = state.write();
            let previous = st.folder_rows.selected();
            st.folder_rows.select(None);
            previous
        };
        if let Some(row) = previous {
            Self::update_row_tile(window, &state.read(), row);
        }
    }

    /// The "Show Folders" toggle of the toolbar.
    pub fn set_show_folders(
        on: bool,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        tracing::info!(on, "[BrowseFolders: Toggle] Show Folders switched");
        {
            let mut st = state.write();
            st.show_folders = on;
            st.last_active_panel = LastActivePanel::Media;
            // Showing the folders anew puts every folder in name order again, the "Up" entry first.
            st.folder_rows.reset_tail();
            st.folder_rows.select(None);
        }
        let mut app_settings = settings.get();
        app_settings.view.show_folders_in_browse = on;
        settings.save_quiet_async(&app_settings);

        Self::refresh_rows(window, library, thumbnails, settings, state);
    }

    /// Recomputes the folder rows from the snapshot and, when they changed, refills the tile pool (O(tile pool)); the media are not
    /// laid out again. Used after the toggle and after changes of the folder tree.
    pub fn refresh_rows(
        window: &MainWindow,
        library: &Library,
        thumbnails: &ThumbnailManager,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        let snapshot = Self::snapshot_if_wanted(library, state);
        let app_settings = settings.get();
        let changed = Self::rebuild_rows(&mut state.write(), snapshot.as_deref(), &app_settings.folders);
        if changed {
            Self::resync_window(window, thumbnails, state);
            BridgeMedia::update_fallback_view(window, &app_settings, state);
            StatusBarBridge::update_metrics(window, state);
        }
    }

    /// The folder tree was rebuilt or patched (rename, move, delete, refresh, new folder): the folder rows follow. Called from
    /// `FolderTreeSync::refresh`, which has no thumbnail manager of its own, hence the one stored at registration.
    pub fn on_tree_changed(window: &MainWindow, library: &Library, settings: &SettingsManager, state: &RwLock<UiState>) {
        if let Some(thumbnails) = THUMBNAILS.get() {
            Self::refresh_rows(window, library, thumbnails, settings, state);
        }
    }

    /// Refills the whole tile pool for the list as it is now, around the rows that were shown.
    pub fn resync_window(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>) {
        let is_table = window.get_is_table_view();
        let mut st = state.write();
        let geometry = st
            .view_geometry
            .unwrap_or_else(|| ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table)));
        // The pool starts `margin_rows` above the first visible row; the viewport callback of the view corrects any difference.
        let first_row = st.ring.range().start / geometry.columns + geometry.margin_rows;
        let view_len = st.view_len();
        window.set_total_items_count(view_len as i32);
        st.invalidate_view_window();
        sync_window(window, &mut st, thumbnails, geometry, first_row, view_len, true);
    }

    /// Enables or disables the "New Folder" button for the view that is shown (memory only).
    pub fn update_new_folder_button(window: &MainWindow, state: &RwLock<UiState>) {
        const DESCRIPTION: &str = "Create a new folder in the open folder";
        let st = state.read();
        let reason = if st.is_query_active {
            Some("Not available while a query filter is active")
        } else if st.is_virtual_folder_mode {
            Some("Not available in the recursive folder view")
        } else if let Some(id) = st.current_folder_id {
            let missing = st.folder_hierarchy.as_ref().and_then(|s| s.get_node(id)).is_some_and(|n| n.is_deleted);
            missing.then_some("The folder does not exist on disk")
        } else {
            Some("No folder selected")
        };
        drop(st);

        window.set_is_new_folder_btn_enabled(reason.is_none());
        window.set_new_folder_tooltip(match reason {
            Some(reason) => format!("{DESCRIPTION}\nDisabled: {reason}"),
            None => DESCRIPTION.to_string(),
        }.into());
    }

    /// Everything that follows folders that were created below `parent_id`: the snapshot gets them (no rescan), the tree shows them
    /// (ancestors expand), and a created folder of the open folder waits at the end of the Browse list until the folder is opened again.
    pub fn folders_created(window: &MainWindow, ctx: &FolderDialogContext, parent_id: FolderId, report: &FolderCreateReport) {
        let records = report.records();
        {
            let mut st = ctx.state.write();
            let inserted = st
                .folder_hierarchy
                .as_mut()
                .map(|snapshot| Arc::make_mut(snapshot).apply_folders_created(&records));
            tracing::info!(
                components = records.len(),
                created_on_disk = report.created_count(),
                inserted_in_snapshot = ?inserted,
                "[BrowseFolders: Created] New folders added to the folder snapshot"
            );
            st.expanded_folder_ids.insert(parent_id);
            for component in report.components.iter().rev().skip(1) {
                st.expanded_folder_ids.insert(component.record.id);
            }
        }
        Self::expand_chain_to(&ctx.state, parent_id);

        // The created folder of the open folder waits after the media (nothing shifts) until the folder is opened again. It is
        // appended before the tree refresh: that refresh rebuilds the rows and refills the view (`on_tree_changed`).
        {
            let mut st = ctx.state.write();
            let in_open_folder = st.show_folders && st.is_plain_folder_view() && st.current_folder_id == Some(parent_id);
            if let (true, Some(top)) = (in_open_folder, report.components.first())
                && top.created_on_disk
            {
                st.folder_rows.append_created(top.record.id);
            }
        }
        FolderTreeSync::refresh(window, &ctx.library, &ctx.settings, &ctx.state);
        if let Some(first) = report.components.iter().find(|c| c.created_on_disk) {
            FolderTreeSync::scroll_to_folder(window, first.record.id);
        }
        BridgeMedia::update_virtual_folder_button_state(window, &ctx.library, &ctx.state);
    }
}
