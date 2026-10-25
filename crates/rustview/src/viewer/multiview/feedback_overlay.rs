//! The feedback icon (`media_feedback`) on a video tile, drawn by mpv itself.
//!
//! The video is a native child window on top of the Slint scene, so Slint cannot draw over it. mpv can: `overlay-add` puts a
//! bitmap on the OSD layer of the player, centered in its window, blended by mpv's own renderer. That needs no compositor,
//! no extra window and no X11 call, so it works the same on Wayland. The icon is the same SVG the rest of the application
//! uses, rasterized once per icon and size; a fade only changes the alpha of the copy that is handed to mpv.

use super::media_feedback::{FeedbackIcon, PulseView};
use crate::icons::{Icon, raster};
use crate::viewer::MpvPlayer;
use parking_lot::Mutex;
use rustview_core::util::cast::FloatToInt;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// What an overlay shows: the icon, how strong, the edge of the icon and the size of the window it is centered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayState {
    pub icon: FeedbackIcon,
    /// 0 = gone, 255 = full strength.
    pub alpha: u8,
    pub edge: u32,
    pub window: (u32, u32),
    /// Where that window starts inside mpv's window (the medium's place in the larger window of an unzoomed video).
    pub offset: (i64, i64),
}

impl OverlayState {
    pub fn new(pulse: PulseView, edge: u32, window: (u32, u32)) -> Self {
        Self {
            icon: match pulse.icon {
                FeedbackIcon::Rating(rating) => FeedbackIcon::Rating(rating.min(5)),
                other => other,
            },
            alpha: (pulse.alpha.clamp(0.0, 1.0) * 255.0).round().to_int::<u8>(),
            edge,
            window,
            offset: (0, 0),
        }
    }

    /// The same, centered in a window that starts at `offset` inside mpv's window.
    pub fn at(self, offset: (i64, i64)) -> Self {
        Self { offset, ..self }
    }

    /// Top left corner of the icon in mpv's window: the icon is centered in its window.
    pub fn origin(&self) -> (i64, i64) {
        let (x, y) = centered_origin(self.window, self.edge);
        (x + self.offset.0, y + self.offset.1)
    }
}

/// The corner that centers an `edge` x `edge` square in a window of `window` pixels (negative when the window is smaller).
pub fn centered_origin(window: (u32, u32), edge: u32) -> (i64, i64) {
    let center = |outer: u32| (i64::from(outer) - i64::from(edge)).div_euclid(2);
    (center(window.0), center(window.1))
}

/// The color of the disc behind the save icon (`Theme.status-ok`, the same as the canvas draws).
pub const SAVED_DISC: [u8; 3] = [0xa6, 0xe3, 0xa1];
/// The share of the disc the save icon covers (the canvas draws it at the same share).
pub const SAVED_ICON_SHARE: f64 = 0.56;

/// The color of the speaker of the mute feedback, dark on the white disc of the rating icons.
pub const SOUND_GLYPH: &str = "#1e1e2e";
/// The white disc behind the speaker (the disc of the rating icons).
pub const SOUND_DISC: [u8; 3] = [0xff, 0xff, 0xff];

/// Straight RGBA pixels of `icon`, rasterized once per size.
fn icon_pixels(icon: FeedbackIcon, edge: u32) -> Option<Arc<Vec<u8>>> {
    type Cache = Mutex<HashMap<(FeedbackIcon, u32), Arc<Vec<u8>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(found) = cache.lock().get(&(icon, edge)) {
        return Some(Arc::clone(found));
    }
    let rendered = match icon {
        FeedbackIcon::Rating(rating) => {
            raster::render_rgba(Icon::rating(rating), edge, "#ffffff").map(|pixels| over_white_disc(&pixels, edge))
        }
        FeedbackIcon::Saved => {
            let glyph = (f64::from(edge) * SAVED_ICON_SHARE).round().to_int::<u32>().max(1);
            raster::render_rgba(Icon::Save, glyph, "#ffffff").map(|pixels| on_disc(&pixels, glyph, edge, SAVED_DISC))
        }
        FeedbackIcon::Sound { muted } => {
            let glyph = (f64::from(edge) * SAVED_ICON_SHARE).round().to_int::<u32>().max(1);
            let speaker = if muted { Icon::VolumeOff } else { Icon::VolumeOn };
            raster::render_rgba(speaker, glyph, SOUND_GLYPH).map(|pixels| on_disc(&pixels, glyph, edge, SOUND_DISC))
        }
    };
    match rendered {
        Ok(pixels) => {
            let pixels = Arc::new(pixels);
            cache.lock().insert((icon, edge), Arc::clone(&pixels));
            Some(pixels)
        }
        Err(e) => {
            tracing::warn!(?icon, edge, error = %e, "[Multiview: Feedback] The icon could not be rasterized");
            None
        }
    }
}

/// A `glyph` x `glyph` icon centered on a disc of `color` that fills an `edge` x `edge` square (antialiased rim). Straight RGBA
/// in and out.
pub fn on_disc(icon: &[u8], glyph: u32, edge: u32, color: [u8; 3]) -> Vec<u8> {
    let center = f64::from(edge) / 2.0;
    let radius = center - 1.0;
    let offset = (edge.saturating_sub(glyph)) / 2;
    let mut out = Vec::with_capacity(usize::try_from(edge * edge * 4).unwrap_or(0));
    for y in 0..edge {
        for x in 0..edge {
            let (fx, fy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            let disc = (radius + 0.5 - (fx - center).hypot(fy - center)).clamp(0.0, 1.0);
            let (gx, gy) = (x.wrapping_sub(offset), y.wrapping_sub(offset));
            let px = (gx < glyph && gy < glyph)
                .then(|| usize::try_from((gy * glyph + gx) * 4).ok())
                .flatten()
                .and_then(|i| icon.get(i..i + 4));
            let icon_alpha = px.map_or(0.0, |p| f64::from(p[3]) / 255.0);
            let alpha = icon_alpha + disc * (1.0 - icon_alpha);
            if alpha <= 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let mix = |channel: usize| {
                let ic = px.map_or(0.0, |p| f64::from(p[channel]));
                ((ic * icon_alpha + f64::from(color[channel]) * disc * (1.0 - icon_alpha)) / alpha)
                    .round()
                    .to_int::<u8>()
            };
            out.extend_from_slice(&[mix(0), mix(1), mix(2), (alpha * 255.0).round().to_int::<u8>()]);
        }
    }
    out
}

/// The icon over a white disc of the icon's size (one pixel smaller than the icon, so no white rim shows around the colored
/// disc). The digit is cut out of the icon; the disc behind it makes the digit white instead of see-through. Straight RGBA in
/// and out.
pub fn over_white_disc(icon: &[u8], edge: u32) -> Vec<u8> {
    let center = f64::from(edge) / 2.0;
    let radius = center - 1.0;
    let mut out = Vec::with_capacity(icon.len());
    for (index, px) in icon.as_chunks::<4>().0.iter().enumerate() {
        let index = u32::try_from(index).unwrap_or(0);
        let (x, y) = (f64::from(index % edge) + 0.5, f64::from(index / edge) + 0.5);
        let disc = (radius + 0.5 - (x - center).hypot(y - center)).clamp(0.0, 1.0);
        let icon_alpha = f64::from(px[3]) / 255.0;
        let alpha = icon_alpha + disc * (1.0 - icon_alpha);
        if alpha <= 0.0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        let mix = |c: u8| {
            ((f64::from(c) * icon_alpha + 255.0 * disc * (1.0 - icon_alpha)) / alpha)
                .round()
                .to_int::<u8>()
        };
        out.extend_from_slice(&[
            mix(px[0]),
            mix(px[1]),
            mix(px[2]),
            (alpha * 255.0).round().to_int::<u8>(),
        ]);
    }
    out
}

/// Straight RGBA -> the premultiplied BGRA mpv wants, everything scaled by `alpha`.
pub fn premultiplied_bgra(rgba: &[u8], alpha: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len());
    for px in rgba.as_chunks::<4>().0 {
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
    let Some(rgba) = icon_pixels(state.icon, state.edge) else {
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
    fn a_hole_in_the_icon_becomes_white_and_the_outside_stays_empty() {
        let edge = 10u32;
        // a fully transparent icon: only the disc is left, white
        let blank = vec![0u8; (edge * edge * 4) as usize];
        let out = over_white_disc(&blank, edge);
        let at = |x: u32, y: u32| &out[((y * edge + x) * 4) as usize..][..4];
        assert_eq!(at(5, 5), &[255, 255, 255, 255]);
        assert_eq!(at(0, 0), &[0, 0, 0, 0]);
        // an opaque colored pixel keeps its color
        let mut icon = blank;
        icon[((5 * edge + 5) * 4) as usize..][..4].copy_from_slice(&[10, 200, 30, 255]);
        let out = over_white_disc(&icon, edge);
        assert_eq!(&out[((5 * edge + 5) * 4) as usize..][..4], &[10, 200, 30, 255]);
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
        let pulse = |icon, alpha| PulseView { icon, alpha };
        let state = OverlayState::new(pulse(FeedbackIcon::Rating(9), 0.5), 50, (100, 100));
        assert_eq!(state.icon, FeedbackIcon::Rating(5));
        assert_eq!(state.alpha, 128);
        assert_ne!(
            state,
            OverlayState::new(pulse(FeedbackIcon::Rating(5), 0.5), 50, (200, 100))
        );
        assert_ne!(
            state,
            OverlayState::new(pulse(FeedbackIcon::Saved, 0.5), 50, (100, 100))
        );
    }

    #[test]
    fn the_save_icon_sits_white_on_a_green_disc() {
        let (glyph, edge) = (4u32, 10u32);
        let mut icon = vec![0u8; (glyph * glyph * 4) as usize];
        // one opaque white pixel of the glyph, at its top left corner
        icon[..4].copy_from_slice(&[255, 255, 255, 255]);
        let out = on_disc(&icon, glyph, edge, SAVED_DISC);
        let at = |x: u32, y: u32| &out[((y * edge + x) * 4) as usize..][..4];
        assert_eq!(
            at(3, 3),
            &[255, 255, 255, 255],
            "the glyph is centered: (3, 3) is its corner"
        );
        assert_eq!(at(5, 6), &[SAVED_DISC[0], SAVED_DISC[1], SAVED_DISC[2], 255]);
        assert_eq!(at(0, 0), &[0, 0, 0, 0]);
    }
}
