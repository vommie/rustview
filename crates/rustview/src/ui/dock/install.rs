//! The dock of the main window: built from `settings.json` at startup, saved after every change, owned by the UI
//! thread (the bridge's closures hold it weakly).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use rustview_core::settings::SettingsManager;

use super::bridge::DockBridge;
use super::model::DockLayout;
use super::persist::{self, LegacySidebars};
use crate::ui::MainWindow;

thread_local! {
    static DOCK: RefCell<Option<Rc<DockBridge>>> = const { RefCell::new(None) };
}

/// The layout `settings.json` holds (`view.dock`), or the one the fields of 0.6.13 and older describe.
pub fn stored_layout(settings: &SettingsManager) -> DockLayout {
    let app = settings.get();
    let view = &app.view;
    let legacy = persist::legacy_layout(&LegacySidebars {
        left_width: view.sidebar_left_width,
        right_width: view.sidebar_right_width,
        left_tabs: &view.left_sidebar_panel_tabs,
        left_active: &view.left_sidebar_active_panel_tab,
        info_collapsed: view.is_inspector_collapsed,
        history_collapsed: app.tags.is_history_collapsed,
    });
    if view.dock.is_none() {
        tracing::info!("[Dock] No stored layout yet: the sidebars of the older settings fields become the dock");
    }
    persist::from_json(view.dock.as_ref(), legacy)
}

/// Shows the stored layout in `window` and keeps it saved; `room_changed` runs after every change (the center may have
/// another size now).
pub fn install(window: &MainWindow, settings: &Arc<SettingsManager>, room_changed: impl Fn() + 'static) {
    let bridge = DockBridge::install(window, stored_layout(settings));
    let settings = Arc::clone(settings);
    bridge.on_change(move |layout| {
        tracing::debug!("[Dock] Layout changed");
        let mut app = settings.get();
        app.view.dock = persist::to_json(layout);
        // The older fields keep the widths (other code and older versions read them).
        app.view.sidebar_left_width = layout.left.width;
        app.view.sidebar_right_width = layout.right.width;
        settings.save_quiet_async(&app);
        room_changed();
    });
    DOCK.with(|dock| *dock.borrow_mut() = Some(bridge));
}

/// Puts the dock back to its default arrangement (Settings > Debug, "Reset Layout").
pub fn reset_layout() {
    let bridge = DOCK.with(|dock| dock.borrow().clone());
    if let Some(bridge) = bridge {
        bridge.reset();
    }
}

/// Lets the dock go (shutdown: its closures must not outlive the window).
pub fn release() {
    DOCK.with(|dock| dock.borrow_mut().take());
}
