//! A zoomed video (pure): mpv draws the picture itself, so the place the zoom model wants it at is turned into mpv's own view
//! properties. mpv first fits the video into its window (keeping the aspect, centered), then scales that by 2^`video-zoom` and
//! moves it by `video-pan-x`/`video-pan-y` times the scaled size (`~/claude/docs/mpv/src/DOCS/man/options.rst`, `--video-zoom`,
//! `--video-pan-x`; `video-align` stays 0 = centered).

use super::geometry::RectF;
use rustview_core::util::cast::Az;

/// mpv's view properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MpvView {
    /// `video-zoom`: log2 of the factor over the fitted size.
    pub zoom: f64,
    pub pan_x: f64,
    pub pan_y: f64,
}

impl MpvView {
    /// The plain fitted video.
    pub const FIT: Self = Self {
        zoom: 0.0,
        pan_x: 0.0,
        pan_y: 0.0,
    };

    /// Whether `other` shows the same picture (the zoom model works in fractions of pixels; mpv needs no finer steps).
    pub fn same_as(&self, other: &Self) -> bool {
        (self.zoom - other.zoom).abs() < 1e-4
            && (self.pan_x - other.pan_x).abs() < 1e-4
            && (self.pan_y - other.pan_y).abs() < 1e-4
    }
}

/// The view that shows a video of `video` pixels (its display size) at `rect` (pixels, relative to the window) in a window of
/// `window` pixels. `None` for sizes mpv cannot show.
pub fn view_for(video: (u32, u32), window: (i32, i32), rect: RectF) -> Option<MpvView> {
    let (vw, vh) = (f64::from(video.0), f64::from(video.1));
    let (ww, wh) = (window.0.az::<f64>(), window.1.az::<f64>());
    if vw <= 0.0 || vh <= 0.0 || ww <= 0.0 || wh <= 0.0 || rect.w <= 0.0 || rect.h <= 0.0 {
        return None;
    }
    let fit_w = vw * (ww / vw).min(wh / vh);
    let zoom = (rect.w / fit_w).log2();
    if !zoom.is_finite() {
        return None;
    }
    Some(MpvView {
        zoom,
        pan_x: (rect.x + rect.w / 2.0 - ww / 2.0) / rect.w,
        pan_y: (rect.y + rect.h / 2.0 - wh / 2.0) / rect.h,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &MpvView, b: &MpvView) -> bool {
        a.same_as(b)
    }

    #[test]
    fn the_fitted_centered_video_is_mpvs_plain_view() {
        // 1920 x 1080 in a 1000 x 1000 window: fitted to 1000 x 562.5, centered.
        let view = view_for((1920, 1080), (1000, 1000), RectF::new(0.0, 218.75, 1000.0, 562.5)).unwrap();
        assert!(close(&view, &MpvView::FIT), "{view:?}");
    }

    #[test]
    fn twice_the_size_around_the_center_is_one_step_of_video_zoom() {
        let view = view_for((1920, 1080), (1000, 1000), RectF::new(-500.0, -62.5, 2000.0, 1125.0)).unwrap();
        assert!(
            close(
                &view,
                &MpvView {
                    zoom: 1.0,
                    pan_x: 0.0,
                    pan_y: 0.0
                }
            ),
            "{view:?}"
        );
    }

    #[test]
    fn a_shifted_picture_pans_by_shares_of_its_own_size() {
        // Twice the fitted size, moved right by a quarter of its width and up by a tenth of its height.
        let rect = RectF::new(-500.0 + 500.0, -62.5 - 112.5, 2000.0, 1125.0);
        let view = view_for((1920, 1080), (1000, 1000), rect).unwrap();
        assert!(
            close(
                &view,
                &MpvView {
                    zoom: 1.0,
                    pan_x: 0.25,
                    pan_y: -0.1
                }
            ),
            "{view:?}"
        );
    }

    #[test]
    fn nothing_for_empty_sizes() {
        assert!(view_for((0, 1080), (100, 100), RectF::new(0.0, 0.0, 10.0, 10.0)).is_none());
        assert!(view_for((10, 10), (0, 100), RectF::new(0.0, 0.0, 10.0, 10.0)).is_none());
        assert!(view_for((10, 10), (100, 100), RectF::new(0.0, 0.0, 0.0, 10.0)).is_none());
    }
}
