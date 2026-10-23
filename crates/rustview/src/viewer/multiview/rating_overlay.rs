//! The rating icon on a video tile, drawn by mpv itself.
//!
//! The video is a native child window on top of the Slint scene, so Slint cannot draw over it. mpv can: `overlay-add` puts a
//! bitmap on the OSD layer of the player, centered in its window, blended by mpv's own renderer. That needs no compositor,
//! no extra window and no X11 call, so it works the same on Wayland. The icon is the same SVG the rest of the application
//! uses, rasterized once per rating and size; a fade only changes the alpha of the copy that is handed to mpv.

use super::rating_feedback::PulseView;
use crate::icons::{Icon, raster};
use crate::viewer::MpvPlayer;
use parking_lot::Mutex;
use rustview_core::util::cast::FloatToInt;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// What an overlay shows: the rating, how strong, the edge of the icon and the size of the window it is centered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayState {
    pub rating: u8,
    /// 0 = gone, 255 = full strength.
    pub alpha: u8,
    pub edge: u32,
    pub window: (u32, u32),
}

impl OverlayState {
    pub fn new(pulse: PulseView, edge: u32, window: (u32, u32)) -> Self {
        Self {
            rating: pulse.rating.min(5),
            alpha: (pulse.alpha.clamp(0.0, 1.0) * 255.0).round().to_int::<u8>(),
            edge,
            window,
        }
    }

    /// Top left corner of the icon in the window: the icon is centered.
    pub fn origin(&self) -> (i64, i64) {
        centered_origin(self.window, self.edge)
    }
}

/// The corner that centers an `edge` x `edge` square in a window of `window` pixels (negative when the window is smaller).
pub fn centered_origin(window: (u32, u32), edge: u32) -> (i64, i64) {
    let center = |outer: u32| (i64::from(outer) - i64::from(edge)).div_euclid(2);
    (center(window.0), center(window.1))
}

/// Straight RGBA pixels of the icon of `rating`, rasterized once per size.
fn icon_pixels(rating: u8, edge: u32) -> Option<Arc<Vec<u8>>> {
    static CACHE: OnceLock<Mutex<HashMap<(u8, u32), Arc<Vec<u8>>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(found) = cache.lock().get(&(rating, edge)) {
        return Some(Arc::clone(found));
    }
    match raster::render_rgba(Icon::rating(rating), edge, "#ffffff") {
        Ok(pixels) => {
            let pixels = Arc::new(pixels);
            cache.lock().insert((rating, edge), Arc::clone(&pixels));
            Some(pixels)
        }
        Err(e) => {
            tracing::warn!(rating, edge, error = %e, "[Multiview: Rating] The icon could not be rasterized");
            None
        }
    }
}

/// Straight RGBA -> the premultiplied BGRA mpv wants, everything scaled by `alpha`.
pub fn premultiplied_bgra(rgba: &[u8], alpha: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.chunks_exact(4) {
        let a = u32::from(px[3]) * u32::from(alpha) / 255;
        let scale = |c: u8| u8::try_from(u32::from(c) * a / 255).unwrap_or(u8::MAX);
        out.extend_from_slice(&[
            scale(px[2]),
            scale(px[1]),
            scale(px[0]),
            u8::try_from(a).unwrap_or(u8::MAX),
        ]);
    }
    out
}

/// Puts the icon on the player's picture. Returns whether mpv took it.
pub fn show(player: &MpvPlayer, state: &OverlayState) -> bool {
    let Some(rgba) = icon_pixels(state.rating, state.edge) else {
        return false;
    };
    let bitmap = premultiplied_bgra(&rgba, state.alpha);
    let (x, y) = state.origin();
    player.show_bitmap_overlay(&bitmap, state.edge, x, y)
}

/// Takes the icon off the player's picture.
pub fn hide(player: &MpvPlayer) {
    player.hide_bitmap_overlay();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_centered_in_every_window() {
        assert_eq!(centered_origin((1920, 1080), 50), (935, 515));
        assert_eq!(centered_origin((50, 50), 50), (0, 0));
        // odd leftovers round down, so the icon is at most half a pixel off center
        assert_eq!(centered_origin((101, 100), 50), (25, 25));
        assert_eq!(centered_origin((30, 40), 50), (-10, -5));
    }

    #[test]
    fn premultiplying_scales_color_and_alpha_and_swaps_to_bgra() {
        let rgba = [200, 100, 50, 255, 200, 100, 50, 0];
        assert_eq!(premultiplied_bgra(&rgba, 255), vec![50, 100, 200, 255, 0, 0, 0, 0]);
        let half = premultiplied_bgra(&rgba, 128);
        assert_eq!(half[3], 128);
        assert!(half[0] <= half[3] && half[1] <= half[3] && half[2] <= half[3]);
        assert_eq!(premultiplied_bgra(&rgba, 0), vec![0; 8]);
    }

    #[test]
    fn the_state_quantizes_the_alpha() {
        let state = OverlayState::new(PulseView { rating: 9, alpha: 0.5 }, 50, (100, 100));
        assert_eq!(state.rating, 5);
        assert_eq!(state.alpha, 128);
        assert_ne!(
            state,
            OverlayState::new(PulseView { rating: 5, alpha: 0.5 }, 50, (200, 100))
        );
    }
}
