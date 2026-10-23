//! Rating keys and the rating icon in the window of a player.
//!
//! The native video window takes the keyboard itself, so Ctrl+0..5 are bound inside mpv and reported to the application as a
//! client message (like DEL, see `DELETE_KEY_MESSAGE`). The icon is an OSD bitmap overlay of mpv (`overlay-add`), see
//! `viewer::multiview::rating_overlay`.

use super::MpvPlayer;
use crate::viewer::multiview::rating_keys::mpv_key_names;
use libmpv2::Mpv;

/// The message mpv sends when a rating key is pressed: `rustview-rate <0..5>`.
pub(super) const RATE_KEY_MESSAGE: &str = "rustview-rate";

/// The id of the overlay (mpv knows 0..=63 per client); the rating icon is the only bitmap overlay of a player.
const OVERLAY_ID: &str = "7";

pub(super) fn bind_rating_keys(mpv: &Mpv) {
    for (key, rating) in mpv_key_names() {
        let command = format!("script-message {RATE_KEY_MESSAGE} {rating}");
        if let Err(e) = mpv.command("keybind", &[&key, &command]) {
            tracing::warn!(key, error = ?e, "[MpvPlayer: Rating] Could not bind the rating key");
        }
    }
}

/// The rating of a client message of mpv, `None` when it is another message.
pub(super) fn rating_of_message(args: &[&str]) -> Option<u8> {
    match args {
        [RATE_KEY_MESSAGE, rating] => rating.parse::<u8>().ok().filter(|r| *r <= 5),
        _ => None,
    }
}

impl MpvPlayer {
    /// Shows a square premultiplied BGRA bitmap of `edge` pixels with its top left corner at (`x`, `y`) of the window, on top of
    /// the picture. Returns whether mpv took it (it does not while no picture is set up).
    pub fn show_bitmap_overlay(&self, bgra: &[u8], edge: u32, x: i64, y: i64) -> bool {
        // mpv copies the pixels before the command returns, so the memory only has to live that long.
        let address = format!("&{}", bgra.as_ptr().addr());
        let args = [
            OVERLAY_ID.to_owned(),
            x.to_string(),
            y.to_string(),
            address,
            "0".to_owned(),
            "bgra".to_owned(),
            edge.to_string(),
            edge.to_string(),
            (edge * 4).to_string(),
        ];
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        match self.mpv.command("overlay-add", &refs) {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(error = ?e, "[MpvPlayer: Rating] The overlay was not taken");
                false
            }
        }
    }

    /// Removes the bitmap overlay.
    pub fn hide_bitmap_overlay(&self) {
        let _ = self.mpv.command("overlay-remove", &[OVERLAY_ID]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_rate_message_with_a_valid_rating_counts() {
        assert_eq!(rating_of_message(&["rustview-rate", "3"]), Some(3));
        assert_eq!(rating_of_message(&["rustview-rate", "0"]), Some(0));
        assert_eq!(rating_of_message(&["rustview-rate", "6"]), None);
        assert_eq!(rating_of_message(&["rustview-rate", "x"]), None);
        assert_eq!(rating_of_message(&["rustview-rate"]), None);
        assert_eq!(rating_of_message(&["rustview-delete"]), None);
    }
}
