//! The columns of the Browse table: their order, which ones are shown and the marks of the sort order in the headers.
//!
//! `ui/media_table_columns.slint` holds the geometry (the `TableColumns` global) and does the dragging; this module
//! fills it from `settings.json` at startup, stores a column that was dropped at another place, and has the menu of the
//! header (right click): one entry per column to show or hide it, and Reset Columns. The widths keep their own path
//! (`view.col_*_width`, the window's `col-*-width` properties).

use crate::ctx_menu::{CtxMenu, CtxMenuActionId, CtxMenuBuilder};
use crate::icons::Icon;
use crate::ui::context_menu::BridgeContextMenu;
use crate::ui::state::{CTX_TABLE_COLUMN_BASE, CTX_TABLE_COLUMNS_RESET, UiState};
use crate::ui::{MainWindow, TableColumns};
use crate::viewer::ViewerTabManager;
use parking_lot::RwLock;
use rustview_core::query::{SortKey, SortOrder};
use rustview_core::settings::table_columns::{
    TABLE_COLUMN_KEYS, display_order, move_column, ranks, set_column_shown, shown_columns, stored_order,
};
use rustview_core::settings::{SettingsManager, ViewSettings};
use rustview_core::util::cast::SaturatingAs;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::cell::Cell;
use std::sync::Arc;

thread_local! {
    /// Where the header's menu was opened: it comes back there after a column was shown or hidden.
    static MENU_AT: Cell<(f32, f32)> = const { Cell::new((0.0, 0.0)) };
}

fn int_model(values: &[usize]) -> ModelRc<i32> {
    ModelRc::new(VecModel::from(
        values
            .iter()
            .map(|&value| value.saturating_as::<i32>())
            .collect::<Vec<_>>(),
    ))
}

/// Shows the stored order in the table.
pub fn apply(window: &MainWindow, stored: &[String]) {
    let display = display_order(stored);
    let columns = window.global::<TableColumns>();
    columns.set_rank(int_model(&ranks(&display)));
    columns.set_order(int_model(&display));
}

/// Shows or hides the columns as the stored list of hidden ones says.
pub fn apply_shown(window: &MainWindow, hidden: &[String]) {
    window
        .global::<TableColumns>()
        .set_shown(ModelRc::new(VecModel::from(shown_columns(hidden))));
}

/// Marks the sort order in the headers of the table and in the sort bar of the thumbnail view.
pub fn apply_sort(window: &MainWindow, keys: &[SortKey]) {
    let mut levels = vec![0i32; TABLE_COLUMN_KEYS.len()];
    let mut descending = vec![false; TABLE_COLUMN_KEYS.len()];
    for (place, key) in keys.iter().enumerate() {
        let column: usize = key.column.to_index().saturating_as();
        if let (Some(level), Some(desc)) = (levels.get_mut(column), descending.get_mut(column)) {
            *level = (place + 1).saturating_as();
            *desc = key.order == SortOrder::Descending;
        }
    }
    let columns = window.global::<TableColumns>();
    columns.set_sort_level(ModelRc::new(VecModel::from(levels)));
    columns.set_sort_desc(ModelRc::new(VecModel::from(descending)));
    columns.set_sort_count(keys.len().saturating_as());
    if let Some(first) = keys.first() {
        window.set_sort_column_idx(first.column.to_index());
        window.set_sort_order_idx(first.order.to_index());
    }
}

/// Takes order and visibility from the settings and keeps the order stored when a header is dropped at another place.
pub fn install(window: &MainWindow, settings: &Arc<SettingsManager>) {
    let view = settings.get().view;
    apply(window, &view.table_column_order);
    apply_shown(window, &view.table_hidden_columns);

    let settings = Arc::clone(settings);
    let weak = window.as_weak();
    window.on_table_column_moved(move |from, to| {
        let Some(window) = weak.upgrade() else { return };
        let mut app_settings = settings.get();
        let mut display = display_order(&app_settings.view.table_column_order);
        move_column(&mut display, from.saturating_as::<usize>(), to.saturating_as::<usize>());
        let stored = stored_order(&display);
        if stored == app_settings.view.table_column_order {
            return;
        }
        tracing::debug!(from, to, "[TableColumns] A column was moved");
        apply(&window, &stored);
        app_settings.view.table_column_order = stored;
        settings.save_quiet_async(&app_settings);
    });
}

/// The menu of the header: the columns in their display order, each checked while it is shown, and Reset Columns.
fn header_menu(window: &MainWindow, view: &ViewSettings) -> CtxMenu {
    let columns = window.global::<TableColumns>();
    let shown = shown_columns(&view.table_hidden_columns);
    let mut builder = CtxMenuBuilder::new().header("Columns");
    for column in display_order(&view.table_column_order) {
        let title = columns.invoke_title(column.saturating_as());
        let id = CTX_TABLE_COLUMN_BASE + column.saturating_as::<CtxMenuActionId>();
        let is_shown = shown.get(column).copied().unwrap_or(true);
        builder = builder.checkable(id, title.as_str(), is_shown, None, None, true);
    }
    builder
        .separator()
        .action(
            CTX_TABLE_COLUMNS_RESET,
            "Reset Columns",
            None,
            Some(Icon::HotkeyReset),
            true,
        )
        .build()
}

/// A right click on the table's header at the window point (x, y).
pub fn show_header_menu(
    window: &MainWindow,
    state: &Arc<RwLock<UiState>>,
    tab_manager: &ViewerTabManager,
    settings: &SettingsManager,
    x: f32,
    y: f32,
) {
    MENU_AT.with(|at| at.set((x, y)));
    let menu = header_menu(window, &settings.get().view);
    BridgeContextMenu::open_custom_menu(window, state, tab_manager, menu, x, y);
}

/// Whether `action_id` belongs to the header's menu.
pub fn is_header_action(action_id: CtxMenuActionId) -> bool {
    (CTX_TABLE_COLUMN_BASE..=CTX_TABLE_COLUMNS_RESET).contains(&action_id)
}

/// An entry of the header's menu was chosen. Showing or hiding a column keeps the menu open (it comes back at its
/// place), so several columns are set in one go; Reset Columns closes it.
pub fn handle_header_action(
    action_id: CtxMenuActionId,
    window: &MainWindow,
    state: &Arc<RwLock<UiState>>,
    tab_manager: &ViewerTabManager,
    settings: &SettingsManager,
) {
    let mut app_settings = settings.get();
    if action_id == CTX_TABLE_COLUMNS_RESET {
        reset(window, &mut app_settings.view);
        settings.save_quiet_async(&app_settings);
        tracing::info!("[TableColumns] Order, visibility and widths reset to the defaults");
        return;
    }
    let column: usize = (action_id - CTX_TABLE_COLUMN_BASE).saturating_as();
    let now_shown = !shown_columns(&app_settings.view.table_hidden_columns)
        .get(column)
        .copied()
        .unwrap_or(true);
    if set_column_shown(&mut app_settings.view.table_hidden_columns, column, now_shown) {
        tracing::debug!(column, shown = now_shown, "[TableColumns] A column was shown or hidden");
        apply_shown(window, &app_settings.view.table_hidden_columns);
        settings.save_quiet_async(&app_settings);
    }
    let (x, y) = MENU_AT.with(Cell::get);
    let menu = header_menu(window, &app_settings.view);
    BridgeContextMenu::open_custom_menu(window, state, tab_manager, menu, x, y);
}

/// Puts order, visibility and widths of the columns back to the defaults (in `view` and in the window).
pub fn reset(window: &MainWindow, view: &mut ViewSettings) {
    let defaults = ViewSettings::default();
    view.table_column_order.clone_from(&defaults.table_column_order);
    view.table_hidden_columns.clone_from(&defaults.table_hidden_columns);
    view.col_filename_width = defaults.col_filename_width;
    view.col_path_width = defaults.col_path_width;
    view.col_modified_date_width = defaults.col_modified_date_width;
    view.col_created_date_width = defaults.col_created_date_width;
    view.col_size_width = defaults.col_size_width;
    view.col_rating_width = defaults.col_rating_width;
    view.col_tags_width = defaults.col_tags_width;
    view.col_width_width = defaults.col_width_width;
    view.col_height_width = defaults.col_height_width;
    view.col_ar_width = defaults.col_ar_width;
    view.col_duration_width = defaults.col_duration_width;
    view.col_format_width = defaults.col_format_width;
    view.col_flags_width = defaults.col_flags_width;

    window.set_col_filename_width(defaults.col_filename_width);
    window.set_col_path_width(defaults.col_path_width);
    window.set_col_modified_date_width(defaults.col_modified_date_width);
    window.set_col_created_date_width(defaults.col_created_date_width);
    window.set_col_size_width(defaults.col_size_width);
    window.set_col_rating_width(defaults.col_rating_width);
    window.set_col_tags_width(defaults.col_tags_width);
    window.set_col_width_width(defaults.col_width_width);
    window.set_col_height_width(defaults.col_height_width);
    window.set_col_ar_width(defaults.col_ar_width);
    window.set_col_duration_width(defaults.col_duration_width);
    window.set_col_format_width(defaults.col_format_width);
    window.set_col_flags_width(defaults.col_flags_width);
    apply(window, &view.table_column_order);
    apply_shown(window, &view.table_hidden_columns);
}
