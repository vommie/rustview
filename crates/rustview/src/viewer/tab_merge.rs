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
    fn a_medium_in_several_tabs_counts_once_per_tab() {
        // two tabs of the same medium: two tiles, the Multiview can show a medium twice
        assert_eq!(plan(&[src(1, 1), src(2, 1)], 4), Ok(2));
        assert_eq!(plan(&[src(1, 3), src(2, 3)], 4).unwrap_err().excess(), 2);
    }
}
