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
