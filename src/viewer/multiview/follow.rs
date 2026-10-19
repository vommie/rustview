//! How a running Multiview changes its set of tiles (pure): following the Browse selection, and showing a medium a second
//! time.
//!
//! The selection says what the user wants to see, stickiness says what must stay no matter what. Sticky tiles are always
//! kept and occupy slots; tiles that are still wanted keep their slot (running playback, user-set widths); newcomers take
//! the freed slots from the left and are then appended; everything beyond the tile cap is reported as overflow. An empty
//! wish never changes anything, so the Multiview is never empty and a folder change or query leaves it alone.
//!
//! A tile is identified by its own key, never by its medium: one medium can be shown by several tiles (a duplicate). A
//! medium counts as wanted for all of its tiles, so they stay and leave together unless one of them is sticky. Duplicating
//! uses the same plan type, so applying it (layout, players, sound) is the one code path of every change of the tile set.
//!
//! A newcomer that takes the slot of a removed tile inherits what belongs to the slot (the user-set width). If both are
//! videos the player of the removed tile can be recycled for the newcomer (`load_file` instead of a new player).

/// Identity of one tile, unique for the life of the process. Two tiles that show the same medium have different keys.
pub type TileKey = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    pub key: TileKey,
    pub media: u64,
    pub sticky: bool,
    pub is_video: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    pub id: u64,
    pub is_video: bool,
}

/// One place of the resulting arrangement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// An existing tile stays.
    Keep(TileKey),
    /// A new tile: the entry of `FollowPlan::add` with this index.
    New(usize),
}

/// A newcomer that replaces a removed tile in the same slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Takeover {
    /// Index into `FollowPlan::add`.
    pub add: usize,
    pub from: TileKey,
    /// Both are videos: the player and window of `from` are reused for the newcomer.
    pub recycle_player: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FollowPlan {
    pub remove: Vec<TileKey>,
    /// The media of the new tiles, in the order of their `Slot::New` entries.
    pub add: Vec<u64>,
    /// The resulting tiles in slot order.
    pub slots: Vec<Slot>,
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
    let keep_all = || shown.iter().map(|s| Slot::Keep(s.key)).collect();
    if wanted.is_empty() {
        return FollowPlan {
            slots: keep_all(),
            ..FollowPlan::default()
        };
    }

    let is_wanted = |media: u64| wanted.iter().any(|w| w.id == media);
    let removed: Vec<&Shown> = shown
        .iter()
        .filter(|s| !s.sticky && !is_wanted(s.media))
        .collect();
    let kept = shown.len() - removed.len();
    let room = cap.saturating_sub(kept);
    let newcomers: Vec<&Wanted> = wanted
        .iter()
        .filter(|w| !shown.iter().any(|s| s.media == w.id))
        .collect();
    let added: Vec<&Wanted> = newcomers.iter().copied().take(room).collect();

    let mut fresh = 0..added.len();
    let mut slots: Vec<Slot> = shown
        .iter()
        .filter_map(|s| {
            if removed.iter().any(|r| r.key == s.key) {
                fresh.next().map(Slot::New)
            } else {
                Some(Slot::Keep(s.key))
            }
        })
        .collect();
    slots.extend(fresh.map(Slot::New));

    let takeovers = added
        .iter()
        .zip(&removed)
        .enumerate()
        .map(|(add, (new, old))| Takeover {
            add,
            from: old.key,
            recycle_player: new.is_video && old.is_video,
        })
        .collect();

    FollowPlan {
        remove: removed.iter().map(|s| s.key).collect(),
        add: added.iter().map(|w| w.id).collect(),
        slots,
        takeovers,
        overflow: newcomers.len() - added.len(),
    }
}

/// Why a medium cannot be shown a second time right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateRefusal {
    /// The view already shows `cap` tiles.
    Full,
    /// The tile shows an error instead of its medium; there is nothing to show a second time.
    Broken,
    /// The tile is not part of the view (a stale click).
    UnknownTile,
}

/// The single rule of the duplicate button: whether a tile can be shown a second time. Used by the button (enablement and
/// tooltip) and by `duplicate`, so what the button promises is what happens.
pub fn duplicate_refusal(tile_count: usize, broken: bool, cap: usize) -> Option<DuplicateRefusal> {
    if tile_count >= cap {
        Some(DuplicateRefusal::Full)
    } else if broken {
        Some(DuplicateRefusal::Broken)
    } else {
        None
    }
}

/// The plan that shows the medium of tile `source` a second time, in the slot right next to it. Every other tile keeps its
/// slot, its playback and its width.
pub fn duplicate(shown: &[Shown], source: TileKey, broken: bool, cap: usize) -> Result<FollowPlan, DuplicateRefusal> {
    let position = shown
        .iter()
        .position(|s| s.key == source)
        .ok_or(DuplicateRefusal::UnknownTile)?;
    if let Some(refusal) = duplicate_refusal(shown.len(), broken, cap) {
        return Err(refusal);
    }
    let mut slots: Vec<Slot> = shown.iter().map(|s| Slot::Keep(s.key)).collect();
    slots.insert(position + 1, Slot::New(0));
    Ok(FollowPlan {
        add: vec![shown[position].media],
        slots,
        ..FollowPlan::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tile(key: u64, media: u64) -> Shown {
        Shown { key, media, sticky: false, is_video: false }
    }

    fn wanted(ids: &[u64]) -> Vec<Wanted> {
        ids.iter().map(|&id| Wanted { id, is_video: false }).collect()
    }

    fn keys(plan: &FollowPlan) -> Vec<Option<TileKey>> {
        plan.slots
            .iter()
            .map(|s| match s {
                Slot::Keep(key) => Some(*key),
                Slot::New(_) => None,
            })
            .collect()
    }

    #[test]
    fn empty_wish_changes_nothing() {
        let plan = follow(&[tile(1, 10), tile(2, 11)], &[], 4);
        assert!(plan.is_noop());
        assert_eq!(keys(&plan), [Some(1), Some(2)]);
    }

    #[test]
    fn newcomers_take_freed_slots_then_append() {
        let shown = [tile(1, 10), tile(2, 11), tile(3, 12)];
        let plan = follow(&shown, &wanted(&[11, 20, 21]), 4);
        assert_eq!(plan.remove, [1, 3]);
        assert_eq!(plan.add, [20, 21]);
        assert_eq!(plan.slots, [Slot::New(0), Slot::Keep(2), Slot::New(1)]);
        assert_eq!(plan.takeovers.len(), 2);
        assert_eq!((plan.takeovers[0].add, plan.takeovers[0].from), (0, 1));
    }

    #[test]
    fn overflow_is_reported() {
        let shown = [tile(1, 10), tile(2, 11), tile(3, 12), tile(4, 13)];
        let plan = follow(&shown, &wanted(&[10, 11, 12, 13, 20, 21]), 4);
        assert!(plan.is_noop());
        assert_eq!(plan.overflow, 2);
    }

    #[test]
    fn sticky_tiles_stay_and_take_a_slot() {
        let mut shown = vec![tile(1, 10), tile(2, 11)];
        shown[0].sticky = true;
        let plan = follow(&shown, &wanted(&[20, 21, 22, 23]), 4);
        assert_eq!(plan.remove, [2]);
        assert_eq!(plan.add.len(), 3);
        assert_eq!(plan.overflow, 1);
        assert_eq!(plan.slots[0], Slot::Keep(1));
    }

    #[test]
    fn a_medium_shown_twice_stays_while_it_is_wanted() {
        let shown = [tile(1, 10), tile(2, 10), tile(3, 11)];
        let plan = follow(&shown, &wanted(&[10, 11]), 4);
        assert!(plan.is_noop());
        assert_eq!(keys(&plan), [Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn both_copies_leave_when_their_medium_is_no_longer_wanted() {
        let shown = [tile(1, 10), tile(2, 10), tile(3, 11)];
        let plan = follow(&shown, &wanted(&[11, 20]), 4);
        assert_eq!(plan.remove, [1, 2]);
        assert_eq!(plan.add, [20]);
        assert_eq!(plan.slots, [Slot::New(0), Slot::Keep(3)]);
    }

    #[test]
    fn a_sticky_copy_outlives_the_selection_and_its_twin_leaves() {
        let mut shown = vec![tile(1, 10), tile(2, 10)];
        shown[1].sticky = true;
        let plan = follow(&shown, &wanted(&[20]), 4);
        assert_eq!(plan.remove, [1]);
        assert_eq!(plan.slots, [Slot::New(0), Slot::Keep(2)]);
    }

    #[test]
    fn copies_occupy_slots_of_the_cap() {
        let shown = [tile(1, 10), tile(2, 10)];
        let plan = follow(&shown, &wanted(&[10, 20, 21, 22]), 4);
        assert_eq!(plan.add, [20, 21]);
        assert_eq!(plan.overflow, 1);
    }

    #[test]
    fn a_shown_medium_is_never_a_newcomer() {
        let shown = [tile(1, 10), tile(2, 10)];
        let plan = follow(&shown, &wanted(&[10]), 4);
        assert!(plan.is_noop());
        assert_eq!(plan.overflow, 0);
    }

    #[test]
    fn takeover_recycles_only_video_for_video() {
        let mut shown = vec![tile(1, 10)];
        shown[0].is_video = true;
        let plan = follow(&shown, &[Wanted { id: 20, is_video: true }], 4);
        assert!(plan.takeovers[0].recycle_player);
        let plan = follow(&shown, &[Wanted { id: 20, is_video: false }], 4);
        assert!(!plan.takeovers[0].recycle_player);
    }

    #[test]
    fn duplicate_goes_right_next_to_its_source() {
        let shown = [tile(1, 10), tile(2, 11), tile(3, 12)];
        let plan = duplicate(&shown, 2, false, 4).expect("room for one more");
        assert_eq!(plan.add, [11]);
        assert_eq!(plan.slots, [Slot::Keep(1), Slot::Keep(2), Slot::New(0), Slot::Keep(3)]);
        assert!(plan.remove.is_empty() && plan.takeovers.is_empty());
        assert!(!plan.is_noop());
    }

    #[test]
    fn a_lone_medium_can_be_duplicated_up_to_the_cap() {
        let mut shown = vec![tile(1, 10)];
        for next in 2..=4 {
            let plan = duplicate(&shown, 1, false, 4).expect("room");
            assert_eq!(plan.slots.len(), shown.len() + 1);
            shown.push(tile(next, 10));
        }
        assert_eq!(duplicate(&shown, 1, false, 4), Err(DuplicateRefusal::Full));
    }

    #[test]
    fn duplicate_refuses_broken_and_unknown_tiles() {
        let shown = [tile(1, 10)];
        assert_eq!(duplicate(&shown, 1, true, 4), Err(DuplicateRefusal::Broken));
        assert_eq!(duplicate(&shown, 9, false, 4), Err(DuplicateRefusal::UnknownTile));
    }

    #[test]
    fn a_full_view_wins_over_a_broken_tile_in_the_message() {
        assert_eq!(duplicate_refusal(4, true, 4), Some(DuplicateRefusal::Full));
        assert_eq!(duplicate_refusal(3, true, 4), Some(DuplicateRefusal::Broken));
        assert_eq!(duplicate_refusal(3, false, 4), None);
    }
}
