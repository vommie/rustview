//! Puts the media tabs at their places in the top bar (`strip_layout`) and keeps them there when the bar changes width.

use parking_lot::RwLock;
use rustview_core::util::cast::SaturatingAs as _;
use slint::Model as _;

use super::BridgeViewerTabs;
use super::strip_layout::StripLayout;
use crate::ui::MainWindow;
use crate::ui::state::UiState;

impl BridgeViewerTabs {
    /// Writes every tab row's place and the "+n" button from the strip's width; only rows that change are written.
    pub fn apply_strip_layout(window: &MainWindow, state: &RwLock<UiState>) {
        let tabs = window.get_tabs();
        let count = tabs.row_count();
        let active = (0..count).find(|&i| tabs.row_data(i).is_some_and(|t| t.is_active));
        let layout = StripLayout::new(count, active, state.read().tab_bar_width);
        for index in 0..count {
            let Some(mut row) = tabs.row_data(index) else {
                continue;
            };
            let (x, width, shown) = (layout.x(index), layout.tab_width, layout.is_shown(index));
            if (row.x - x).abs() > 0.01 || (row.width - width).abs() > 0.01 || row.shown != shown {
                row.x = x;
                row.width = width;
                row.shown = shown;
                tabs.set_row_data(index, row);
            }
        }
        window.set_tab_overflow_count(layout.hidden().saturating_as::<i32>());
        window.set_tab_overflow_x(layout.overflow_x.unwrap_or(0.0));
    }

    /// The strip of the top bar has another width (window resized, sidebars moved).
    pub fn handle_tab_bar_resized(width: f32, window: &MainWindow, state: &RwLock<UiState>) {
        let changed = {
            let mut st = state.write();
            let changed = (st.tab_bar_width - width).abs() > 0.5;
            st.tab_bar_width = width;
            changed
        };
        if changed {
            tracing::debug!(width, "[TabStrip] Bar width changed, tabs placed again");
            Self::apply_strip_layout(window, state);
        }
    }
}
