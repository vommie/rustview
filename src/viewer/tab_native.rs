//! Native windows of tab payloads behind one interface: a video tab owns one mpv window, a Multiview tab owns one per
//! tile, an image tab none. The tab manager asks the payload instead of knowing what it owns.

use super::types::TabPayload;
use super::x11_embed::XRectangle;

impl TabPayload {
    /// The tab stops being the shown one: its windows are hidden and its players paused (the user's pause state is kept).
    pub fn deactivate_native(&self) {
        match self {
            Self::Video(vid) => {
                vid.player.set_view_active(false);
                if let Some(sub) = vid.subwindow.lock().as_mut() {
                    sub.hide();
                }
            }
            Self::Multiview(mv) => mv.instance.lock().deactivate(),
            Self::Image(_) => {}
        }
    }

    /// The tab grid is entered (`active == false`) or left (`active == true`). Videos switch their input mode and pause
    /// or resume; a Multiview is hidden either way and placed again by whoever shows it next.
    pub fn set_grid_view_active(&self, active: bool) {
        match self {
            Self::Video(vid) => {
                vid.player.set_tab_grid_mode(!active);
                vid.player.set_view_active(active);
                if !active && let Some(sub) = vid.subwindow.lock().as_mut() {
                    sub.hide();
                }
            }
            Self::Multiview(mv) => mv.instance.lock().deactivate(),
            Self::Image(_) => {}
        }
    }

    /// Cuts the areas of open menus and tooltips out of every visible window of the tab so they stay on top of the video.
    pub fn apply_overlay_cutouts(&self, cutouts: &[XRectangle]) {
        match self {
            Self::Video(vid) => {
                if let Some(sub) = vid.subwindow.lock().as_mut()
                    && sub.is_visible()
                {
                    if cutouts.is_empty() {
                        sub.reset_clip_shape();
                    } else {
                        sub.clip_cutout_rectangles(cutouts);
                    }
                }
            }
            Self::Multiview(mv) => mv.instance.lock().set_overlay_cutouts(cutouts),
            Self::Image(_) => {}
        }
    }

    /// The tab is closed: playback stops and the windows disappear. Memory is released by the last owner of the payload.
    pub fn stop_native(&self) {
        match self {
            Self::Video(vid) => {
                vid.player.stop();
                if let Some(sub) = vid.subwindow.lock().as_mut() {
                    sub.hide();
                }
            }
            Self::Multiview(mv) => mv.instance.lock().stop_and_hide(),
            Self::Image(_) => {}
        }
    }
}
