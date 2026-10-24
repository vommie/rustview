//! The control bars of the videos of the fullscreen (`video_bar`): shown on every tick, and what the user does on them or with
//! the mouse over a video (`RustView` takes the pointer there: the wheel changes the volume, a right click plays or pauses).

use super::bar_overlay::BarOwner;
use super::session::Session;
use super::video_bar::{self, VideoBarHost};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::video_bar::view::VideoBarInput;
use std::time::Instant;

impl Session {
    /// Shows, updates or hides the control bars of the videos (none while `active` is false: a dialog locks the view).
    /// Returns whether one is visible.
    pub(super) fn update_video_bars(&mut self, active: bool) -> bool {
        if !active || !self.populated {
            self.instance.lock().release_video_bars();
            video_bar::hide(BarOwner::Fullscreen);
            return false;
        }
        let prefs = video_bar::effective_prefs();
        let targets = self.instance.lock().video_bar_targets(&prefs, Instant::now());
        let host = VideoBarHost {
            owner: BarOwner::Fullscreen,
            host_xid: self.fs_xid,
            on_input: self.video_bar_input.clone(),
        };
        video_bar::update(&host, &targets, &prefs)
    }

    /// A click, drag or wheel on the bar of tile `key`.
    pub(super) fn on_video_bar_input(&mut self, key: TileKey, input: VideoBarInput) {
        let prefs = video_bar::effective_prefs();
        if self.instance.lock().video_bar_input(key, input, &prefs) {
            self.refresh();
            self.update_video_bars(true);
        }
    }

    /// The mouse wheel over the video of tile `index`: the volume, with Ctrl mpv's zoom.
    pub(super) fn video_wheel(&mut self, index: usize, delta: f32, ctrl: bool) {
        if self.instance.lock().video_wheel(index, delta, ctrl) {
            self.update_video_bars(true);
        }
    }

    /// A right click on the video of tile `index`: play or pause it.
    pub(super) fn video_right_click(&mut self, index: usize) {
        if self.instance.lock().video_right_click(index) {
            self.update_video_bars(true);
        }
    }
}
