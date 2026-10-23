//! Tile widths the user set by dragging a gutter (pure).
//!
//! A pin belongs to a tile, not to a slot or to the whole arrangement: it says how much of its row's width the tile
//! wants. Tiles without a pin stay automatic and share what the pinned tiles leave over, so a tile the user never
//! resized adapts whenever media are added or removed, while a resized (typically sticky) tile keeps its width. If the
//! pinned tiles want more than the row can give, they are scaled down so that every automatic tile keeps a minimum. If the
//! pins already fill the whole row (typically right after a drag between two tiles), a tile that joins is a newcomer
//! without a share: all pinned tiles then give up the same fraction, so their proportions survive and the newcomer gets
//! the same share as everybody else instead of a sliver.

use super::layout::{Gutter, GutterKind, Layout, LayoutParams, fair_share, fit_cap, proportional};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pins {
    /// Per tile: its share of the usable width of its row (`None` = automatic).
    pub cols: Vec<Option<f64>>,
    /// Heights of the two rows of a 2x2 grid as shares of the available height (`None` = automatic).
    pub rows: Option<[f64; 2]>,
}

/// Pinned shares that add up to at least this much leave no room: the row counts as completely set by the user.
const FULL_ROW_SHARE: f64 = 0.98;

fn valid_share(share: &f64) -> bool {
    share.is_finite() && *share > 0.0
}

impl Pins {
    /// The pinned share of a tile; garbage (NaN, zero, negative) counts as automatic.
    pub fn col(&self, tile: usize) -> Option<f64> {
        self.cols.get(tile).copied().flatten().filter(valid_share)
    }

    pub fn is_auto(&self) -> bool {
        self.rows.is_none() && (0..self.cols.len()).all(|i| self.col(i).is_none())
    }
}

/// Makes every width at least `floor` (taking the pixels from the widest elements) and the sum exactly `total`.
fn settle(mut widths: Vec<i32>, total: i32, floor: i32) -> Vec<i32> {
    for w in &mut widths {
        *w = (*w).max(floor);
    }
    let mut diff = total - widths.iter().sum::<i32>();
    if diff > 0 {
        if let Some(last) = widths.last_mut() {
            *last += diff;
        }
        return widths;
    }
    while diff < 0 {
        let widest = (0..widths.len())
            .filter(|&i| widths[i] > floor)
            .max_by_key(|&i| widths[i]);
        let Some(i) = widest else { break };
        let take = (-diff).min(widths[i] - floor);
        widths[i] -= take;
        diff += take;
    }
    widths
}

/// Column widths of one row. `caps` are the widths at which each medium is fitted exactly (the automatic share),
/// `pins` the per-tile pinned shares, `floor` the smallest column. The result always sums to `usable`.
pub fn split_columns(usable: i32, caps: &[i32], pins: &[Option<f64>], floor: i32) -> Vec<i32> {
    let n = caps.len();
    if n == 0 {
        return Vec::new();
    }
    let usable = usable.max(n as i32);
    let wants: Vec<Option<f64>> = (0..n)
        .map(|i| pins.get(i).copied().flatten().filter(valid_share))
        .collect();
    if n == 1 || wants.iter().all(Option::is_none) {
        return fair_share(usable, caps);
    }
    let floor = floor.clamp(1, (usable / n as i32).max(1));

    if wants.iter().all(Option::is_some) {
        let total: f64 = wants.iter().flatten().sum();
        let shares: Vec<f64> = wants.iter().map(|w| w.unwrap_or(0.0) / total).collect();
        return settle(proportional(usable, &shares), usable, floor);
    }

    let free = wants.iter().filter(|w| w.is_none()).count() as i32;
    let pinned_share: f64 = wants.iter().flatten().sum();
    let reserve = if pinned_share >= FULL_ROW_SHARE {
        let equal = usable / n as i32;
        (0..n)
            .filter(|&i| wants[i].is_none())
            .map(|i| caps[i].min(equal))
            .sum::<i32>()
            .max(free * floor)
    } else {
        free * floor
    };
    let budget = (usable - reserve).max(1);
    let mut pinned: Vec<i32> = wants
        .iter()
        .flatten()
        .map(|share| (share * f64::from(usable)).round() as i32)
        .collect();
    let wanted: i32 = pinned.iter().sum();
    if wanted > budget {
        for w in &mut pinned {
            *w = (f64::from(*w) * f64::from(budget) / f64::from(wanted)).round() as i32;
        }
    }
    let taken: i32 = pinned.iter().sum();
    let free_caps: Vec<i32> = (0..n).filter(|&i| wants[i].is_none()).map(|i| caps[i]).collect();
    let mut shared = fair_share(usable - taken, &free_caps).into_iter();
    let mut pinned = pinned.into_iter();
    let widths = (0..n)
        .map(|i| {
            if wants[i].is_some() {
                pinned.next().unwrap_or(floor)
            } else {
                shared.next().unwrap_or(floor)
            }
        })
        .collect();
    settle(widths, usable, floor)
}

/// Pins after the user dragged `gutter` by `delta` pixels from the arrangement `layout`. The two tiles next to a vertical
/// gutter get pinned (both neighbors keep at least the minimum column and the result snaps to "neighbor exactly fitted"
/// within `snap` pixels), a horizontal gutter pins the row heights. Every other pin stays as it was.
pub fn drag_pins(
    before: &Pins,
    layout: &Layout,
    gutter: &Gutter,
    delta: i32,
    p: &LayoutParams,
    aspects: &[f32],
    snap: i32,
) -> Pins {
    let mut next = before.clone();
    next.cols.resize(layout.tiles.len(), None);
    match gutter.kind {
        GutterKind::Vertical => {
            let Some(row) = layout.rows.get(gutter.row) else {
                return next;
            };
            let (Some(&left), Some(&right)) = (row.get(gutter.left), row.get(gutter.left + 1)) else {
                return next;
            };
            let usable: i32 = row.iter().map(|&i| layout.tiles[i].column.w).sum();
            let left_w = layout.tiles[left].column.w;
            let pair = left_w + layout.tiles[right].column.w;
            let min_col = p.min_column.min(pair / 3).max(1);
            let row_h = layout.tiles[left].column.h;
            let mut new_left = (left_w + delta).clamp(min_col, (pair - min_col).max(min_col));
            let left_fit = aspects.get(left).map_or(0, |&a| fit_cap(row_h, a));
            let right_fit = aspects.get(right).map_or(0, |&a| fit_cap(row_h, a));
            if (new_left - left_fit).abs() <= snap {
                new_left = left_fit;
            } else if ((pair - new_left) - right_fit).abs() <= snap {
                new_left = pair - right_fit;
            }
            let new_left = new_left.clamp(min_col, (pair - min_col).max(min_col));
            let usable = f64::from(usable.max(1));
            next.cols[left] = Some(f64::from(new_left) / usable);
            next.cols[right] = Some(f64::from(pair - new_left) / usable);
        }
        GutterKind::ColumnSwap => {}
        GutterKind::Horizontal => {
            let (Some(top_row), true) = (layout.rows.first(), layout.rows.len() == 2) else {
                return next;
            };
            let Some(&first) = top_row.first() else {
                return next;
            };
            let available = (p.height - p.gutter).max(2);
            let top = layout.tiles[first].column.h;
            let min_row = p.min_column.min(available / 3).max(1);
            let new_top = (top + delta).clamp(min_row, (available - min_row).max(min_row));
            next.rows = Some([
                f64::from(new_top) / f64::from(available),
                f64::from(available - new_top) / f64::from(available),
            ]);
        }
    }
    next
}
