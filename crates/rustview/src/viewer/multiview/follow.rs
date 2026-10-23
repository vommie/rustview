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
    let removed: Vec<&Shown> = shown.iter().filter(|s| !s.sticky && !is_wanted(s.media)).collect();
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
