//! What the user does with folders in the Browse view: clicking a folder row, opening it (double click, Enter), the "Show Folders"
//! toggle, the "New Folder" button state and what follows a created folder. Everything works on the in-memory folder snapshot.

use super::BrowseFolders;
use super::rows::FolderRowRef;
use super::selection::{BrowseItem, SelectionDelta};
use super::view::stamp_labels;
use crate::ui::MainWindow;
use crate::ui::browse_window::{
    DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, ViewGeometry, margin_rows_for, sync_window,
};
use crate::ui::folder_dialogs::FolderDialogContext;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::folder_tree::tree_sync::FolderTreeSync;
use crate::ui::media::BridgeMedia;
use crate::ui::media::inspector::BridgeInspector;
use crate::ui::multiview::handle::{self as multiview_handle, Pace};
use crate::ui::selection::BridgeFileSelection;
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::status_bar::StatusBarBridge;
use crate::ui::tags::BridgeTags;
use parking_lot::{Mutex, RwLock};
use rustview_core::library::Library;
use rustview_core::library::folder_create::FolderCreateReport;
use rustview_core::library::models::{FolderHierarchySnapshot, FolderId};
use rustview_core::settings::{LogObscurer, SettingsManager};
use rustview_core::thumbnails::ThumbnailManager;
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

        let (win, lib, thumb, set, st) = (
            window.clone_strong(),
            library.clone(),
            thumbnails.clone(),
            settings.clone(),
            state.clone(),
        );
        window.on_folder_row_clicked(move |id, ctrl, shift| {
            Self::handle_click(id, ctrl, shift, &win, &lib, &thumb, &set, &st)
        });

        // The date reader thread says that dates arrived: the tiles that show folders fill them in.
        {
            let (weak, st) = (window.as_weak(), state.clone());
            state.read().folder_times.set_notifier(Box::new(move || {
                let st = st.clone();
                let _ = weak.upgrade_in_event_loop(move |window| Self::refresh_folder_dates(&window, &st));
            }));
        }

        let (win, st) = (window.clone_strong(), state.clone());
        window.on_media_drag_over_row(move |id| Self::handle_drag_over_row(id, &win, &st));

        let (win, lib, thumb, set, st) = (window.clone_strong(), library, thumbnails, settings, state);
        window.on_show_folders_toggled(move |on| Self::set_show_folders(on, &win, &lib, &thumb, &set, &st));
    }

    /// A click on a folder row: plain selects it (and deselects the rest), Ctrl toggles it, Shift selects the range from the anchor,
    /// and the second plain click of a double click opens it.
    #[allow(clippy::too_many_arguments)]
    pub fn handle_click(
        id: i32,
        ctrl: bool,
        shift: bool,
        window: &MainWindow,
        library: &Arc<Library>,
        thumbnails: &Arc<ThumbnailManager>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let Some(row) = FolderRowRef::from_view_id(id) else {
            return;
        };

        if !ctrl && !shift {
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
        } else {
            *LAST_CLICK.lock() = None;
        }

        let delta = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            st.browse_click(BrowseItem::Folder(row), ctrl, shift)
        };
        Self::apply_selection_change(window, library, state, &delta);
    }

    /// Brings the tiles and the panels (inspector, tags, status bar) in line with a selection change.
    pub fn apply_selection_change(
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
        delta: &SelectionDelta,
    ) {
        BridgeFileSelection::sync_selection_delta(window, state, delta);
        BridgeInspector::update(window, state);
        BridgeTags::sync_tag_selection(window, library, state);
        StatusBarBridge::update_metrics(window, state);
    }

    /// The right click on a folder row: a row that is part of a selection of several items keeps that selection (the menu then
    /// acts on all of them), any other row becomes the only selected item.
    pub fn select_row_for_menu(
        row: FolderRowRef,
        window: &MainWindow,
        library: &Arc<Library>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let delta = {
            let mut st = state.write();
            st.last_active_panel = LastActivePanel::Media;
            let (folders, files) = st.browse_selection_counts();
            if st.folder_rows.is_selected(row) && folders + files > 1 {
                return;
            }
            st.browse_click(BrowseItem::Folder(row), false, false)
        };
        Self::apply_selection_change(window, library, state, &delta);
    }

    /// Opens the folder behind a row: the folder is shown like a click in the folder tree would show it, and the tree reveals it.
    /// Going up (the "..." row) selects the first element of the folder that is shown then.
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
            id.and_then(|id| {
                st.folder_hierarchy
                    .as_ref()
                    .and_then(|s| s.get_node(id))
                    .map(|n| (id, n.path.clone()))
            })
        };
        let Some((folder_id, path)) = target else {
            tracing::debug!(
                ?row,
                "[BrowseFolders: Open] The folder is not in the snapshot; nothing to open"
            );
            return;
        };
        tracing::info!(folder = %LogObscurer::folder(Some(folder_id), &path), up = row == FolderRowRef::Up, "[BrowseFolders: Open] Opening a folder from the media view");

        if row == FolderRowRef::Up {
            state.write().pending_first_select = Some(folder_id);
        } else {
            // The tree shows the folder below its parent: the parent must be expanded for the new selection to be visible.
            Self::expand_chain_to(state, folder_id);
        }
        BridgeFolders::select_folder(folder_id, path, window, library, thumbnails, settings, state);
    }

    /// Enter: opens the folder row that is selected alone ("..." included). `false` when something else is selected.
    pub fn open_selected_row(
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) -> bool {
        let row = {
            let st = state.read();
            if st.file_selection.count() > 0 {
                return false;
            }
            st.folder_rows.single_selected()
        };
        let (Some(row), Some(thumbnails)) = (row, THUMBNAILS.get()) else {
            return false;
        };
        Self::open_row(row, window, library, thumbnails, settings, state);
        true
    }

    /// Backspace and Alt+Up: shows the parent of the open folder (nothing at the root).
    pub fn go_up(
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let target = {
            let st = state.read();
            if !st.is_plain_folder_view() {
                return;
            }
            let Some(snapshot) = st.folder_hierarchy.as_ref() else {
                return;
            };
            let node = st.current_folder_id.and_then(|id| snapshot.get_node(id));
            node.filter(|n| !snapshot.root_ids.contains(&n.id))
                .and_then(|n| n.parent_id)
                .filter(|p| snapshot.nodes.contains_key(p))
        };
        let (Some(parent), Some(thumbnails)) = (target, THUMBNAILS.get()) else {
            return;
        };
        let path = state
            .read()
            .folder_hierarchy
            .as_ref()
            .and_then(|s| s.get_node(parent))
            .map(|n| n.path.clone());
        if let Some(path) = path {
            state.write().pending_first_select = Some(parent);
            BridgeFolders::select_folder(parent, path, window, library, thumbnails, settings, state);
        }
    }

    /// After going up: the first element of the folder that is shown now is selected ("..." when there is one, else the first folder
    /// or medium). The request belongs to one folder; a request for any other folder is stale and dropped.
    pub fn apply_pending_first_select(window: &MainWindow, state: &Arc<RwLock<UiState>>, folder_id: FolderId) {
        let delta = {
            let mut st = state.write();
            match st.pending_first_select.take() {
                Some(target) if target == folder_id => {}
                _ => return,
            }
            let Some(first) = st.browse_item_at(0) else { return };
            st.last_active_panel = LastActivePanel::Media;
            st.browse_click(first, false, false)
        };
        BridgeFileSelection::sync_selection_delta(window, state, &delta);
        if !delta.media.is_empty() {
            multiview_handle::on_user_selection_changed(Pace::Settled);
        }
    }

    /// Alt+Left (`direction` < 0) and Alt+Right: back / forward through the folders the user visited.
    pub fn go_history(
        direction: i32,
        window: &MainWindow,
        library: &Arc<Library>,
        settings: &Arc<SettingsManager>,
        state: &Arc<RwLock<UiState>>,
    ) {
        let Some(thumbnails) = THUMBNAILS.get() else { return };
        let target = {
            let mut st = state.write();
            let (Some(current), Some(snapshot)) = (st.current_folder_id, st.folder_hierarchy.clone()) else {
                return;
            };
            let valid = |id: FolderId| snapshot.get_node(id).is_some_and(|n| !n.is_deleted);
            let step = if direction < 0 {
                st.nav_history.step_back(current, valid)
            } else {
                st.nav_history.step_forward(current, valid)
            };
            step.and_then(|id| snapshot.get_node(id).map(|n| (id, n.path.clone())))
        };
        let Some((folder_id, path)) = target else {
            StatusBarBridge::info(
                window,
                if direction < 0 {
                    "No earlier folder."
                } else {
                    "No later folder."
                },
            );
            return;
        };
        Self::expand_chain_to(state, folder_id);
        state.write().last_active_panel = LastActivePanel::Media;
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

    /// Updates the selection highlight of the folder tiles in the pool (O(pool), whatever the number of selected folders).
    pub fn refresh_folder_tiles(window: &MainWindow, state: &RwLock<UiState>) {
        let st = state.read();
        let model = window.get_media_items();
        for slot in 0..model.row_count() {
            let Some(mut view) = model.row_data(slot) else { continue };
            let Some(row) = FolderRowRef::from_view_id(view.id).filter(|_| view.is_folder) else {
                continue;
            };
            let selected = st.folder_rows.is_selected(row);
            if view.is_selected != selected {
                view.is_selected = selected;
                model.set_row_data(slot, view);
            }
        }
    }

    /// Fills the dates of the folder tiles in the pool that were requested and have arrived (O(pool)).
    pub fn refresh_folder_dates(window: &MainWindow, state: &RwLock<UiState>) {
        let st = state.read();
        let model = window.get_media_items();
        for slot in 0..model.row_count() {
            let Some(mut view) = model.row_data(slot) else { continue };
            if !view.is_folder || view.is_folder_up || view.is_deleted {
                continue;
            }
            let Some(FolderRowRef::Folder(id)) = FolderRowRef::from_view_id(view.id) else {
                continue;
            };
            let (modified, created) = stamp_labels(st.folder_times.peek(id));
            if view.modified_date_str != modified || view.created_date_str != created {
                view.modified_date_str = modified;
                view.created_date_str = created;
                model.set_row_data(slot, view);
            }
        }
    }

    /// Drops the highlight of every folder row (a medium was selected on its own, or the selection was cleared).
    pub fn clear_folder_selection(window: &MainWindow, state: &RwLock<UiState>) {
        let changed = {
            let mut st = state.write();
            let had = st.folder_rows.has_selection();
            st.folder_rows.clear_selection();
            had
        };
        if changed {
            Self::refresh_folder_tiles(window, state);
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
        window.set_show_folders(on);
        {
            let mut st = state.write();
            st.show_folders = on;
            st.last_active_panel = LastActivePanel::Media;
            // Showing the folders anew puts every folder in name order again, the "Up" entry first.
            st.folder_rows.reset_tail();
            st.folder_rows.clear_selection();
        }
        let mut app_settings = settings.get();
        app_settings.view.show_folders_in_browse = on;
        settings.save_quiet_async(&app_settings);

        Self::refresh_rows(window, library, thumbnails, settings, state);
        BridgeInspector::update(window, state);
        StatusBarBridge::update_metrics(window, state);
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
            // The rows may have taken selected folders with them.
            BridgeInspector::update(window, state);
            StatusBarBridge::update_metrics(window, state);
        }
    }

    /// The folder tree was rebuilt or patched (rename, move, delete, refresh, new folder): the folder rows follow. Called from
    /// `FolderTreeSync::refresh`, which has no thumbnail manager of its own, hence the one stored at registration.
    pub fn on_tree_changed(
        window: &MainWindow,
        library: &Library,
        settings: &SettingsManager,
        state: &RwLock<UiState>,
    ) {
        if let Some(thumbnails) = THUMBNAILS.get() {
            Self::refresh_rows(window, library, thumbnails, settings, state);
        }
    }

    /// Refills the whole tile pool for the list as it is now, around the rows that were shown.
    pub fn resync_window(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>) {
        let is_table = window.get_is_table_view();
        let mut st = state.write();
        let geometry = st.view_geometry.unwrap_or_else(|| {
            ViewGeometry::new(DEFAULT_GRID_COLUMNS, DEFAULT_VISIBLE_ROWS, margin_rows_for(is_table))
        });
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
            let missing = st
                .folder_hierarchy
                .as_ref()
                .and_then(|s| s.get_node(id))
                .is_some_and(|n| n.is_deleted);
            missing.then_some("The folder does not exist on disk")
        } else {
            Some("No folder selected")
        };
        drop(st);

        window.set_is_new_folder_btn_enabled(reason.is_none());
        window.set_new_folder_tooltip(
            match reason {
                Some(reason) => format!("{DESCRIPTION}\nDisabled: {reason}"),
                None => DESCRIPTION.to_string(),
            }
            .into(),
        );
    }

    /// Everything that follows folders that were created below `parent_id`: the snapshot gets them (no rescan), the tree shows them
    /// (ancestors expand), and a created folder of the open folder waits at the end of the Browse list until the folder is opened again.
    pub fn folders_created(
        window: &MainWindow,
        ctx: &FolderDialogContext,
        parent_id: FolderId,
        report: &FolderCreateReport,
    ) {
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
            let in_open_folder =
                st.show_folders && st.is_plain_folder_view() && st.current_folder_id == Some(parent_id);
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
