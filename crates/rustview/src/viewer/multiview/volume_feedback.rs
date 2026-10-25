//! The volume bar of a video (pure, the clock is passed in): after the mouse wheel or a video player key changed the volume of a
//! video, a flat bar with the level and a speaker icon of the bar's height shows near the bottom of the video for a moment. It
//! holds and fades exactly like the feedback pulse (`media_feedback`); a new change restarts it. A slider (the control bar's
//! volume slider) shows the level itself and starts none ([`VolumeInput::shows_bar`]); the same holds for mute, whose centered
//! icon (`FeedbackIcon::Sound`) only answers the key ([`MuteInput::shows_icon`]).

use super::follow::TileKey;
use super::media_feedback::{pulse_alpha, pulse_wakeup};
use super::video_bar::volume::{VolumeIcon, slider_share};
use rustview_core::settings::schema::{
    MULTIVIEW_VOLUME_BAR_BOTTOM_LOGICAL_PX, MULTIVIEW_VOLUME_BAR_BOTTOM_SHARE, MULTIVIEW_VOLUME_BAR_GAP_LOGICAL_PX,
    MULTIVIEW_VOLUME_BAR_HEIGHT_LOGICAL_PX, MULTIVIEW_VOLUME_BAR_WIDTH_LOGICAL_PX,
};
use rustview_core::util::cast::{Az, FloatToInt};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// What changed the volume of a video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolumeInput {
    /// The mouse wheel over the video or over its control bar.
    Wheel,
    /// A key of Settings > Hotkeys > Video Player.
    Hotkey,
    /// A slider (the volume slider of the control bar).
    Slider,
}

impl VolumeInput {
    /// Whether the change shows the volume bar: a slider shows the level itself.
    pub fn shows_bar(self) -> bool {
        matches!(self, Self::Wheel | Self::Hotkey)
    }
}

/// What muted or unmuted a video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MuteInput {
    /// A key of Settings > Hotkeys > Video Player.
    Hotkey,
    /// A button (media bar, primary bar "Mute All", the speaker of the control bar).
    Button,
}

impl MuteInput {
    /// Whether the change shows the centered speaker icon: a button shows its state itself.
    pub fn shows_icon(self) -> bool {
        self == Self::Hotkey
    }
}

/// The level a bar shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeLevel {
    /// Percent (mpv's `volume`).
    pub level: f64,
    pub muted: bool,
}

impl VolumeLevel {
    /// The filled share of the bar, like the control bar's slider (a level above its maximum is a full bar).
    pub fn share(self) -> f32 {
        slider_share(self.level)
    }

    pub fn icon(self) -> VolumeIcon {
        VolumeIcon::of(self.level, self.muted)
    }
}

/// What tile shows of its bar right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeBarView {
    pub volume: VolumeLevel,
    /// 1.0 = full strength, 0.0 = gone.
    pub alpha: f32,
}

#[derive(Debug, Clone, Copy)]
struct BarPulse {
    volume: VolumeLevel,
    started: Instant,
}

/// The volume bars of one Multiview, by tile.
#[derive(Debug, Default)]
pub struct VolumeFeedback {
    bars: HashMap<TileKey, BarPulse>,
}

impl VolumeFeedback {
    /// Shows `volume` on tile `tile`, restarting the bar that runs there.
    pub fn trigger(&mut self, tile: TileKey, volume: VolumeLevel, now: Instant) {
        self.bars.insert(tile, BarPulse { volume, started: now });
    }

    pub fn view(&self, tile: TileKey, now: Instant) -> Option<VolumeBarView> {
        let bar = self.bars.get(&tile)?;
        pulse_alpha(bar.started, now).map(|alpha| VolumeBarView {
            volume: bar.volume,
            alpha,
        })
    }

    /// Drops the bars that are over.
    pub fn expire(&mut self, now: Instant) {
        self.bars.retain(|_, bar| pulse_alpha(bar.started, now).is_some());
    }

    pub fn retain_tiles(&mut self, keep: impl Fn(TileKey) -> bool) {
        self.bars.retain(|tile, _| keep(*tile));
    }

    pub fn clear(&mut self) {
        self.bars.clear();
    }

    pub fn is_active(&self) -> bool {
        !self.bars.is_empty()
    }

    /// How long until a bar changes on screen, `None` when none is shown (see `MediaFeedback::wakeup`).
    pub fn wakeup(&self, now: Instant) -> Option<Duration> {
        self.bars.values().map(|bar| pulse_wakeup(bar.started, now)).min()
    }
}

/// A rectangle in the pixels of a video window, relative to its top left corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowRect {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

/// The picture of a video of `aspect` (width / height) in a window of `window` pixels, letterboxed by mpv: fitted and centered.
/// An unknown aspect is the whole window.
pub fn fitted_picture(window: (u32, u32), aspect: f32) -> WindowRect {
    let (w, h) = (f64::from(window.0), f64::from(window.1));
    let whole = WindowRect {
        x: 0,
        y: 0,
        w: window.0,
        h: window.1,
    };
    if !aspect.is_finite() || aspect <= 0.0 || window.0 == 0 || window.1 == 0 {
        return whole;
    }
    let aspect = f64::from(aspect);
    let (pw, ph) = if w / h > aspect {
        (h * aspect, h)
    } else {
        (w, w / aspect)
    };
    let (pw, ph) = (
        pw.round().to_int::<u32>().clamp(1, window.0),
        ph.round().to_int::<u32>().clamp(1, window.1),
    );
    WindowRect {
        x: i64::from((window.0 - pw) / 2),
        y: i64::from((window.1 - ph) / 2),
        w: pw,
        h: ph,
    }
}

/// Where the bar goes on a picture, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolumeBarGeometry {
    /// The whole bitmap: the icon, the gap and the bar, side by side.
    pub rect: WindowRect,
    /// The edge of the square icon (= the height of the bar).
    pub icon: u32,
    pub gap: u32,
    /// The width of the bar right of the icon.
    pub bar: u32,
}

/// The smallest bar, in heights of the icon: a narrower video makes everything smaller instead.
const MIN_BAR_IN_HEIGHTS: u32 = 2;
/// A bar lower than this (physical pixels) is not shown at all.
const MIN_HEIGHT_PX: u32 = 4;

/// The bar for `picture` at `scale` (physical pixels per logical pixel): horizontally centered on the picture in its lower part,
/// at most half the picture's width. `None` when the picture is too small to carry one.
pub fn place_bar(picture: WindowRect, scale: f32) -> Option<VolumeBarGeometry> {
    let scale = if scale.is_finite() && scale > 0.0 { scale } else { 1.0 };
    let px = |logical: f32| (logical * scale).round().to_int::<u32>();
    let (height, gap, bar) = (
        px(MULTIVIEW_VOLUME_BAR_HEIGHT_LOGICAL_PX).max(1),
        px(MULTIVIEW_VOLUME_BAR_GAP_LOGICAL_PX),
        px(MULTIVIEW_VOLUME_BAR_WIDTH_LOGICAL_PX),
    );
    let room = picture.w / 2;
    let (height, gap, bar) = if height + gap + bar <= room {
        (height, gap, bar)
    } else if height * (1 + MIN_BAR_IN_HEIGHTS) + gap <= room {
        (height, gap, room - height - gap)
    } else {
        // Everything shrinks together until the smallest bar fits.
        let factor = f64::from(room) / f64::from(height * (1 + MIN_BAR_IN_HEIGHTS) + gap);
        let shrink = |n: u32| (f64::from(n) * factor).floor().to_int::<u32>();
        let (h, g) = (shrink(height), shrink(gap));
        (h, g, room.saturating_sub(h + g))
    };
    if height < MIN_HEIGHT_PX || bar == 0 || picture.h < height * 2 {
        return None;
    }
    let width = height + gap + bar;
    let bottom = (picture.h.az::<f32>() * MULTIVIEW_VOLUME_BAR_BOTTOM_SHARE)
        .round()
        .to_int::<u32>()
        .max(px(MULTIVIEW_VOLUME_BAR_BOTTOM_LOGICAL_PX))
        // The bar stays in the lower half of a low video.
        .min(picture.h / 2 - height);
    Some(VolumeBarGeometry {
        rect: WindowRect {
            x: picture.x + i64::from((picture.w - width) / 2),
            y: picture.y + i64::from(picture.h - bottom - height),
            w: width,
            h: height,
        },
        icon: height,
        gap,
        bar,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn rect(x: i64, y: i64, w: u32, h: u32) -> WindowRect {
        WindowRect { x, y, w, h }
    }

    const LOUD: VolumeLevel = VolumeLevel {
        level: 80.0,
        muted: false,
    };

    #[test]
    fn the_wheel_and_the_keys_show_the_bar_a_slider_does_not() {
        assert!(VolumeInput::Wheel.shows_bar());
        assert!(VolumeInput::Hotkey.shows_bar());
        assert!(!VolumeInput::Slider.shows_bar());
    }

    #[test]
    fn only_the_mute_key_shows_the_speaker_icon() {
        assert!(MuteInput::Hotkey.shows_icon());
        assert!(!MuteInput::Button.shows_icon());
    }

    #[test]
    fn the_bar_holds_and_fades_like_the_feedback_pulse() {
        let t0 = Instant::now();
        let mut fb = VolumeFeedback::default();
        fb.trigger(1, LOUD, t0);
        assert_eq!(fb.view(1, t0 + ms(749)).map(|v| v.alpha), Some(1.0));
        let mid = fb.view(1, t0 + ms(875)).expect("fading").alpha;
        assert!((mid - 0.5).abs() < 1e-3, "{mid}");
        assert_eq!(fb.view(1, t0 + ms(1000)), None);
        assert_eq!(fb.wakeup(t0), Some(ms(750)));
        assert_eq!(fb.wakeup(t0 + ms(800)), Some(super::super::media_feedback::FRAME));
        fb.expire(t0 + ms(1000));
        assert!(!fb.is_active());
        assert_eq!(fb.wakeup(t0 + ms(1000)), None);
    }

    #[test]
    fn a_new_change_restarts_the_bar_with_the_new_level() {
        let t0 = Instant::now();
        let mut fb = VolumeFeedback::default();
        fb.trigger(1, LOUD, t0);
        let quieter = VolumeLevel {
            level: 75.0,
            muted: false,
        };
        fb.trigger(1, quieter, t0 + ms(900));
        assert_eq!(
            fb.view(1, t0 + ms(900)),
            Some(VolumeBarView {
                volume: quieter,
                alpha: 1.0
            })
        );
        assert_eq!(fb.view(1, t0 + ms(1600)).map(|v| v.alpha), Some(1.0));
        fb.retain_tiles(|tile| tile != 1);
        assert!(!fb.is_active());
    }

    #[test]
    fn the_share_and_icon_follow_the_control_bar() {
        assert!((LOUD.share() - 0.8).abs() < 1e-6);
        let loud = VolumeLevel {
            level: 130.0,
            muted: false,
        };
        assert!((loud.share() - 1.0).abs() < 1e-6);
        assert_eq!(
            VolumeLevel {
                level: 80.0,
                muted: true
            }
            .icon(),
            VolumeIcon::Muted
        );
        assert_eq!(
            VolumeLevel {
                level: 20.0,
                muted: false
            }
            .icon(),
            VolumeIcon::Low
        );
        assert_eq!(LOUD.icon(), VolumeIcon::High);
    }

    #[test]
    fn a_wide_video_gets_the_full_bar_centered_in_its_lower_part() {
        let g = place_bar(rect(0, 0, 1920, 1080), 1.0).expect("bar");
        assert_eq!((g.icon, g.gap, g.bar), (24, 4, 240));
        assert_eq!(g.rect.w, 268);
        assert_eq!(g.rect.x, (1920 - 268) / 2);
        // 12 % of the height above the bottom (130 px > 72 px)
        assert_eq!(g.rect.y, 1080 - 130 - 24);
        assert!(g.rect.y > 1080 / 2);
    }

    #[test]
    fn the_bar_follows_the_scale() {
        let g = place_bar(rect(0, 0, 3840, 2160), 2.0).expect("bar");
        assert_eq!((g.icon, g.gap, g.bar), (48, 8, 480));
        assert_eq!(g.rect.x, (3840 - 536) / 2);
    }

    #[test]
    fn the_bar_is_never_wider_than_half_the_video() {
        for width in [60u32, 120, 200, 300, 400, 536, 537, 960, 1920] {
            let picture = rect(10, 20, width, 400);
            let Some(g) = place_bar(picture, 1.0) else {
                continue;
            };
            assert!(g.rect.w <= width / 2, "width {width}: {g:?}");
            assert_eq!(g.rect.w, g.icon + g.gap + g.bar);
            // centered: equal space left and right (one pixel of rounding at most)
            let left = g.rect.x - picture.x;
            let right = i64::from(width) - left - i64::from(g.rect.w);
            assert!((left - right).abs() <= 1, "width {width}: {left} vs {right}");
        }
    }

    #[test]
    fn a_narrow_video_shortens_the_bar_then_shrinks_everything() {
        // half of 400 = 200: the icon keeps its height, the bar gets the rest
        let g = place_bar(rect(0, 0, 400, 300), 1.0).expect("bar");
        assert_eq!((g.icon, g.gap, g.bar), (24, 4, 172));
        // half of 120 = 60 < 3 * 24 + 4: everything shrinks
        let g = place_bar(rect(0, 0, 120, 300), 1.0).expect("bar");
        assert!(g.icon < 24 && g.bar >= g.icon * 2, "{g:?}");
        assert!(g.rect.w <= 60);
        assert_eq!(place_bar(rect(0, 0, 20, 300), 1.0), None);
    }

    #[test]
    fn a_low_video_keeps_the_bar_in_its_lower_half() {
        let g = place_bar(rect(0, 0, 800, 120), 1.0).expect("bar");
        assert!(g.rect.y >= 60 && g.rect.y + i64::from(g.rect.h) <= 120, "{g:?}");
        assert_eq!(place_bar(rect(0, 0, 800, 40), 1.0), None);
    }

    #[test]
    fn a_letterboxed_video_centers_the_bar_on_its_picture() {
        // a portrait video alone on a landscape display: mpv draws it in the middle
        let picture = fitted_picture((1920, 1080), 9.0 / 16.0);
        assert_eq!(picture, rect(656, 0, 608, 1080));
        let g = place_bar(picture, 1.0).expect("bar");
        assert!(g.rect.w <= 304);
        assert_eq!(g.rect.x, 656 + (608 - i64::from(g.rect.w)) / 2);
        // a wide video in a tall window: bars above and below
        assert_eq!(fitted_picture((1000, 1000), 2.0), rect(0, 250, 1000, 500));
        assert_eq!(fitted_picture((800, 600), f32::NAN), rect(0, 0, 800, 600));
    }
}
