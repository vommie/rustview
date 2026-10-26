//! The control bars of the videos in the main window (`video_bar`): the bars of the Multiview of the tab view, and where the events
//! of every bar of the main window go (the tab view's bars here, the small bars of the tab grid to `grid_bars`).

use super::video_bar::{self, VideoBarModel};
use crate::ui::{MainWindow, MultiviewTabBus, VideoBarBus};
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::video_bar::prefs;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::time::Instant;

thread_local! {
    /// The bars of the tab view.
    static BARS: RefCell<VideoBarModel> = RefCell::new(VideoBarModel::default());
}

/// Hands the bar model of the tab view to the canvas and routes the bar events of the main window. Once at startup.
pub(super) fn bind(window: &MainWindow) {
    BARS.with(|bars| window.global::<MultiviewTabBus>().set_video_bars(bars.borrow().model()));
    window.global::<VideoBarBus>().on_event(|key, event| {
        let Some(input) = video_bar::input_of(event) else {
            return;
        };
        match BARS.with(|bars| bars.borrow().tile(key)) {
            Some(tile) => super::tab_host::video_bar_after(|mv| mv.video_bar_input(tile, input, &prefs::current())),
            None => super::grid_bars::on_input(key, input),
        }
    });
}

/// Shows, updates or hides the control bars of the videos of the tab view (none while `active` is false). Returns whether one
/// is visible.
pub(super) fn update(window: &MainWindow, instance: &SharedInstance, active: bool) -> bool {
    let targets = if active {
        instance.lock().video_bar_targets(&prefs::current(), Instant::now())
    } else {
        instance.lock().release_video_bars();
        Vec::new()
    };
    let (visible, focus_back) = BARS.with(|bars| {
        let mut bars = bars.borrow_mut();
        let visible = if targets.is_empty() {
            bars.clear();
            false
        } else {
            bars.sync(&targets, &prefs::current())
        };
        (visible, bars.take_focus_back())
    });
    if focus_back {
        window.invoke_focus_keys();
    }
    visible
}

/// The tab view shows no Multiview any more: its bars go.
pub(super) fn clear(window: Option<&MainWindow>) {
    let focus_back = BARS.with(|bars| {
        let mut bars = bars.borrow_mut();
        bars.clear();
        bars.take_focus_back()
    });
    if focus_back && let Some(window) = window {
        window.invoke_focus_keys();
    }
}
