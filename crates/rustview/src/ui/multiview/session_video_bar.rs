//! The control bars of the videos of the fullscreen (`video_bar`): shown on every tick, and what the user does on them or with
//! the mouse over a video (`RustView` takes the pointer there: the wheel changes the volume, a right click plays or pauses).

use super::session::{Session, with_session};
use super::video_bar::{self, VideoBarModel};
use crate::ui::{MultiviewFullscreenWindow, TooltipSettings, VideoBarBus};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::video_bar::prefs;
use crate::viewer::multiview::video_bar::view::VideoBarInput;
use rustview_core::settings::schema::AppearanceSettings;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::time::Instant;

thread_local! {
    /// The bars the fullscreen window draws (there is one fullscreen window, made once).
    static BARS: RefCell<VideoBarModel> = RefCell::new(VideoBarModel::default());
}

/// Hands the bar model to the window and its bar events to the session. Once, when the window is made.
pub(super) fn bind(win: &MultiviewFullscreenWindow) {
    BARS.with(|bars| win.set_video_bars(bars.borrow().model()));
    crate::ui::middle_elide::install(win);
    win.global::<VideoBarBus>().on_event(|key, event| {
        let tile = BARS.with(|bars| bars.borrow().tile(key));
        if let (Some(tile), Some(input)) = (tile, video_bar::input_of(event)) {
            with_session(|s| s.on_video_bar_input(tile, input));
        }
    });
}

/// The settings the elements of the bars read from globals of their window: the tooltips and the duration of hold to confirm.
pub(super) fn apply_settings(win: &MultiviewFullscreenWindow, appearance: &AppearanceSettings) {
    let tooltips = win.global::<TooltipSettings>();
    tooltips.set_enabled(appearance.enable_tooltips);
    tooltips.set_default_delay(i64::from(appearance.tooltip_delay_ms));
    crate::ui::hold_confirm::install(win, crate::ui::hold_confirm::current_ms());
}

impl Session {
    /// Shows, updates or hides the control bars of the videos (none while `active` is false: a dialog locks the view).
    /// Returns whether one is visible.
    pub(super) fn update_video_bars(&mut self, active: bool) -> bool {
        let targets = if active && self.populated {
            self.instance
                .lock()
                .video_bar_targets(&prefs::current(), Instant::now())
        } else {
            self.instance.lock().release_video_bars();
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
        if focus_back && let Some(win) = self.window.upgrade() {
            win.invoke_focus_keys();
        }
        visible
    }

    /// A click, drag or wheel on the bar of tile `key`.
    pub(super) fn on_video_bar_input(&mut self, key: TileKey, input: VideoBarInput) {
        if self.instance.lock().video_bar_input(key, input, &prefs::current()) {
            self.refresh();
            self.update_video_bars(true);
        }
        self.arm_requested_feedback();
    }

    /// The mouse wheel over the video of tile `index`: the volume, with Ctrl mpv's zoom.
    pub(super) fn video_wheel(&mut self, index: usize, delta: f32, ctrl: bool) {
        if self.instance.lock().video_wheel(index, delta, ctrl) {
            self.update_video_bars(true);
        }
        self.arm_requested_feedback();
    }

    /// A right click on the video of tile `index`: play or pause it.
    pub(super) fn video_right_click(&mut self, index: usize) {
        if self.instance.lock().video_right_click(index) {
            self.update_video_bars(true);
        }
    }
}

/// The session ends: no bar of the fullscreen stays.
pub(super) fn clear() {
    BARS.with(|bars| {
        let mut bars = bars.borrow_mut();
        bars.clear();
        bars.take_focus_back();
    });
}
