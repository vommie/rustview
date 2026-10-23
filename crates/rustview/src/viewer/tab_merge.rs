//! Merging several tabs into one Multiview tab: the decision (pure, no Slint, no X11).
//!
//! Every tab is a Multiview of one to four tiles. A merge puts the tiles of all selected tabs side by side in a new tab, so it
//! works only while they fit into one Multiview. A medium that several tabs show stays one tile per tab (the Multiview can show a
//! medium more than once, see the duplicate button), so what counts is tiles.

use super::types::TabId;

/// One tab that takes part in a merge and the tiles it brings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MergeSource {
    pub tab: TabId,
    pub tiles: usize,
}

/// Why a merge cannot happen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeRefusal {
    /// Fewer than two different tabs were given.
    TooFewTabs,
    /// The tabs hold more tiles than one Multiview shows.
    TooManyMedia {
        total: usize,
        cap: usize,
        /// The tabs in tab order, each with the tiles it brings.
        sources: Vec<MergeSource>,
    },
}

impl MergeRefusal {
    /// How many media have to leave the tabs before the merge fits (0 for any other refusal).
    pub fn excess(&self) -> usize {
        match self {
            Self::TooManyMedia { total, cap, .. } => total.saturating_sub(*cap),
            Self::TooFewTabs => 0,
        }
    }
}

/// Everything that can stop a merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    Refused(MergeRefusal),
    /// A selected tab is shown in the fullscreen window right now (its players live there).
    ShownInFullscreen,
    /// The Multiview machinery does not exist yet (the application is still starting).
    NotReady,
}

/// What a merge did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedTabs {
    /// The new tab.
    pub tab: TabId,
    /// The tabs that were merged and are closed now, in tab order.
    pub closed: Vec<TabId>,
    /// The tiles of the new tab.
    pub tiles: usize,
}

/// Which neighbors of a tab a single-tab merge takes along (the entries "Merge With Left/Right/Left & Right" of its menu).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeSide {
    Left,
    Right,
    Both,
}

/// What the menu of a tab offers for merging, from the tabs that exist next to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMenu {
    /// A single tab: nothing to merge with.
    None,
    /// Only one side has a neighbor: one entry "Merge With Left" or "Merge With Right".
    Entry(MergeSide),
    /// Tabs on both sides: an entry "Merge" with a submenu of all three.
    Submenu,
}

/// The merge entries of the menu of the tab at `index` among `total` tabs.
pub fn merge_menu(index: usize, total: usize) -> MergeMenu {
    match (index > 0, index + 1 < total) {
        (false, false) => MergeMenu::None,
        (true, false) => MergeMenu::Entry(MergeSide::Left),
        (false, true) => MergeMenu::Entry(MergeSide::Right),
        (true, true) => MergeMenu::Submenu,
    }
}

/// The tab `tab` and the neighbors `side` names, in tab order. Empty when `tab` is unknown or a neighbor that was asked for does
/// not exist (the menu does not offer that then).
pub fn with_neighbors(order: &[TabId], tab: TabId, side: MergeSide) -> Vec<TabId> {
    let Some(index) = order.iter().position(|id| *id == tab) else {
        return Vec::new();
    };
    let left = (index > 0).then(|| order[index - 1]);
    let right = order.get(index + 1).copied();
    let (take_left, take_right) = match side {
        MergeSide::Left => (true, false),
        MergeSide::Right => (false, true),
        MergeSide::Both => (true, true),
    };
    if (take_left && left.is_none()) || (take_right && right.is_none()) {
        return Vec::new();
    }
    let mut ids = Vec::with_capacity(3);
    ids.extend(left.filter(|_| take_left));
    ids.push(tab);
    ids.extend(right.filter(|_| take_right));
    ids
}

/// Decides whether `sources` (in tab order) can become one Multiview of at most `cap` tiles. Returns the number of tiles it
/// will have.
pub fn plan(sources: &[MergeSource], cap: usize) -> Result<usize, MergeRefusal> {
    let mut seen: Vec<TabId> = Vec::with_capacity(sources.len());
    for source in sources {
        if !seen.contains(&source.tab) {
            seen.push(source.tab);
        }
    }
    if seen.len() < 2 {
        return Err(MergeRefusal::TooFewTabs);
    }
    let total: usize = sources.iter().map(|s| s.tiles).sum();
    if total > cap {
        return Err(MergeRefusal::TooManyMedia {
            total,
            cap,
            sources: sources.to_vec(),
        });
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(tab: TabId, tiles: usize) -> MergeSource {
        MergeSource { tab, tiles }
    }

    #[test]
    fn two_single_medium_tabs_make_two_tiles() {
        assert_eq!(plan(&[src(1, 1), src(2, 1)], 4), Ok(2));
    }

    #[test]
    fn exactly_the_cap_is_allowed() {
        assert_eq!(plan(&[src(1, 2), src(2, 2)], 4), Ok(4));
        assert_eq!(plan(&[src(1, 1), src(2, 1), src(3, 1), src(4, 1)], 4), Ok(4));
    }

    #[test]
    fn one_medium_too_many_is_refused_with_the_excess() {
        let refusal = plan(&[src(1, 2), src(2, 2), src(3, 1)], 4).unwrap_err();
        assert_eq!(refusal.excess(), 1);
        assert_eq!(
            refusal,
            MergeRefusal::TooManyMedia {
                total: 5,
                cap: 4,
                sources: vec![src(1, 2), src(2, 2), src(3, 1)],
            }
        );
    }

    #[test]
    fn the_excess_counts_every_tile_over_the_cap() {
        assert_eq!(plan(&[src(1, 4), src(2, 4)], 4).unwrap_err().excess(), 4);
        let seven: Vec<MergeSource> = (1..=7).map(|tab| src(tab, 1)).collect();
        assert_eq!(plan(&seven, 4).unwrap_err().excess(), 3);
    }

    #[test]
    fn fewer_than_two_tabs_cannot_be_merged() {
        assert_eq!(plan(&[], 4), Err(MergeRefusal::TooFewTabs));
        assert_eq!(plan(&[src(1, 3)], 4), Err(MergeRefusal::TooFewTabs));
        assert_eq!(plan(&[src(1, 1), src(1, 1)], 4), Err(MergeRefusal::TooFewTabs));
        assert_eq!(MergeRefusal::TooFewTabs.excess(), 0);
    }

    #[test]
    fn the_menu_offers_what_the_neighbors_allow() {
        assert_eq!(merge_menu(0, 1), MergeMenu::None);
        assert_eq!(merge_menu(0, 3), MergeMenu::Entry(MergeSide::Right));
        assert_eq!(merge_menu(2, 3), MergeMenu::Entry(MergeSide::Left));
        assert_eq!(merge_menu(1, 3), MergeMenu::Submenu);
        assert_eq!(merge_menu(1, 2), MergeMenu::Entry(MergeSide::Left));
        assert_eq!(merge_menu(0, 2), MergeMenu::Entry(MergeSide::Right));
    }

    #[test]
    fn the_neighbors_are_the_adjacent_tabs_in_tab_order() {
        let order = [10, 11, 12, 13];
        assert_eq!(with_neighbors(&order, 12, MergeSide::Left), vec![11, 12]);
        assert_eq!(with_neighbors(&order, 12, MergeSide::Right), vec![12, 13]);
        assert_eq!(with_neighbors(&order, 12, MergeSide::Both), vec![11, 12, 13]);
        assert_eq!(with_neighbors(&order, 10, MergeSide::Right), vec![10, 11]);
        assert_eq!(with_neighbors(&order, 13, MergeSide::Left), vec![12, 13]);
    }

    #[test]
    fn a_neighbor_that_does_not_exist_gives_nothing() {
        let order = [10, 11, 12];
        assert_eq!(with_neighbors(&order, 10, MergeSide::Left), Vec::<TabId>::new());
        assert_eq!(with_neighbors(&order, 12, MergeSide::Right), Vec::<TabId>::new());
        assert_eq!(with_neighbors(&order, 10, MergeSide::Both), Vec::<TabId>::new());
        assert_eq!(with_neighbors(&order, 99, MergeSide::Left), Vec::<TabId>::new());
        assert_eq!(with_neighbors(&[], 10, MergeSide::Right), Vec::<TabId>::new());
    }

    #[test]
    fn a_medium_in_several_tabs_counts_once_per_tab() {
        // two tabs of the same medium: two tiles, the Multiview can show a medium twice
        assert_eq!(plan(&[src(1, 1), src(2, 1)], 4), Ok(2));
        assert_eq!(plan(&[src(1, 3), src(2, 3)], 4).unwrap_err().excess(), 2);
    }
}
