//! Instance behavior around the primary action bar: the state its buttons show (computed live from the tiles, so it follows
//! every tile that joins or leaves and every mute or pause done inside mpv), what a click does, the pin that keeps the
//! fullscreen open and the idle mouse cursor.

use super::audio::{AudioMode, AudioOrigin, TileAudio};
use super::bar_placement::{BarCorner, near_bar_corner};
use super::instance::MultiviewInstance;
use super::prefs;
use super::primary_actions::{PrimaryActionId, PrimaryActionSpec, PrimaryBarContext, primary_actions_for};
use super::route::{HostMode, UiAction};
use super::view::{PrimaryActionView, PrimaryBarTarget};
use crate::settings::schema::{MULTIVIEW_PIN_BLINK_MS, MULTIVIEW_PIN_PULSE_MS};
use std::time::{Duration, Instant};

impl MultiviewInstance {
    fn primary_context(&self) -> PrimaryBarContext {
        let bar = prefs::current().primary;
        let mut video_count = 0;
        let (mut any_audible, mut any_playing) = (false, false);
        for tile in &self.tiles {
            let (Some(video), Some(_)) = (tile.video.as_ref(), tile.audio) else {
                continue;
            };
            video_count += 1;
            any_audible |= !video.last_mute;
            any_playing |= !video.player.is_user_paused();
        }
        PrimaryBarContext {
            host: self.host.surface.mode,
            origin: self.origin,
            tile_count: self.tiles.len(),
            video_count,
            any_audible,
            any_playing,
            layout_custom: self.row_pins.is_some() || self.tiles.iter().any(|t| t.pin.is_some()),
            pinned: self.pinned,
            hide_idle_cursor: bar.hide_idle_cursor,
            idle_cursor_ms: bar.idle_cursor_ms,
        }
    }

    /// The buttons the user allows, in the context of this moment.
    fn primary_specs(&self) -> Vec<PrimaryActionSpec> {
        let bar = prefs::current().primary;
        primary_actions_for(&self.primary_context())
            .into_iter()
            .filter(|spec| bar.allows(spec.id))
            .collect()
    }

    fn pulsing(&self, now: Instant) -> bool {
        self.pin_pulse_until.is_some_and(|until| now < until)
    }

    /// The pin button is in its highlighted phase of the blinking.
    fn pin_blink_on(&self, now: Instant) -> bool {
        self.pin_pulse_until.is_some_and(|until| {
            until
                .checked_duration_since(now)
                .is_some_and(|left| (left.as_millis() as u64 / MULTIVIEW_PIN_BLINK_MS) % 2 == 0)
        })
    }

    /// The primary bar the host may show now, or `None` while the user switched it off, nothing is on offer, or the pointer
    /// is nowhere near its corner (unless the bar shows itself to give feedback).
    pub fn primary_bar_target(&self) -> Option<PrimaryBarTarget> {
        let bar = prefs::current().primary;
        if !bar.enabled || !matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView) {
            return None;
        }
        let now = Instant::now();
        let forced = self.pulsing(now);
        let area = self.host.projection.screen();
        let near = self
            .pointer
            .is_some_and(|p| near_bar_corner(BarCorner::TopLeft, area, p, self.host.surface.dpi, bar.reveal_distance_px));
        if !near && !forced {
            return None;
        }
        let blink = self.pin_blink_on(now);
        let actions: Vec<PrimaryActionView> = self
            .primary_specs()
            .into_iter()
            .map(|spec| PrimaryActionView {
                attention: blink && spec.id == PrimaryActionId::Pin,
                id: spec.id,
                icon: spec.icon,
                tooltip: spec.tooltip,
                enabled: spec.enabled,
                active: spec.active,
            })
            .collect();
        (!actions.is_empty()).then_some(PrimaryBarTarget {
            area,
            actions,
            pointer: self.pointer,
            reveal_distance_px: bar.reveal_distance_px,
            forced,
        })
    }

    /// A click on a button of the primary bar. Buttons that are not on offer (a stale click) or disabled do nothing, except
    /// that a click on a blocked close button is a refused close like any other.
    pub fn perform_primary_action(&mut self, id: PrimaryActionId) -> UiAction {
        let Some(spec) = self.primary_specs().into_iter().find(|s| s.id == id) else {
            return UiAction::None;
        };
        if !spec.enabled {
            if self.pinned && matches!(id, PrimaryActionId::SendToTab | PrimaryActionId::Close) {
                self.refuse_close();
            }
            return UiAction::None;
        }
        tracing::info!(?id, "[Multiview: Primary bar] Button clicked");
        match id {
            PrimaryActionId::SendToTab => UiAction::ConvertToTab,
            PrimaryActionId::Close => self.close_action(),
            PrimaryActionId::MuteAll => {
                let mute = self.primary_context().any_audible;
                self.set_all_muted(mute);
                UiAction::None
            }
            PrimaryActionId::PlayPauseAll => {
                let pause = self.primary_context().any_playing;
                self.set_all_paused(pause);
                UiAction::None
            }
            PrimaryActionId::ResetLayout => {
                self.reset_split();
                UiAction::None
            }
            PrimaryActionId::HideIdleCursor => UiAction::SetIdleCursorHide(!prefs::current().primary.hide_idle_cursor),
            PrimaryActionId::Pin => {
                self.pinned = !self.pinned;
                tracing::info!(pinned = self.pinned, "[Multiview: Primary bar] Pin toggled");
                self.bump();
                UiAction::None
            }
        }
    }

    /// Mutes or unmutes every video; the choice is the user's, so the sound rules leave it alone afterwards.
    fn set_all_muted(&mut self, muted: bool) {
        for tile in &mut self.tiles {
            let (Some(video), Some(audio)) = (tile.video.as_mut(), tile.audio.as_mut()) else {
                continue;
            };
            match video.player.set_mute(muted) {
                Ok(()) => {
                    video.last_mute = muted;
                    *audio = TileAudio {
                        mode: if muted { AudioMode::Muted } else { AudioMode::Unmuted },
                        origin: AudioOrigin::User,
                    };
                }
                Err(e) => tracing::warn!(media_id = tile.media_id, error = %e, "[Multiview: Primary bar] Mute failed"),
            }
        }
        tracing::info!(muted, "[Multiview: Primary bar] All videos muted or unmuted");
        self.bump();
    }

    fn set_all_paused(&mut self, paused: bool) {
        for video in self.tiles.iter().filter_map(|t| t.video.as_ref()) {
            let result = if paused { video.player.pause() } else { video.player.play() };
            if let Err(e) = result {
                tracing::warn!(error = %e, "[Multiview: Primary bar] Pause or play failed");
            }
        }
        tracing::info!(paused, "[Multiview: Primary bar] All videos paused or started");
        self.bump();
    }

    /// Whether a pinned fullscreen refuses to close. A refusal shows the primary bar and blinks the pin button for a moment.
    pub fn refuse_close(&mut self) -> bool {
        if !self.pinned {
            return false;
        }
        self.pin_pulse_until = Some(Instant::now() + Duration::from_millis(MULTIVIEW_PIN_PULSE_MS));
        tracing::info!("[Multiview: Primary bar] Close refused: the fullscreen is pinned");
        self.bump();
        true
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// Lets mpv hide the cursor over its videos after the idle time of the user's setting (and show it when it moves), or
    /// returns control to the user's mpv configuration. mpv owns the cursor over a video, so this is the one place it is set.
    pub fn apply_cursor_autohide(&self) {
        for video in self.tiles.iter().filter_map(|t| t.video.as_ref()) {
            self.apply_cursor_autohide_to(video);
        }
    }

    pub(super) fn apply_cursor_autohide_to(&self, video: &super::tile::VideoTile) {
        let bar = prefs::current().primary;
        let idle = (bar.hide_idle_cursor && matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView))
            .then_some(u64::from(bar.idle_cursor_ms));
        video.player.set_cursor_autohide(idle);
    }

    /// The mouse cursor over the Multiview should be hidden now: the user wants idle cursors hidden, the pointer is in the
    /// host and has not moved for long enough.
    pub fn cursor_should_hide(&self) -> bool {
        let prefs = prefs::current().primary;
        prefs.hide_idle_cursor
            && self.pointer.is_some()
            && self
                .last_move
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(u64::from(prefs.idle_cursor_ms)))
    }
}
