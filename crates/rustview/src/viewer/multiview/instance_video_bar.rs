//! The control bars of the video tiles (`video_bar`): which bars the host shows on this tick and with what, and what a click,
//! drag or wheel on a bar does. The mouse over a video is `RustView`'s (its window lets the pointer through to the host), so
//! the wheel and the right button over a video come here as well.

use super::instance::MultiviewInstance;
use super::instance_zoom::WHEEL_STEP_DELTA;
use super::layout::Rect;
use super::model::TileKind;
use super::route::HostMode;
use super::tile::VideoTile;
use super::video_bar::placement::{self, VideoBarPlacement};
use super::video_bar::prefs::{RevealMode, VideoBarPrefs};
use super::video_bar::progress::ProgressTrack;
use super::video_bar::reveal::{Phase, RevealInput};
use super::video_bar::time_text;
use super::video_bar::view::{DragPhase, VideoBarInput, VideoBarTarget, VideoBarView};
use super::video_bar::volume::{self, VolumeIcon};
use super::volume_feedback::VolumeInput;
use crate::viewer::mpv_engine::SeekPrecision;
use crate::viewer::multiview::follow::TileKey;
use rustview_core::library::models::MediaId;
use rustview_core::settings::schema::{
    MULTIVIEW_VIDEO_BAR_FADE_MS, MULTIVIEW_VIDEO_BAR_IDLE_MS, MULTIVIEW_VIDEO_BAR_REVEAL_MARGIN_LOGICAL_PX,
    MULTIVIEW_VIDEO_WHEEL_VOLUME_STEP,
};
use rustview_core::util::cast::{Az, FloatToInt};
use std::time::{Duration, Instant};

impl MultiviewInstance {
    /// Whether the host shows the bars of its videos: the fullscreen and the tab view, not while a dialog or menu is open.
    fn video_bars_allowed(&self) -> bool {
        matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView)
            && !self.host.modal_locked
            && !self.host.menu_open
    }

    /// The control bars to show (or to let fade out) on this tick, one per video at most, with their content.
    pub fn video_bar_targets(&mut self, prefs: &VideoBarPrefs, now: Instant) -> Vec<VideoBarTarget> {
        let allowed = self.video_bars_allowed();
        let scale = self.host.surface.dpi.max(0.5);
        self.collect_bar_targets(allowed, prefs, (prefs.reveal, false), now, |rect| {
            placement::place(rect, prefs.style, prefs.height, scale)
        })
    }

    /// The bars of every video tile that `place` gives a place: `kind` is how they reveal and whether they are the small bars
    /// of a tab grid item. With `allowed` false none shows and every drag on one ends.
    pub(super) fn collect_bar_targets(
        &mut self,
        allowed: bool,
        prefs: &VideoBarPrefs,
        kind: (RevealMode, bool),
        now: Instant,
        place: impl Fn(Rect) -> Option<VideoBarPlacement>,
    ) -> Vec<VideoBarTarget> {
        let (mode, mini) = kind;
        let scale = self.host.surface.dpi.max(0.5);
        let margin = (MULTIVIEW_VIDEO_BAR_REVEAL_MARGIN_LOGICAL_PX * scale)
            .round()
            .to_int::<i32>();
        let fade = prefs.fade.then(|| Duration::from_millis(MULTIVIEW_VIDEO_BAR_FADE_MS));
        let pointer = self.pointer;
        let mut targets = Vec::new();
        for index in 0..self.tiles.len() {
            let video_rect = self.video_window_rect(index);
            let tile = &mut self.tiles[index];
            let (key, media_id, broken) = (tile.key, tile.media_id, tile.error.is_some());
            let Some(video) = tile.video.as_mut().filter(|_| tile.kind == TileKind::Video) else {
                continue;
            };
            let (Some(rect), Some(place)) = (video_rect, video_rect.and_then(&place)) else {
                video.bar.release();
                continue;
            };
            if !allowed || broken {
                video.bar.release();
                continue;
            }
            let phase = video.bar.reveal.decide(
                &RevealInput {
                    mode,
                    video: rect,
                    bar: place.bar,
                    pointer,
                    margin_px: margin,
                    forced: video.bar.dragging(),
                    idle: Duration::from_millis(MULTIVIEW_VIDEO_BAR_IDLE_MS),
                    fade,
                },
                now,
            );
            if phase == Phase::Hidden {
                video.bar.set_hover(None);
                continue;
            }
            targets.push(VideoBarTarget {
                key,
                placement: place,
                phase,
                held: video.bar.dragging(),
                mini,
                view: bar_view(video, media_id, prefs, now),
            });
        }
        targets
    }

    /// The user did something on the bar of tile `key`. Every press on a bar selects its tile, like a click on its media
    /// bar. Returns whether the view has to be refreshed.
    pub fn video_bar_input(&mut self, key: TileKey, input: VideoBarInput, prefs: &VideoBarPrefs) -> bool {
        let Some(index) = self.tile_index(key) else {
            return false;
        };
        let pressed = matches!(
            input,
            VideoBarInput::PlayPause
                | VideoBarInput::ToggleMs
                | VideoBarInput::Speaker
                | VideoBarInput::Seek {
                    phase: DragPhase::Press,
                    ..
                }
                | VideoBarInput::Volume {
                    phase: DragPhase::Press,
                    ..
                }
        );
        let selected = pressed && self.select_tile(key);
        let media_id = self.tiles[index].media_id;
        match input {
            VideoBarInput::Speaker => {
                self.speaker_click_at(index);
                return true;
            }
            VideoBarInput::Wheel { delta_y } => return self.video_wheel(index, delta_y, false) || selected,
            _ => {}
        }
        let Some(video) = self.tiles[index].video.as_mut() else {
            return selected;
        };
        let player = video.player.clone();
        let result = match input {
            VideoBarInput::PlayPause => player.toggle_pause(),
            VideoBarInput::ToggleMs => {
                video.bar.toggle_ms(media_id, prefs.show_ms);
                tracing::debug!(media_id, "[Multiview: VideoBar] Milliseconds switched");
                Ok(())
            }
            VideoBarInput::Seek { share, phase } => {
                let secs = ProgressTrack::new(player.playback().duration, 0.0).time_at(share);
                let precision = prefs.seek.seek_for(phase);
                let exact = precision.map(|p| p == SeekPrecision::Exact);
                video
                    .bar
                    .seek_to(share, secs, Instant::now(), phase == DragPhase::Release, exact);
                if phase != DragPhase::Move {
                    tracing::debug!(media_id, ?phase, ?precision, secs, "[Multiview: VideoBar] Seek");
                }
                precision.map_or(Ok(()), |precision| player.seek_to(secs, precision))
            }
            VideoBarInput::Hover { share } => {
                video.bar.set_hover(share);
                Ok(())
            }
            VideoBarInput::Volume { share, phase } => {
                video.bar.set_volume_drag(phase != DragPhase::Release);
                let level = volume::level_at(share);
                if phase == DragPhase::Release {
                    video.bar.note_level(level);
                    tracing::debug!(media_id, level, "[Multiview: VideoBar] Volume set");
                }
                player.set_volume(level)
            }
            // Handled above.
            VideoBarInput::Wheel { .. } | VideoBarInput::Speaker => Ok(()),
        };
        if let Err(e) = result {
            tracing::warn!(media_id, ?input, error = %e, "[Multiview: VideoBar] The player did not take it");
        }
        true
    }

    /// A click on the speaker of tile `index`: see `volume::speaker_click`.
    fn speaker_click_at(&mut self, index: usize) {
        let Some(video) = self.tiles[index].video.as_mut() else {
            return;
        };
        let level = video.player.playback().volume;
        let click = volume::speaker_click(level, video.last_mute, video.bar.last_audible());
        if let Some(level) = click.level
            && let Err(e) = video.player.set_volume(level)
        {
            tracing::warn!(error = %e, "[Multiview: VideoBar] The level could not be restored");
        }
        tracing::info!(
            media_id = self.tiles[index].media_id,
            toggle_mute = click.toggle_mute,
            restored = ?click.level,
            "[Multiview: VideoBar] Speaker clicked"
        );
        if click.toggle_mute {
            self.toggle_mute_at(index);
        }
    }

    /// The mouse wheel over the video of tile `index` or over its bar: louder or quieter by one step per notch, like mpv did;
    /// with Ctrl (`zoom`) mpv zooms the video by the zoom step instead.
    pub fn video_wheel(&mut self, index: usize, delta_y: f32, zoom: bool) -> bool {
        if !delta_y.is_finite() || delta_y == 0.0 {
            return false;
        }
        // Whole notches only (a touchpad sends small steps); a change of direction starts over.
        if self.video_wheel_accum * delta_y < 0.0 {
            self.video_wheel_accum = 0.0;
        }
        self.video_wheel_accum += delta_y;
        let notches = (self.video_wheel_accum.abs() / WHEEL_STEP_DELTA)
            .floor()
            .to_int::<i32>();
        if notches == 0 {
            return false;
        }
        let up = self.video_wheel_accum > 0.0;
        self.video_wheel_accum -= self.video_wheel_accum.signum() * WHEEL_STEP_DELTA * notches.az::<f32>();
        let Some(video) = self.tiles.get(index).and_then(|t| t.video.as_ref()) else {
            return false;
        };
        if zoom {
            let step = 1.0 + crate::viewer::zoom::prefs::current().wheel_step;
            let factor = if up { step } else { 1.0 / step };
            video.player.zoom_video_by(factor.powi(notches));
            return true;
        }
        let delta = f64::from(notches) * MULTIVIEW_VIDEO_WHEEL_VOLUME_STEP;
        match video.player.add_volume(if up { delta } else { -delta }) {
            Ok(()) => self.show_volume_feedback(index, VolumeInput::Wheel, Instant::now()),
            Err(e) => tracing::warn!(error = %e, "[Multiview: VideoBar] The wheel did not change the volume"),
        }
        true
    }

    /// A right click on the video of tile `index`: play or pause it, like mpv did.
    pub fn video_right_click(&mut self, index: usize) -> bool {
        if self.host.modal_locked {
            return false;
        }
        let Some(tile) = self.tiles.get(index) else {
            return false;
        };
        let Some(video) = tile.video.as_ref() else {
            return false;
        };
        tracing::debug!(
            media_id = tile.media_id,
            "[Multiview: VideoBar] Right click: play/pause"
        );
        if let Err(e) = video.player.toggle_pause() {
            tracing::warn!(error = %e, "[Multiview: VideoBar] Play/pause failed");
        }
        true
    }

    /// The host stops showing bars (it closes, hides or is locked): drags and hovers on them are over.
    pub fn release_video_bars(&mut self) {
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            video.bar.release();
        }
    }
}

/// What the bar of `video` (showing medium `media_id`) shows now.
fn bar_view(video: &mut VideoTile, media_id: MediaId, prefs: &VideoBarPrefs, now: Instant) -> VideoBarView {
    let playback = video.player.playback();
    let ms = video.bar.ms_shown(media_id, prefs.show_ms);
    // With milliseconds the time of the observer (a tenth of a second) is too coarse: mpv is asked directly.
    let actual = if ms && !playback.paused {
        video.player.get_position()
    } else {
        playback.position
    };
    let position = video.bar.shown_position(actual, now);
    let track = ProgressTrack::new(playback.duration, position);
    if !video.bar.dragging() {
        video.bar.note_level(playback.volume);
    }
    let hours = track.duration_secs() >= 3600.0;
    let hover = video
        .bar
        .hover()
        .map(|share| (share, time_text::clock(track.time_at(share), hours, ms)));
    VideoBarView {
        playing: !playback.paused,
        times: time_text::texts(position, track.duration_secs(), ms),
        fraction: track.fraction(),
        spans: track.shares(),
        volume_share: volume::slider_share(playback.volume),
        volume_icon: VolumeIcon::of(playback.volume, video.last_mute),
        muted: video.last_mute,
        hover,
    }
}
