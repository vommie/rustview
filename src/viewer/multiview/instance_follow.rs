//! Instance behavior around the live selection: the Multiview follows the media the user selects in the Browse view.
//!
//! The pure planner (`follow`) decides which tiles stay, leave and join; this applies the plan: tiles that stay keep
//! running (and keep their pinned width), a newcomer that takes the slot of a leaving tile inherits the slot's width, and a
//! video that replaces a video reuses the player of the leaving tile (`loadfile replace`, like the former quick fullscreen
//! browsed videos) so volume, speed and loop settings carry over.

use super::audio::{self, TileAudio};
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
        audio::after_reconcile(&mut slots, &fresh);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::HostSurface;
    use crate::viewer::multiview::instance::SharedInstance;
    use std::path::PathBuf;

    fn seed(id: MediaId, aspect: f32) -> TileSeed {
        TileSeed {
            media_id: id,
            kind: TileKind::Image,
            aspect,
            path: PathBuf::new(),
            filename: format!("{id}.png"),
        }
    }

    fn instance(ids: &[MediaId]) -> SharedInstance {
        let shared =
            MultiviewInstance::detached(ids.iter().map(|&id| seed(id, 1.78)).collect());
        shared
            .lock()
            .set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0));
        shared
    }

    fn wanted(ids: &[MediaId]) -> Vec<Wanted> {
        ids.iter()
            .map(|&id| Wanted {
                id,
                is_video: false,
            })
            .collect()
    }

    fn follow_to(mv: &mut MultiviewInstance, ids: &[MediaId]) -> FollowPlan {
        let plan = mv.plan_follow(&wanted(ids));
        let seeds = plan.add.iter().map(|&id| (id, seed(id, 1.78))).collect();
        mv.apply_follow(&plan, seeds);
        plan
    }

    #[test]
    fn the_view_grows_and_shrinks_with_the_selection() {
        let shared = instance(&[1]);
        let mut mv = shared.lock();
        for (ids, shown) in [
            (vec![1, 2], 2),
            (vec![1, 2, 3], 3),
            (vec![2, 3, 4, 5], 4),
            (vec![3], 1),
            (vec![3, 9], 2),
        ] {
            follow_to(&mut mv, &ids);
            assert_eq!(mv.tile_count(), shown, "{ids:?}");
            assert_eq!(mv.layout.tiles.len(), shown);
            assert!(mv.member_ids().iter().all(|id| ids.contains(id)));
        }
    }

    #[test]
    fn an_empty_selection_leaves_the_view_alone() {
        let shared = instance(&[1, 2]);
        let mut mv = shared.lock();
        let revision = mv.revision();
        let plan = follow_to(&mut mv, &[]);
        assert!(plan.is_noop());
        assert_eq!(mv.member_ids(), vec![1, 2]);
        assert_eq!(mv.revision(), revision, "nothing was redrawn");
    }

    #[test]
    fn sticky_tiles_survive_a_new_selection_and_keep_their_width() {
        let shared = instance(&[1, 2]);
        let mut mv = shared.lock();
        mv.drag_started(0);
        mv.dragged(150.0);
        mv.tiles[0].sticky = true;
        let pinned = mv.tiles[0].pin;
        assert!(pinned.is_some());

        follow_to(&mut mv, &[7, 8]);

        assert_eq!(mv.member_ids(), vec![1, 7, 8]);
        assert_eq!(mv.tiles[0].pin, pinned, "the sticky tile keeps its width");
        assert_eq!(
            (mv.tiles[1].pin, mv.tiles[2].pin),
            (None, None),
            "newcomers are automatic when the number of tiles changes"
        );
        let widths: i32 = mv.layout.tiles.iter().map(|t| t.column.w).sum();
        assert_eq!(widths, 1920 - 2 * mv.params.gutter);
    }

    #[test]
    fn a_replacement_inherits_the_width_of_the_slot() {
        let shared = instance(&[1, 2]);
        let mut mv = shared.lock();
        mv.drag_started(0);
        mv.dragged(150.0);
        let (left, right) = (mv.tiles[0].pin, mv.tiles[1].pin);

        follow_to(&mut mv, &[8, 9]);

        assert_eq!(mv.member_ids(), vec![8, 9]);
        assert_eq!((mv.tiles[0].pin, mv.tiles[1].pin), (left, right));
    }

    #[test]
    fn unpinned_tiles_adapt_when_the_selection_changes() {
        let shared = instance(&[1]);
        let mut mv = shared.lock();
        follow_to(&mut mv, &[1, 2, 3]);
        assert!(mv.tiles.iter().all(|t| t.pin.is_none()));
        let widths: Vec<i32> = mv.layout.tiles.iter().map(|t| t.column.w).collect();
        assert_eq!(widths.iter().sum::<i32>(), 1920 - 2 * mv.params.gutter);
        follow_to(&mut mv, &[2]);
        assert_eq!(mv.layout.tiles.len(), 1);
        assert_eq!(mv.layout.tiles[0].column.w, 1920);
    }

    #[test]
    fn full_sticky_views_report_the_overflow_without_changing() {
        let shared = instance(&[1, 2, 3, 4]);
        let mut mv = shared.lock();
        for tile in &mut mv.tiles {
            tile.sticky = true;
        }
        let revision = mv.revision();
        let plan = follow_to(&mut mv, &[9]);
        assert!(plan.is_noop());
        assert_eq!(plan.overflow, 1);
        assert_eq!(mv.member_ids(), vec![1, 2, 3, 4]);
        assert_eq!(mv.revision(), revision);
    }

    #[test]
    fn a_thumbnail_placeholder_fills_a_loading_tile_until_the_decode_arrives() {
        use slint::{Rgb8Pixel, SharedPixelBuffer};
        let shared = instance(&[1]);
        let mut mv = shared.lock();
        assert!(mv.view().tiles[0].is_loading, "a fresh image tile shows the spinner");

        let thumbnail = SharedPixelBuffer::<Rgb8Pixel>::new(8, 6);
        mv.set_placeholder(1, thumbnail);
        let view = mv.view();
        assert!(view.tiles[0].pixels.is_some() && !view.tiles[0].is_loading);

        mv.on_image_loaded(1, Ok((800, 600, SharedPixelBuffer::<Rgb8Pixel>::new(80, 60))));
        let full = mv.view().tiles[0].pixels.clone().expect("the decoded image");
        assert_eq!((full.width(), full.height()), (80, 60));

        mv.set_placeholder(1, SharedPixelBuffer::<Rgb8Pixel>::new(8, 6));
        let still = mv.view().tiles[0].pixels.clone().expect("still the decoded image");
        assert_eq!(still.width(), 80, "a late placeholder never replaces the decoded image");
    }
}
