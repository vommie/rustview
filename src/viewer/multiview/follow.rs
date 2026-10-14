//! How a running Multiview follows the Browse selection (pure, replaces the former slot-stable reconcile).
//!
//! The selection says what the user wants to see, stickiness says what must stay no matter what. Sticky tiles are always
//! kept and occupy slots; tiles that are still wanted keep their slot (running playback, user-set widths); newcomers take
//! the freed slots from the left and are then appended; everything beyond the tile cap is reported as overflow. An empty
//! wish never changes anything, so the Multiview is never empty and a folder change or query leaves it alone.
//!
//! A newcomer that takes the slot of a removed tile inherits what belongs to the slot (the user-set width). If both are
//! videos the player of the removed tile can be recycled for the newcomer (`load_file` instead of a new player).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub id: u64,
    pub sticky: bool,
    pub is_video: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    pub id: u64,
    pub is_video: bool,
}

/// A newcomer that replaces a removed tile in the same slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Takeover {
    pub new_id: u64,
    pub from_id: u64,
    /// Both are videos: the player and window of `from_id` are reused for `new_id`.
    pub recycle_player: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FollowPlan {
    pub remove: Vec<u64>,
    pub add: Vec<u64>,
    /// The resulting tiles in slot order.
    pub slots: Vec<u64>,
    pub takeovers: Vec<Takeover>,
    /// Wanted media that found no free slot because sticky or already shown tiles fill all of them.
    pub overflow: usize,
}

impl FollowPlan {
    pub fn is_noop(&self) -> bool {
        self.remove.is_empty() && self.add.is_empty()
    }
}

pub fn follow(shown: &[Shown], wanted: &[Wanted], cap: usize) -> FollowPlan {
    let current: Vec<u64> = shown.iter().map(|s| s.id).collect();
    if wanted.is_empty() {
        return FollowPlan {
            slots: current,
            ..FollowPlan::default()
        };
    }

    let is_wanted = |id: u64| wanted.iter().any(|w| w.id == id);
    let removed: Vec<&Shown> = shown
        .iter()
        .filter(|s| !s.sticky && !is_wanted(s.id))
        .collect();
    let kept = shown.len() - removed.len();
    let room = cap.saturating_sub(kept);
    let newcomers: Vec<&Wanted> = wanted.iter().filter(|w| !current.contains(&w.id)).collect();
    let added: Vec<&Wanted> = newcomers.iter().copied().take(room).collect();

    let mut fresh = added.iter().map(|w| w.id);
    let mut slots: Vec<Option<u64>> = shown
        .iter()
        .map(|s| {
            if removed.iter().any(|r| r.id == s.id) {
                fresh.next()
            } else {
                Some(s.id)
            }
        })
        .collect();
    slots.extend(fresh.map(Some));

    let takeovers = added
        .iter()
        .zip(&removed)
        .map(|(new, old)| Takeover {
            new_id: new.id,
            from_id: old.id,
            recycle_player: new.is_video && old.is_video,
        })
        .collect();

    FollowPlan {
        remove: removed.iter().map(|s| s.id).collect(),
        add: added.iter().map(|w| w.id).collect(),
        slots: slots.into_iter().flatten().collect(),
        takeovers,
        overflow: newcomers.len() - added.len(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(id: u64) -> Shown {
        Shown {
            id,
            sticky: false,
            is_video: false,
        }
    }

    fn sticky(id: u64) -> Shown {
        Shown {
            sticky: true,
            ..s(id)
        }
    }

    fn video(id: u64) -> Shown {
        Shown {
            is_video: true,
            ..s(id)
        }
    }

    fn w(id: u64) -> Wanted {
        Wanted {
            id,
            is_video: false,
        }
    }

    fn wv(id: u64) -> Wanted {
        Wanted { id, is_video: true }
    }

    fn ws(ids: &[u64]) -> Vec<Wanted> {
        ids.iter().map(|&id| w(id)).collect()
    }

    #[test]
    fn an_unchanged_selection_is_a_noop() {
        let p = follow(&[s(1), s(2), s(3)], &ws(&[1, 2, 3]), 4);
        assert!(p.is_noop());
        assert_eq!(p.slots, vec![1, 2, 3]);
        assert!(p.takeovers.is_empty());
    }

    #[test]
    fn an_empty_wish_never_changes_anything_not_even_sticky_ones() {
        let p = follow(&[s(1), sticky(2)], &[], 4);
        assert!(p.is_noop());
        assert_eq!(p.slots, vec![1, 2]);
        assert_eq!(p.overflow, 0);
    }

    #[test]
    fn newcomers_take_the_freed_slots_from_the_left() {
        let p = follow(&[s(1), s(2), s(3)], &ws(&[2, 3, 9]), 4);
        assert_eq!((p.remove, p.add, p.slots), (vec![1], vec![9], vec![9, 2, 3]));
        assert_eq!(
            p.takeovers,
            vec![Takeover {
                new_id: 9,
                from_id: 1,
                recycle_player: false
            }]
        );
    }

    #[test]
    fn more_removals_than_newcomers_compacts_and_keeps_the_order() {
        let p = follow(&[s(1), s(2), s(3), s(4)], &ws(&[3, 8]), 4);
        assert_eq!(
            (p.remove, p.add, p.slots),
            (vec![1, 2, 4], vec![8], vec![8, 3])
        );
    }

    #[test]
    fn growing_appends_after_the_existing_tiles() {
        let p = follow(&[s(1)], &ws(&[1, 2, 3]), 4);
        assert_eq!((p.add, p.slots), (vec![2, 3], vec![1, 2, 3]));
        assert!(p.takeovers.is_empty(), "nothing was replaced");
    }

    #[test]
    fn a_single_selection_replaces_everything_that_is_not_sticky() {
        let p = follow(&[s(1), s(2)], &ws(&[7]), 4);
        assert_eq!((p.remove, p.add, p.slots), (vec![1, 2], vec![7], vec![7]));
    }

    #[test]
    fn sticky_tiles_stay_even_when_the_selection_moves_on() {
        let p = follow(&[sticky(1), s(2), s(3)], &ws(&[8, 9]), 4);
        assert_eq!(p.remove, vec![2, 3]);
        assert_eq!(p.slots, vec![1, 8, 9]);
    }

    #[test]
    fn sticky_tiles_keep_their_slot_and_newcomers_fill_around_them() {
        let p = follow(&[s(1), sticky(2), s(3)], &ws(&[8]), 4);
        assert_eq!((p.remove, p.slots), (vec![1, 3], vec![8, 2]));
    }

    #[test]
    fn sticky_tiles_reduce_the_room_for_newcomers_and_the_rest_is_overflow() {
        let p = follow(&[sticky(1), sticky(2), s(3)], &ws(&[7, 8, 9]), 4);
        assert_eq!(p.add, vec![7, 8]);
        assert_eq!(p.slots, vec![1, 2, 7, 8]);
        assert_eq!(p.overflow, 1, "9 found no free slot");
    }

    #[test]
    fn a_full_set_of_sticky_tiles_accepts_nothing() {
        let shown = [sticky(1), sticky(2), sticky(3), sticky(4)];
        let p = follow(&shown, &ws(&[9]), 4);
        assert!(p.is_noop());
        assert_eq!((p.slots, p.overflow), (vec![1, 2, 3, 4], 1));
    }

    #[test]
    fn selecting_only_a_sticky_medium_drops_the_unsticky_ones() {
        let p = follow(&[sticky(1), s(2), s(3)], &ws(&[1]), 4);
        assert_eq!((p.remove, p.slots), (vec![2, 3], vec![1]));
    }

    #[test]
    fn the_cap_is_respected_and_shown_tiles_win_over_newcomers() {
        let p = follow(&[s(1), s(2), s(3)], &ws(&[1, 2, 3, 4, 5, 6]), 4);
        assert_eq!((p.add, p.slots, p.overflow), (vec![4], vec![1, 2, 3, 4], 2));
        let p = follow(&[s(1), s(2), s(3), s(4)], &ws(&[5, 6, 1]), 4);
        assert_eq!(
            (p.remove.len(), p.add, p.slots),
            (3, vec![5, 6], vec![1, 5, 6]),
            "the kept tile keeps its slot"
        );
    }

    #[test]
    fn a_video_replacing_a_video_recycles_the_player() {
        let p = follow(&[video(1)], &[wv(2)], 4);
        assert_eq!(
            p.takeovers,
            vec![Takeover {
                new_id: 2,
                from_id: 1,
                recycle_player: true
            }]
        );
    }

    #[test]
    fn mixed_kinds_never_recycle() {
        let p = follow(&[video(1), s(2)], &[w(3), wv(4)], 4);
        assert_eq!(
            p.takeovers,
            vec![
                Takeover {
                    new_id: 3,
                    from_id: 1,
                    recycle_player: false
                },
                Takeover {
                    new_id: 4,
                    from_id: 2,
                    recycle_player: false
                }
            ]
        );
    }

    #[test]
    fn takeovers_follow_the_slot_order_of_the_removed_tiles() {
        let p = follow(&[video(1), s(2), video(3)], &[wv(8), wv(9)], 4);
        assert_eq!(p.remove, vec![1, 2, 3]);
        assert_eq!(p.slots, vec![8, 9]);
        let pairs: Vec<(u64, u64, bool)> = p
            .takeovers
            .iter()
            .map(|t| (t.new_id, t.from_id, t.recycle_player))
            .collect();
        assert_eq!(pairs, vec![(8, 1, true), (9, 2, false)]);
    }

    #[test]
    fn shrinking_to_one_keeps_a_single_tile() {
        let p = follow(&[s(1), s(2), s(3)], &ws(&[2]), 4);
        assert_eq!((p.slots, p.remove), (vec![2], vec![1, 3]));
    }
}
