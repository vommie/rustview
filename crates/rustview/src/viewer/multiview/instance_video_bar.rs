//! The control bars of the video tiles (`video_bar`): which bars the host shows on this tick and with what, and what a click,
//! drag or wheel on a bar does. The mouse over a video is `RustView`'s (its window lets the pointer through to the host), so
//! the wheel and the right button over a video come here as well.

use super::instance::MultiviewInstance;
use super::instance_zoom::WHEEL_STEP_DELTA;
use super::key_labels;
use super::layout::Rect;
use super::model::TileKind;
use super::route::HostMode;
use super::tile::VideoTile;
use super::video_bar::ab_view;
use super::video_bar::bar_menu::{MenuAction, MenuLevels, MenuPath};
use super::video_bar::collapse::BarFit;
use super::video_bar::more_menu::{self, MoreInput};
use super::video_bar::placement::{self, AbRoom, VideoBarPlacement};
use super::video_bar::popups::{self, BarPopup};
use super::video_bar::prefs::{RevealMode, VideoBarPrefs};
use super::video_bar::progress::ProgressTrack;
use super::video_bar::reveal::{Phase, RevealInput};
use super::video_bar::time_text;
use super::video_bar::view::{DragPhase, VideoBarInput, VideoBarTarget, VideoBarView};
use super::video_bar::volume::{self, VolumeIcon};
use super::video_filter::menu as filter_menu;
use super::video_filter::{FilterChange, KNOB_RANGE, VideoFilters, default_normalize};
use super::volume_feedback::VolumeInput;
use crate::viewer::ab_repeat::prefs as ab_prefs;
use crate::viewer::mpv_engine::SeekPrecision;
use crate::viewer::multiview::follow::TileKey;
use rustview_core::library::models::MediaId;
use rustview_core::settings::schema::{
    MULTIVIEW_VIDEO_BAR_COLLAPSE_HYSTERESIS_LOGICAL_PX, MULTIVIEW_VIDEO_BAR_FADE_MS, MULTIVIEW_VIDEO_BAR_IDLE_MS,
    MULTIVIEW_VIDEO_BAR_REVEAL_MARGIN_LOGICAL_PX, MULTIVIEW_VIDEO_BAR_SEEK_MIN_BAR_HEIGHTS,
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

    /// The control bars to show (or to let fade out) on this tick, one per video at most, with their content and their place
    /// relative to the host's rectangle (the canvas).
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
        let origin = (self.host.surface.rect.x, self.host.surface.rect.y);
        let mut targets = Vec::new();
        for index in 0..self.tiles.len() {
            // The bar lies at the bottom of what shows of the video: its place (a lone video's is the whole view), or the room
            // while it is zoomed.
            let video_rect = self.video_placement(index).map(|p| p.clip);
            let tile = &mut self.tiles[index];
            let (key, media_id, broken) = (tile.key, tile.media_id, tile.error.is_some());
            let filters = tile.filters;
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
            // The A-B group above the bar (strip, flags, labels) counts as the bar: the pointer there keeps it.
            let keep = if !mini && video.player.ab_repeat().active() {
                let room = (AbRoom::for_bar(prefs.height).above_bar(prefs.height) * scale)
                    .round()
                    .to_int::<i32>();
                Rect::new(place.bar.x, place.bar.y - room, place.bar.w, place.bar.h + room)
            } else {
                place.bar
            };
            let phase = video.bar.reveal.decide(
                &RevealInput {
                    mode,
                    video: rect,
                    bar: keep,
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
            let width = place.bar.w.az::<f32>() / scale;
            let view = bar_view(video, media_id, prefs, (mini, width, &filters), now);
            let placement = match &view.ab {
                Some(_) => place.with_room_above(ab_room_px(prefs.height, scale)),
                None => place,
            };
            targets.push(VideoBarTarget {
                key,
                placement: placement.translated(-origin.0, -origin.1),
                scale,
                phase,
                mini,
                view,
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
                | VideoBarInput::AudioButton
                | VideoBarInput::AbBubble
                | VideoBarInput::MoreButton
                | VideoBarInput::FilterButton
                | VideoBarInput::Menu(_)
                | VideoBarInput::MenuReset(_)
                | VideoBarInput::MenuSlider {
                    phase: DragPhase::Press,
                    ..
                }
                | VideoBarInput::Seek {
                    phase: DragPhase::Press,
                    ..
                }
                | VideoBarInput::Volume {
                    phase: DragPhase::Press,
                    ..
                }
        ) || matches!(&input, VideoBarInput::Ab(ab) if ab.is_press());
        let selected = pressed && self.select_tile(key);
        let media_id = self.tiles[index].media_id;
        match input {
            VideoBarInput::Speaker => {
                self.speaker_click_at(index);
                return true;
            }
            VideoBarInput::Wheel { delta_y } => return self.video_wheel(index, delta_y, false) || selected,
            VideoBarInput::Ab(ab) => return self.ab_input_from_bar(index, ab) || selected,
            VideoBarInput::Menu(action) => return self.menu_action(index, action, prefs) || selected,
            VideoBarInput::MenuSlider { action, share, phase } => {
                return self.menu_slider(index, action, share, phase) || selected;
            }
            VideoBarInput::MenuReset(action) => {
                return action
                    .reset_part()
                    .is_some_and(|part| self.change_filters(index, FilterChange::Reset(part)))
                    || selected;
            }
            _ => {}
        }
        let Some(video) = self.tiles[index].video.as_mut() else {
            return selected;
        };
        let player = video.player.clone();
        let name = input.name();
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
                let seeks = video
                    .bar
                    .seek_to(share, secs, Instant::now(), phase == DragPhase::Release, exact);
                if phase != DragPhase::Move {
                    tracing::debug!(media_id, ?phase, ?precision, seeks, secs, "[Multiview: VideoBar] Seek");
                }
                match precision {
                    Some(precision) if seeks => player.seek_to(secs, precision),
                    _ => Ok(()),
                }
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
            VideoBarInput::Measured(needs) => {
                // Only a change of the widths (the bar height, the times' templates) changes what fits.
                return video.bar.fit.measured(needs);
            }
            VideoBarInput::AudioButton => {
                video.bar.toggle_popup(BarPopup::Volume);
                tracing::debug!(
                    media_id,
                    open = video.bar.popup.is_some(),
                    "[Multiview: VideoBar] Volume popup"
                );
                Ok(())
            }
            VideoBarInput::MoreButton => {
                video.bar.toggle_popup(BarPopup::More {
                    path: MenuPath::default(),
                });
                tracing::debug!(
                    media_id,
                    open = video.bar.popup.is_some(),
                    "[Multiview: VideoBar] More menu"
                );
                Ok(())
            }
            VideoBarInput::FilterButton => {
                video.bar.toggle_popup(BarPopup::Filters {
                    path: MenuPath::default(),
                });
                tracing::debug!(
                    media_id,
                    open = video.bar.popup.is_some(),
                    "[Multiview: VideoBar] Filter menu"
                );
                Ok(())
            }
            VideoBarInput::AbBubble => {
                popups::set_ab_bubble_open(!popups::ab_bubble_open());
                video.bar.ab.popup = None;
                Ok(())
            }
            VideoBarInput::ClosePopups => {
                video.bar.close_popups();
                Ok(())
            }
            // Handled above.
            VideoBarInput::Wheel { .. }
            | VideoBarInput::Speaker
            | VideoBarInput::Ab(_)
            | VideoBarInput::Menu(_)
            | VideoBarInput::MenuReset(_)
            | VideoBarInput::MenuSlider { .. } => Ok(()),
        };
        if let Err(e) = result {
            tracing::warn!(media_id, input = name, error = %e, "[Multiview: VideoBar] The player did not take it");
        }
        true
    }

    /// An input of the A-B group; a save bubble or load menu it opens takes the place of the bar's other popups.
    fn ab_input_from_bar(&mut self, index: usize, input: super::video_bar::view::AbInput) -> bool {
        let changed = self.ab_input(index, input);
        if let Some(video) = self.tiles[index].video.as_mut()
            && video.bar.ab.popup.is_some()
        {
            video.bar.popup = None;
        }
        changed
    }

    /// An entry of the open menu of tile `index`: a filter (the menu stays open for the next one), a submenu, or what its
    /// control in the bar does (the menu closes).
    fn menu_action(&mut self, index: usize, action: MenuAction, prefs: &VideoBarPrefs) -> bool {
        let key = self.tiles[index].key;
        tracing::debug!(
            media_id = self.tiles[index].media_id,
            ?action,
            "[Multiview: VideoBar] Menu entry"
        );
        if let Some(change) = action.filter_change() {
            self.change_filters(index, change);
            return true;
        }
        let Some(video) = self.tiles[index].video.as_mut() else {
            return false;
        };
        match action {
            MenuAction::Volume => {
                video.bar.popup = Some(BarPopup::Volume);
                video.bar.ab.popup = None;
                true
            }
            MenuAction::Open(sub) => {
                video.bar.popup = match video.bar.popup {
                    Some(BarPopup::More { mut path }) => {
                        path.toggle(MenuPath::level_of(sub, false), sub);
                        Some(BarPopup::More { path })
                    }
                    Some(BarPopup::Filters { mut path }) => {
                        path.toggle(MenuPath::level_of(sub, true), sub);
                        Some(BarPopup::Filters { path })
                    }
                    other => other,
                };
                true
            }
            other => {
                // The menu closes, unless the entry opens a popup of its own (Save, Load), which takes its place anyway.
                video.bar.popup = None;
                more_menu::input(other).is_some_and(|input| self.video_bar_input(key, input, prefs))
            }
        }
    }

    /// A slider of the open menu of tile `index` (a picture adjustment) moved to `share` of its width.
    fn menu_slider(&mut self, index: usize, action: MenuAction, share: f32, phase: DragPhase) -> bool {
        let MenuAction::Knob(knob) = action else {
            return false;
        };
        let range = f64::from(KNOB_RANGE);
        let value = ((f64::from(share.clamp(0.0, 1.0)) * 2.0 - 1.0) * range)
            .round()
            .to_int::<i32>();
        if phase == DragPhase::Release {
            tracing::debug!(
                media_id = self.tiles[index].media_id,
                ?knob,
                value,
                "[Multiview: Filters] Slider let go"
            );
        }
        self.change_filters(index, FilterChange::SetKnob(knob, value));
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

    /// Esc closes the volume popup or the "..." menu of a bar (the A-B popups take Esc in their text fields). Returns
    /// whether one was open.
    pub fn close_video_bar_popups(&mut self) -> bool {
        let mut closed = false;
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            closed |= video.bar.popup.take().is_some();
        }
        if closed {
            tracing::debug!("[Multiview: VideoBar] Esc closed a popup of a bar");
            self.bump();
        }
        closed
    }

    /// The host stops showing bars (it closes, hides or is locked): drags and hovers on them are over.
    pub fn release_video_bars(&mut self) {
        for video in self.tiles.iter_mut().filter_map(|t| t.video.as_mut()) {
            video.bar.release();
        }
    }
}

/// The room the A-B group needs above a bar of `bar_height` (logical), in physical pixels.
fn ab_room_px(bar_height: f32, scale: f32) -> i32 {
    (AbRoom::for_bar(bar_height).above_bar(bar_height) * scale)
        .round()
        .to_int::<i32>()
}

/// What the bar of `video` (showing medium `media_id`) shows now (`mini`: the small bar of the tab grid; `width`: the bar's
/// width in logical pixels).
fn bar_view(
    video: &mut VideoTile,
    media_id: MediaId,
    prefs: &VideoBarPrefs,
    (mini, width, filters): (bool, f32, &VideoFilters),
    now: Instant,
) -> VideoBarView {
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
    let ab = if mini {
        None
    } else {
        ab_view::view(
            &video.player.ab_repeat(),
            track.duration_secs(),
            &ab_prefs::current(),
            &video.bar.ab,
            (hours, ms),
        )
    };
    // The small bar of the tab grid has no step of its own: it only ever shows play/pause and the progress bar.
    let fit = if mini {
        BarFit::Full
    } else {
        video.bar.fit.fit(
            width,
            MULTIVIEW_VIDEO_BAR_SEEK_MIN_BAR_HEIGHTS * prefs.height,
            MULTIVIEW_VIDEO_BAR_COLLAPSE_HYSTERESIS_LOGICAL_PX,
        )
    };
    video.bar.settle_popups(fit);
    let times = time_text::texts(position, track.duration_secs(), ms);
    let volume_icon = VolumeIcon::of(playback.volume, video.last_mute);
    let keys = key_labels::filters();
    let default_normalize = default_normalize();
    let menu = match video.bar.popup {
        Some(BarPopup::More { path }) => more_menu::build(&MoreInput {
            fit,
            times: &times,
            ms,
            volume: playback.volume,
            volume_icon,
            muted: video.last_mute,
            ab: ab.as_ref(),
            filters,
            default_normalize,
            keys: &keys,
            path,
        }),
        Some(BarPopup::Filters { path }) => filter_menu::button_menu(filters, &keys, default_normalize, path),
        _ => MenuLevels::default(),
    };
    VideoBarView {
        playing: !playback.paused,
        times,
        fraction: track.fraction(),
        spans: track.shares(),
        volume_share: volume::slider_share(playback.volume),
        volume_icon,
        muted: video.last_mute,
        hover,
        volume_text: format!("{:.0} %", playback.volume.max(0.0)),
        ab,
        fit,
        popup: video.bar.popup,
        ab_bubble: popups::ab_bubble_open(),
        menu,
        filter_count: filters.active_count(default_normalize),
    }
}
