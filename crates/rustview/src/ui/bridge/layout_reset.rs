//! The two reset tools of Settings > Debug (`docs/debug_page.md`): the main window back to its default size centered on its
//! monitor, and the layout of the main window (sidebars, inspector, panel tabs, table columns, grid or table, tab grid columns)
//! back to its defaults. Sort order, filters, settings, tags and the library stay.

use crate::system::SystemDisplayService;
use crate::ui::MainWindow;
use crate::ui::media::BridgeMedia;
use crate::ui::state::UiState;
use parking_lot::RwLock;
use rustview_core::settings::SettingsManager;
use rustview_core::settings::schema::{DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, ViewSettings};
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::SaturatingAs;
use slint::ComponentHandle;
use std::sync::Arc;

/// Unmaximizes the main window, gives it its default size and centers it on the monitor it is on (the monitors are looked up on
/// this click).
pub fn reset_window(window: &MainWindow) {
    let slint_window = window.window();
    slint_window.set_maximized(false);
    let (width, height) = (DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT);
    slint_window.set_size(slint::PhysicalSize::new(width, height));
    let position = slint_window.position();
    let size = slint_window.size();
    let center_x = position.x + (size.width / 2).saturating_as::<i32>();
    let center_y = position.y + (size.height / 2).saturating_as::<i32>();
    let monitors = SystemDisplayService::detect_monitors();
    let monitor = monitors
        .iter()
        .find(|m| m.contains_point(center_x, center_y))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .or_else(|| monitors.first());
    if let Some(m) = monitor {
        let x = m.x + (m.width.saturating_sub(width) / 2).saturating_as::<i32>();
        let y = m.y + (m.height.saturating_sub(height) / 2).saturating_as::<i32>();
        slint_window.set_position(slint::PhysicalPosition::new(x, y));
    }
    tracing::info!(
        width,
        height,
        centered = monitor.is_some(),
        "[SettingsDialog: Debug] Main window size and position reset"
    );
}

/// Puts the layout of the main window back to its defaults and saves it.
pub fn reset_layout(
    window: &MainWindow,
    settings: &Arc<SettingsManager>,
    state: &Arc<RwLock<UiState>>,
    thumbnails: &Arc<ThumbnailManager>,
) {
    let defaults = ViewSettings::default();
    let mut app_settings = settings.get();
    let view = &mut app_settings.view;
    let table_view_changed = view.is_table_view != defaults.is_table_view;
    view.sidebar_left_width = defaults.sidebar_left_width;
    view.sidebar_right_width = defaults.sidebar_right_width;
    view.is_inspector_collapsed = defaults.is_inspector_collapsed;
    view.left_sidebar_panel_tabs
        .clone_from(&defaults.left_sidebar_panel_tabs);
    view.left_sidebar_active_panel_tab
        .clone_from(&defaults.left_sidebar_active_panel_tab);
    view.tab_grid_columns = defaults.tab_grid_columns;
    view.tab_grid_auto_columns = defaults.tab_grid_auto_columns;
    view.dock = None;
    // Order, visibility and widths of the table's columns.
    crate::ui::table_columns::reset(window, view);
    settings.save_quiet(&app_settings);

    {
        let mut st = state.write();
        st.is_inspector_collapsed = defaults.is_inspector_collapsed;
        st.tab_grid_columns = defaults.tab_grid_columns;
        st.tab_grid_auto_columns = defaults.tab_grid_auto_columns;
    }
    window.set_sidebar_left_width(defaults.sidebar_left_width);
    window.set_sidebar_right_width(defaults.sidebar_right_width);
    window.set_tab_grid_columns(defaults.tab_grid_columns.saturating_as::<i32>());
    window.set_tab_grid_auto_columns(defaults.tab_grid_auto_columns);
    // The dock goes back to its default arrangement (it saves itself, `view.dock`).
    crate::ui::dock::reset_layout();
    if table_view_changed {
        BridgeMedia::handle_view_mode_toggled(defaults.is_table_view, 0, window, thumbnails, settings, state);
    }
    tracing::info!(
        table_view_changed,
        "[SettingsDialog: Debug] Layout of the main window reset to the defaults"
    );
}
