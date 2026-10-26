//! Instance behavior around the primary action bar: the state its buttons show (computed live from the tiles, so it follows
//! every tile that joins or leaves and every mute or pause done inside mpv), what a click does, the pin that keeps the
//! fullscreen open and the idle mouse cursor.

use super::audio::{AudioMode, AudioOrigin, TileAudio};
use super::bar_placement::{BarCorner, bar_size_logical, in_reveal_zone, place_bar};
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::prefs;
use super::primary_actions::{PrimaryActionId, PrimaryActionSpec, PrimaryBarContext, primary_actions_for};
use super::route::{HostMode, UiAction};
use super::view::{PrimaryActionView, PrimaryBarTarget};
use rustview_core::settings::schema::{MULTIVIEW_PIN_BLINK_MS, MULTIVIEW_PIN_PULSE_MS};
use rustview_core::util::cast::SaturatingAs;
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
            pinned: self.pin_in_effect(),
            hide_idle_cursor: bar.hide_idle_cursor,
            hide_selection_with_cursor: prefs::current().focus_marks.hide_with_cursor,
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

    /// A pinned fullscreen only stays pinned while the user can reach the pin button: switching the primary bar or its pin
    /// button off in the settings releases the pin, otherwise nothing could ever unpin the view again.
    fn pin_in_effect(&self) -> bool {
        self.pinned && prefs::current().primary.allows(PrimaryActionId::Pin)
    }

    fn pulsing(&self, now: Instant) -> bool {
        self.pin_pulse_until.is_some_and(|until| now < until)
    }

    /// The pin button is in its highlighted phase of the blinking.
    fn pin_blink_on(&self, now: Instant) -> bool {
        self.pin_pulse_until.is_some_and(|until| {
            until.checked_duration_since(now).is_some_and(|left| {
                (left.as_millis().saturating_as::<u64>() / MULTIVIEW_PIN_BLINK_MS).is_multiple_of(2)
            })
        })
    }

    /// The primary bar the canvas of the host draws now, or `None` while the user switched it off, nothing is on offer, the
    /// Multiview is too small for it or the pointer is not within the reveal distance of it (unless the bar shows itself to give
    /// feedback: a refused close blinks its pin button).
    pub fn primary_bar_target(&self) -> Option<PrimaryBarTarget> {
        let prefs = prefs::current().primary;
        if !prefs.enabled || !matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView) {
            return None;
        }
        let now = Instant::now();
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
        if actions.is_empty() {
            return None;
        }
        let scale = self.host.surface.dpi;
        let area = self.host.projection.screen();
        let bar = place_bar(BarCorner::TopLeft, area, bar_size_logical(actions.len(), false), scale)?;
        let revealed = self.pulsing(now)
            || self
                .pointer
                .is_some_and(|p| in_reveal_zone(bar, p, scale, prefs.reveal_distance_px));
        let origin = (self.host.surface.rect.x, self.host.surface.rect.y);
        revealed.then(|| PrimaryBarTarget {
            bar: Rect::new(bar.x - origin.0, bar.y - origin.1, bar.w, bar.h),
            actions,
        })
    }

    /// A click on a button of the primary bar. Buttons that are not on offer (a stale click) or disabled do nothing, except
    /// that a click on a blocked close button is a refused close like any other.
    pub fn perform_primary_action(&mut self, id: PrimaryActionId) -> UiAction {
        let Some(spec) = self.primary_specs().into_iter().find(|s| s.id == id) else {
            return UiAction::None;
        };
        if !spec.enabled {
            if self.pin_in_effect() && matches!(id, PrimaryActionId::SendToTab | PrimaryActionId::Close) {
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
            let result = if paused {
                video.player.pause()
            } else {
                video.player.play()
            };
            if let Err(e) = result {
                tracing::warn!(error = %e, "[Multiview: Primary bar] Pause or play failed");
            }
        }
        tracing::info!(paused, "[Multiview: Primary bar] All videos paused or started");
        self.bump();
    }

    /// Whether a pinned fullscreen refuses to close. A refusal shows the primary bar and blinks the pin button for a moment.
    pub fn refuse_close(&mut self) -> bool {
        if !self.pin_in_effect() {
            return false;
        }
        self.pin_pulse_until = Some(Instant::now() + Duration::from_millis(MULTIVIEW_PIN_PULSE_MS));
        tracing::info!("[Multiview: Primary bar] Close refused: the fullscreen is pinned");
        self.bump();
        true
    }

    pub fn is_pinned(&self) -> bool {
        self.pin_in_effect()
    }

    /// The mouse cursor over the Multiview should be hidden now: the user wants idle cursors hidden, the pointer is inside
    /// the Multiview (in a tab, the media area of the main window) and has not moved for long enough.
    pub fn cursor_should_hide(&self) -> bool {
        let prefs = prefs::current().primary;
        let frame = self.host.surface.rect;
        prefs.hide_idle_cursor
            && self.pointer.is_some_and(|(x, y)| frame.contains(x, y))
            && self
                .last_move
                .is_some_and(|at| at.elapsed() >= Duration::from_millis(u64::from(prefs.idle_cursor_ms)))
    }

    /// Records whether the host hides the mouse cursor over the Multiview (the host's Slint window does it, videos included).
    pub fn set_cursor_hidden(&mut self, hidden: bool) {
        self.cursor_hidden = hidden;
        // The marks of the selected medium may go and come with the cursor.
        self.marks_visibility_changed();
    }

    pub fn is_cursor_hidden(&self) -> bool {
        self.cursor_hidden
    }
}
