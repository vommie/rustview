//! The tick of a shown Multiview, and when it is needed at all. Nothing of the Multiview polls: the hosts react to pointer
//! events, keys and player events at once, and the tick only advances what changes with time alone (a bar that hides after the
//! pointer rested, the cursor that hides, a zoom that waits to be rendered, the playing time on a visible control bar). The host
//! asks `tick_deadline` after every change and sleeps until then; with nothing pending it does not wake up at all.

use super::instance::MultiviewInstance;
use super::model::TileKind;
use super::prefs;
use rustview_core::settings::schema::{MULTIVIEW_TICK_MS, MULTIVIEW_VIDEO_BAR_IDLE_MS};
use std::time::{Duration, Instant};

/// How often the time on a visible control bar of a playing video moves (and how often a running pulse is drawn).
pub const TICK: Duration = Duration::from_millis(MULTIVIEW_TICK_MS);

impl MultiviewInstance {
    /// Advances what changes with time alone. `pointer` is the last pointer position the host saw over its window (window
    /// coordinates, `None` outside it); the hovered tile is found again, the layout may have changed.
    pub fn tick(&mut self, pointer: Option<(i32, i32)>) {
        self.pointer_moved(pointer);
        self.poll_zoom();
        self.poll_mark_reveal(Instant::now());
    }

    /// When the host has to call `tick` next, at the latest: `now + TICK` while something moves continuously (the time on the
    /// visible bar of a playing video, a blinking pin button), else the next moment something changes by itself. `None`: nothing
    /// changes until the next event.
    pub fn tick_deadline(&self, now: Instant) -> Option<Instant> {
        if self.ticks_continuously(now) {
            return Some(now + TICK);
        }
        let idle = Duration::from_millis(MULTIVIEW_VIDEO_BAR_IDLE_MS);
        let cursor = prefs::current().primary;
        let cursor_hides = cursor.hide_idle_cursor && self.pointer.is_some() && !self.cursor_hidden;
        let rested = self
            .last_move
            .filter(|_| cursor_hides)
            .map(|at| at + Duration::from_millis(u64::from(cursor.idle_cursor_ms)));
        let tiles = self.tiles.iter().flat_map(|tile| {
            let zoom = tile.zoom.as_ref().and_then(|zoom| zoom.wakeup(now));
            let bar = tile.video.as_ref().and_then(|video| {
                [video.bar.reveal.deadline(idle, now), video.bar.seek_deadline()]
                    .into_iter()
                    .flatten()
                    .min()
            });
            [zoom, bar]
        });
        [rested, self.marks_revealed_until, self.pin_pulse_until]
            .into_iter()
            .chain(tiles)
            .flatten()
            .filter(|at| *at > now)
            .min()
    }

    /// Something on screen moves with every tick: the playing time of a video whose control bar shows, or the blinking pin.
    fn ticks_continuously(&self, now: Instant) -> bool {
        self.pin_pulse_until.is_some_and(|until| now < until)
            || self.tiles.iter().any(|tile| {
                tile.kind == TileKind::Video
                    && tile.error.is_none()
                    && tile.video.as_ref().is_some_and(|video| {
                        let playback = video.player.playback();
                        video.bar.reveal.visible() && !playback.paused && !playback.ended
                    })
            })
    }

    /// Whether a video of the Multiview plays: its time badge in the tab grid moves.
    pub fn has_playing_video(&self) -> bool {
        self.tiles.iter().any(|tile| {
            tile.error.is_none()
                && tile.video.as_ref().is_some_and(|video| {
                    let playback = video.player.playback();
                    !playback.paused && !playback.ended
                })
        })
    }
}
