//! Bitmaps that mpv draws over the picture of a player (`overlay-add`): the feedback icon of the Multiview, the corner marks
//! of a selected video tile and the volume bar. The video is a native window Slint cannot draw over, so mpv does it on its OSD
//! layer (no compositor, no X11 call; it works on Wayland and over a paused video too).

use super::MpvPlayer;

/// The overlay ids of a player (mpv knows 0..=63 per client), one place for all of them so that two uses never collide.
pub const FEEDBACK_OVERLAY_ID: u8 = 7;
/// The four corner marks of the selected tile (top left, top right, bottom left, bottom right).
pub const FOCUS_OVERLAY_IDS: [u8; 4] = [8, 9, 10, 11];
/// The volume bar after a change of the volume by the wheel or a key (`volume_overlay`).
pub const VOLUME_BAR_OVERLAY_ID: u8 = 12;

impl MpvPlayer {
    /// Shows `width` x `height` premultiplied BGRA pixels as overlay `id`, with the top left corner at (`x`, `y`) of the window.
    /// Returns whether mpv took it (it does not while no picture is set up). Pixels that do not cover the size are refused,
    /// because mpv reads `stride * height` bytes from the address it is given.
    pub fn show_overlay(&self, id: u8, bgra: &[u8], width: u32, height: u32, x: i64, y: i64) -> bool {
        let needed = u64::from(width) * u64::from(height) * 4;
        if width == 0 || height == 0 || u64::try_from(bgra.len()).map_or(true, |len| len < needed) {
            tracing::warn!(
                id,
                width,
                height,
                bytes = bgra.len(),
                "[MpvPlayer: Overlay] Refused: wrong size"
            );
            return false;
        }
        // mpv copies the pixels before the command returns, so the memory only has to live that long.
        let address = format!("&{}", bgra.as_ptr().addr());
        let args = [
            id.to_string(),
            x.to_string(),
            y.to_string(),
            address,
            "0".to_owned(),
            "bgra".to_owned(),
            width.to_string(),
            height.to_string(),
            (width * 4).to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.mpv.command("overlay-add", &refs) {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(id, error = ?e, "[MpvPlayer: Overlay] The overlay was not taken");
                false
            }
        }
    }

    /// Removes overlay `id` (nothing happens when it is not shown).
    pub fn remove_overlay(&self, id: u8) {
        let _ = self.mpv.command("overlay-remove", &[&id.to_string()]);
    }
}
