//! The volume bar (`volume_feedback`) on a video, drawn by mpv itself (`overlay-add`, like the feedback icon of
//! `feedback_overlay`): one bitmap with the speaker icon on a square of the bar's height, a small gap and the flat bar with the
//! level filled in. Both parts are semi-transparent; the fade scales the whole bitmap.

use super::feedback_overlay::premultiplied_bgra;
use super::video_bar::volume::VolumeIcon;
use super::volume_feedback::{VolumeBarGeometry, VolumeBarView};
use crate::icons::{Icon, raster};
use crate::viewer::MpvPlayer;
use crate::viewer::mpv_engine::overlay::VOLUME_BAR_OVERLAY_ID;
use parking_lot::Mutex;
use rustview_core::util::cast::FloatToInt;
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// The square behind the icon and the empty part of the bar: black, semi-transparent (the video shows through).
const BACKDROP: [u8; 4] = [0, 0, 0, 96];
/// The filled part of the bar and the icon: white, semi-transparent.
const FILL: [u8; 4] = [255, 255, 255, 160];
/// The filled part of the bar of a muted video: dimmed, like the control bar's slider.
const FILL_MUTED: [u8; 4] = [255, 255, 255, 80];
/// The share of the square the speaker covers.
const GLYPH_SHARE: f64 = 0.8;

/// What an overlay shows (only what changes the pixels, so an unchanged state is not sent to mpv again).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeOverlayState {
    pub geometry: VolumeBarGeometry,
    pub icon: VolumeIcon,
    /// The filled width of the bar, in pixels.
    pub filled: u32,
    pub muted: bool,
    /// 0 = gone, 255 = full strength.
    pub alpha: u8,
}

impl VolumeOverlayState {
    pub fn new(view: VolumeBarView, geometry: VolumeBarGeometry) -> Self {
        let filled = (f64::from(view.volume.share()) * f64::from(geometry.bar))
            .round()
            .to_int::<u32>()
            .min(geometry.bar);
        Self {
            geometry,
            icon: view.volume.icon(),
            filled,
            muted: view.volume.muted,
            alpha: (view.alpha.clamp(0.0, 1.0) * 255.0).round().to_int::<u8>(),
        }
    }
}

fn speaker(icon: VolumeIcon) -> Icon {
    match icon {
        VolumeIcon::Muted => Icon::VolumeOff,
        VolumeIcon::Low => Icon::VolumeLow,
        VolumeIcon::High => Icon::VolumeOn,
    }
}

/// Straight RGBA pixels of the speaker `icon` at `edge` pixels, white, rasterized once per icon and size.
fn glyph_pixels(icon: VolumeIcon, edge: u32) -> Option<Arc<Vec<u8>>> {
    type Cache = Mutex<HashMap<(i32, u32), Arc<Vec<u8>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(found) = cache.lock().get(&(icon.code(), edge)) {
        return Some(Arc::clone(found));
    }
    match raster::render_rgba(speaker(icon), edge, "#ffffff") {
        Ok(pixels) => {
            let pixels = Arc::new(pixels);
            cache.lock().insert((icon.code(), edge), Arc::clone(&pixels));
            Some(pixels)
        }
        Err(e) => {
            tracing::warn!(?icon, edge, error = %e, "[Multiview: VolumeBar] The icon could not be rasterized");
            None
        }
    }
}

/// The straight RGBA pixels of the bar of `state`, the speaker drawn from `glyph` (`glyph_edge` x `glyph_edge`, straight RGBA,
/// white) in the middle of its square.
pub fn compose(state: &VolumeOverlayState, glyph: &[u8], glyph_edge: u32) -> Vec<u8> {
    let g = state.geometry;
    let (width, height) = (g.rect.w, g.rect.h);
    let fill = if state.muted { FILL_MUTED } else { FILL };
    let bar_start = g.icon + g.gap;
    let inset = g.icon.saturating_sub(glyph_edge) / 2;
    let mut out = Vec::with_capacity(usize::try_from(u64::from(width) * u64::from(height) * 4).unwrap_or(0));
    for y in 0..height {
        for x in 0..width {
            let px = if x < g.icon {
                let (gx, gy) = (x.wrapping_sub(inset), y.wrapping_sub(inset));
                let glyph_px = (gx < glyph_edge && gy < glyph_edge)
                    .then(|| usize::try_from((gy * glyph_edge + gx) * 4).ok())
                    .flatten()
                    .and_then(|i| glyph.get(i..i + 4));
                let coverage = glyph_px.map_or(0, |p| p[3]);
                over(BACKDROP, [FILL[0], FILL[1], FILL[2], scale(FILL[3], coverage)])
            } else if x < bar_start {
                [0, 0, 0, 0]
            } else if x - bar_start < state.filled {
                fill
            } else {
                BACKDROP
            };
            out.extend_from_slice(&px);
        }
    }
    out
}

fn scale(a: u8, by: u8) -> u8 {
    u8::try_from(u32::from(a) * u32::from(by) / 255).unwrap_or(u8::MAX)
}

/// `top` over `bottom`, straight RGBA.
fn over(bottom: [u8; 4], top: [u8; 4]) -> [u8; 4] {
    let (ta, ba) = (f64::from(top[3]) / 255.0, f64::from(bottom[3]) / 255.0);
    let alpha = ta + ba * (1.0 - ta);
    if alpha <= 0.0 {
        return [0, 0, 0, 0];
    }
    let mix = |c: usize| {
        ((f64::from(top[c]) * ta + f64::from(bottom[c]) * ba * (1.0 - ta)) / alpha)
            .round()
            .to_int::<u8>()
    };
    [mix(0), mix(1), mix(2), (alpha * 255.0).round().to_int::<u8>()]
}

/// Puts the bar on the player's picture. Returns whether mpv took it.
pub fn show(player: &MpvPlayer, state: &VolumeOverlayState) -> bool {
    let glyph_edge = (f64::from(state.geometry.icon) * GLYPH_SHARE)
        .round()
        .to_int::<u32>()
        .max(1);
    let Some(glyph) = glyph_pixels(state.icon, glyph_edge) else {
        return false;
    };
    let bitmap = premultiplied_bgra(&compose(state, &glyph, glyph_edge), state.alpha);
    let rect = state.geometry.rect;
    player.show_overlay(VOLUME_BAR_OVERLAY_ID, &bitmap, rect.w, rect.h, rect.x, rect.y)
}

/// Takes the bar off the player's picture.
pub fn hide(player: &MpvPlayer) {
    player.remove_overlay(VOLUME_BAR_OVERLAY_ID);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::volume_feedback::{VolumeLevel, WindowRect, place_bar};

    fn state(level: f64, muted: bool) -> VolumeOverlayState {
        let geometry = place_bar(
            WindowRect {
                x: 0,
                y: 0,
                w: 1920,
                h: 1080,
            },
            1.0,
        )
        .expect("bar");
        VolumeOverlayState::new(
            VolumeBarView {
                volume: VolumeLevel { level, muted },
                alpha: 1.0,
            },
            geometry,
        )
    }

    #[test]
    fn the_level_fills_its_share_of_the_bar() {
        assert_eq!(state(50.0, false).filled, 120);
        assert_eq!(state(0.0, false).filled, 0);
        assert_eq!(state(150.0, false).filled, 240);
        assert_eq!(state(50.0, true).icon, VolumeIcon::Muted);
    }

    #[test]
    fn the_bitmap_has_the_icon_square_the_gap_and_the_bar() {
        let s = state(50.0, false);
        let (w, h) = (s.geometry.rect.w, s.geometry.rect.h);
        let glyph = vec![0u8; 4];
        let out = compose(&s, &glyph, 1);
        assert_eq!(out.len(), (w * h * 4) as usize);
        let at = |x: u32, y: u32| &out[((y * w + x) * 4) as usize..][..4];
        // the square behind the icon and the empty part of the bar: translucent black
        assert_eq!(at(0, 0), &BACKDROP);
        assert_eq!(at(w - 1, h - 1), &BACKDROP);
        // the gap: nothing
        assert_eq!(at(s.geometry.icon, 5), &[0, 0, 0, 0]);
        // the filled part: slightly transparent white
        let bar_start = s.geometry.icon + s.geometry.gap;
        assert_eq!(at(bar_start, 5), &FILL);
        assert_eq!(at(bar_start + 119, 5), &FILL);
        assert_eq!(at(bar_start + 120, 5), &BACKDROP);
        // a muted video dims the fill
        let muted = compose(&state(50.0, true), &glyph, 1);
        assert_eq!(&muted[((5 * w + bar_start) * 4) as usize..][..4], &FILL_MUTED);
    }

    #[test]
    fn the_glyph_sits_in_the_middle_of_its_square() {
        let s = state(50.0, false);
        let w = s.geometry.rect.w;
        // a 2 x 2 glyph with one opaque pixel at its top left corner
        let mut glyph = vec![0u8; 16];
        glyph[..4].copy_from_slice(&[255, 255, 255, 255]);
        let out = compose(&s, &glyph, 2);
        let inset = (s.geometry.icon - 2) / 2;
        let at = |x: u32, y: u32| &out[((y * w + x) * 4) as usize..][..4];
        assert_eq!(at(inset, inset), &over(BACKDROP, FILL));
        assert_eq!(at(inset + 1, inset), &BACKDROP);
    }
}
