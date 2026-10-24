//! Instance behavior around every change of the tile set: the Multiview follows the media the user selects in the Browse
//! view, tiles leave through their remove button, and a medium is shown a second time (`instance_clone`).
//!
//! The pure planner (`follow`) decides which tiles stay, leave and join; this applies the plan, whatever caused it: tiles that stay keep
//! running (and keep their pinned width), a newcomer that takes the slot of a leaving tile inherits the slot's width, and a
//! video that replaces a video reuses the player of the leaving tile (`loadfile replace`) so volume, speed and loop
//! settings carry over.

use super::audio::{self, TileAudio};
use super::follow::{self, FollowPlan, Shown, Slot, TileKey, Wanted};
use super::instance::MultiviewInstance;
use super::model::TileKind;
use super::prefs;
use super::teardown::teardown_videos;
use super::tile::{StaleProbe, TileSeed, TileState, VideoTile};
use rustview_core::settings::schema::{MULTIVIEW_MAX_TILES, MULTIVIEW_STALE_PROBE_TICKS};
use std::collections::HashMap;

/// What a newcomer takes over from the tile whose slot it fills.
struct Inherited {
    pin: Option<f64>,
    /// Present when the player of the leaving tile is recycled.
    video: Option<VideoTile>,
    audio: Option<TileAudio>,
    stale: Option<StaleProbe>,
}

/// What a leaving tile passes on to the newcomer in its slot: its player (if recycled) and, when media are only replaced
/// one for one, its pinned width (otherwise the newcomer is automatic and the row is shared anew).
fn take_over(leaving: &mut TileState, recycle: bool, keep_width: bool) -> Inherited {
    let mut inherited = Inherited {
        pin: leaving.pin.filter(|_| keep_width),
        video: None,
        audio: None,
        stale: None,
    };
    if recycle && let Some(video) = leaving.video.take() {
        inherited.stale = video.player.video_display_params().map(|params| StaleProbe {
            params,
            ticks_left: MULTIVIEW_STALE_PROBE_TICKS,
        });
        inherited.audio = leaving.audio;
        inherited.video = Some(video);
    }
    inherited
}

impl MultiviewInstance {
    /// What the planner needs to know about the tiles, in slot order.
    pub(super) fn shown(&self) -> Vec<Shown> {
        self.tiles
            .iter()
            .map(|t| Shown {
                key: t.key,
                media: t.media_id,
                sticky: t.sticky,
                is_video: t.kind == TileKind::Video,
            })
            .collect()
    }

    /// What the Multiview would do for a selection of `wanted` media (in view order). Nothing is changed.
    pub fn plan_follow(&self, wanted: &[Wanted]) -> FollowPlan {
        follow::follow(&self.shown(), wanted, MULTIVIEW_MAX_TILES)
    }

    /// Brings the tiles in line with a plan: leaving tiles go, newcomers take their slots, the remaining tiles keep running.
    /// `seeds` holds the newcomers, one entry per `plan.add` (`None` for a medium the caller could not describe: its tile is
    /// skipped).
    pub fn apply_follow(&mut self, plan: &FollowPlan, mut seeds: Vec<Option<TileSeed>>) {
        if plan.is_noop() {
            return;
        }
        self.drag = None;
        self.clear_hover();
        let mut old: HashMap<TileKey, TileState> = self.tiles.drain(..).map(|t| (t.key, t)).collect();

        let one_for_one = plan.add.len() == plan.remove.len();
        let mut inherited: HashMap<usize, Inherited> = HashMap::new();
        for takeover in &plan.takeovers {
            if let Some(leaving) = old.get_mut(&takeover.from) {
                let passed = take_over(leaving, takeover.recycle_player, one_for_one);
                inherited.insert(takeover.add, passed);
            }
        }

        let mut ended: Vec<VideoTile> = Vec::new();
        for key in &plan.remove {
            ended.extend(old.remove(key).and_then(|t| t.video));
        }
        let (mut added, mut recycled) = (Vec::new(), 0usize);
        for slot in &plan.slots {
            match *slot {
                Slot::Keep(key) => {
                    if let Some(tile) = old.remove(&key) {
                        self.tiles.push(tile);
                    }
                }
                Slot::New(index) => {
                    let Some(seed) = seeds.get_mut(index).and_then(Option::take) else {
                        continue;
                    };
                    let mut tile = TileState::from_seed(seed);
                    if let Some(passed) = inherited.remove(&index) {
                        tile.pin = passed.pin;
                        if let Some(video) = passed.video {
                            tile.video = Some(video);
                            tile.audio = passed.audio;
                            tile.stale_probe = passed.stale;
                            recycled += 1;
                        }
                    }
                    added.push(self.tiles.len());
                    self.tiles.push(tile);
                }
            }
        }
        ended.extend(old.into_values().filter_map(|t| t.video));

        let fresh: Vec<usize> = added
            .iter()
            .copied()
            .filter(|&i| self.tiles[i].video.is_none())
            .collect();
        let mut slots: Vec<Option<TileAudio>> = self.tiles.iter().map(|t| t.audio).collect();
        audio::after_reconcile(&mut slots, &fresh, &prefs::current().audio);
        for (tile, slot) in self.tiles.iter_mut().zip(slots) {
            tile.audio = slot;
        }

        self.apply_layout();
        for &index in &added {
            self.start_tile(index);
        }
        self.apply_audio();
        let players_closed = ended.len();
        teardown_videos(ended);
        self.bump();
        tracing::info!(
            removed = plan.remove.len(),
            added = added.len(),
            recycled,
            shown = self.tiles.len(),
            players_closed,
            overflow = plan.overflow,
            "[Multiview: Follow] Tiles updated"
        );
    }
}
