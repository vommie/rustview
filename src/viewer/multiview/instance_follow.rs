//! Instance behavior around the live selection: the Multiview follows the media the user selects in the Browse view.
//!
//! The pure planner (`follow`) decides which tiles stay, leave and join; this applies the plan: tiles that stay keep
//! running (and keep their pinned width), a newcomer that takes the slot of a leaving tile inherits the slot's width, and a
//! video that replaces a video reuses the player of the leaving tile (`loadfile replace`) so volume, speed and loop
//! settings carry over.

use super::audio::{self, TileAudio};
use super::prefs;
use super::follow::{self, FollowPlan, Shown, Wanted};
use super::instance::MultiviewInstance;
use super::model::TileKind;
use super::teardown::teardown_videos;
use super::tile::{StaleProbe, TileSeed, TileState, VideoTile};
use crate::library::models::MediaId;
use crate::settings::schema::{MULTIVIEW_MAX_TILES, MULTIVIEW_STALE_PROBE_TICKS};
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
        inherited.stale = video
            .player
            .video_display_params()
            .map(|params| StaleProbe {
                params,
                ticks_left: MULTIVIEW_STALE_PROBE_TICKS,
            });
        inherited.audio = leaving.audio;
        inherited.video = Some(video);
    }
    inherited
}

impl MultiviewInstance {
    /// What the Multiview would do for a selection of `wanted` media (in view order). Nothing is changed.
    pub fn plan_follow(&self, wanted: &[Wanted]) -> FollowPlan {
        let shown: Vec<Shown> = self
            .tiles
            .iter()
            .map(|t| Shown {
                id: t.media_id,
                sticky: t.sticky,
                is_video: t.kind == TileKind::Video,
            })
            .collect();
        follow::follow(&shown, wanted, MULTIVIEW_MAX_TILES)
    }

    /// Brings the tiles in line with a follow plan: leaving tiles go, newcomers take the freed slots, the remaining tiles
    /// keep running. `seeds` holds the newcomers.
    pub fn apply_follow(&mut self, plan: &FollowPlan, mut seeds: HashMap<MediaId, TileSeed>) {
        if plan.is_noop() {
            return;
        }
        self.drag = None;
        self.clear_hover();
        let mut old: HashMap<MediaId, TileState> =
            self.tiles.drain(..).map(|t| (t.media_id, t)).collect();

        let one_for_one = plan.add.len() == plan.remove.len();
        let mut inherited: HashMap<MediaId, Inherited> = HashMap::new();
        for takeover in &plan.takeovers {
            if let Some(mut leaving) = old.remove(&takeover.from_id) {
                let passed = take_over(&mut leaving, takeover.recycle_player, one_for_one);
                inherited.insert(takeover.new_id, passed);
                old.insert(takeover.from_id, leaving);
            }
        }

        let mut ended: Vec<VideoTile> = Vec::new();
        for id in &plan.remove {
            ended.extend(old.remove(id).and_then(|t| t.video));
        }
        let (mut added, mut recycled) = (Vec::new(), 0usize);
        for id in &plan.slots {
            if let Some(tile) = old.remove(id) {
                self.tiles.push(tile);
                continue;
            }
            let Some(seed) = seeds.remove(id) else {
                continue;
            };
            let mut tile = TileState::from_seed(seed);
            if let Some(passed) = inherited.remove(id) {
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
            "[Multiview: Follow] Tiles updated to match the selection"
        );
    }
}
