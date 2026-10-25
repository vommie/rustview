//! The two reset tools of Settings > Debug (`docs/debug_page.md`): the main window back to its default size centered on its
//! monitor, and the layout of the main window (sidebars, inspector, panel tabs, table columns, grid or table, tab grid columns)
//! back to its defaults. Sort order, filters, settings, tags and the library stay.

use crate::system::SystemDisplayService;
use crate::ui::MainWindow;
use crate::ui::media::BridgeMedia;
use crate::ui::sidebar::BridgeSidebarPanelTabs;
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
    settings.save_quiet(&app_settings);

    {
        let mut st = state.write();
        st.is_inspector_collapsed = defaults.is_inspector_collapsed;
        st.tab_grid_columns = defaults.tab_grid_columns;
        st.tab_grid_auto_columns = defaults.tab_grid_auto_columns;
        defaults
            .left_sidebar_active_panel_tab
            .clone_into(&mut st.active_sidebar_panel_tab);
    }
    window.set_sidebar_left_width(defaults.sidebar_left_width);
    window.set_sidebar_right_width(defaults.sidebar_right_width);
    window.set_info_is_collapsed(defaults.is_inspector_collapsed);
    window.set_tab_grid_columns(defaults.tab_grid_columns.saturating_as::<i32>());
    window.set_tab_grid_auto_columns(defaults.tab_grid_auto_columns);
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
    BridgeSidebarPanelTabs::sync_ui(window, settings, state);
    if table_view_changed {
        BridgeMedia::handle_view_mode_toggled(defaults.is_table_view, 0, window, thumbnails, settings, state);
    }
    tracing::info!(
        table_view_changed,
        "[SettingsDialog: Debug] Layout of the main window reset to the defaults"
    );
}
