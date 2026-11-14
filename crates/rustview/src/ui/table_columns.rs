//! The order of the Browse table's columns: shown from `settings.json` at startup, changed by dragging a header
//! (`ui/media_table_columns.slint` does the dragging; a drop reports the old and the new display place) and stored at once.
//! The widths keep their own path (`view.col_*_width`, the window's `col-*-width` properties).

use crate::ui::{MainWindow, TableColumns};
use rustview_core::settings::SettingsManager;
use rustview_core::settings::table_columns::{display_order, move_column, ranks, stored_order};
use rustview_core::util::cast::SaturatingAs;
use slint::{ComponentHandle, ModelRc, VecModel};
use std::sync::Arc;

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

/// Takes the order from the settings and keeps it stored when a header is dropped at another place.
pub fn install(window: &MainWindow, settings: &Arc<SettingsManager>) {
    apply(window, &settings.get().view.table_column_order);

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
