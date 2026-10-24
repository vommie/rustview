//! The rating (and save) icon in the window of a player: an OSD bitmap overlay of mpv (`overlay-add`), see
//! `viewer::multiview::feedback_overlay`. The rating keys themselves reach the application through the keymap (`keys`).

use super::MpvPlayer;
use super::overlay::FEEDBACK_OVERLAY_ID;

impl MpvPlayer {
    /// Shows a square premultiplied BGRA bitmap of `edge` pixels with its top left corner at (`x`, `y`) of the window, on top of
    /// the picture. Returns whether mpv took it (it does not while no picture is set up).
    pub fn show_bitmap_overlay(&self, bgra: &[u8], edge: u32, x: i64, y: i64) -> bool {
        self.show_overlay(FEEDBACK_OVERLAY_ID, bgra, edge, edge, x, y)
    }

    /// Removes the bitmap overlay.
    pub fn hide_bitmap_overlay(&self) {
        self.remove_overlay(FEEDBACK_OVERLAY_ID);
    }
}
