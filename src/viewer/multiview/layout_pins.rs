//! Tile widths the user set by dragging a gutter (pure).
//!
//! A pin belongs to a tile, not to a slot or to the whole arrangement: it says how much of its row's width the tile
//! wants. Tiles without a pin stay automatic and share what the pinned tiles leave over, so a tile the user never
//! resized adapts whenever media are added or removed, while a resized (typically sticky) tile keeps its width. If the
//! pinned tiles want more than the row can give, they are scaled down so that every automatic tile keeps a minimum.

use super::layout::{Gutter, GutterKind, Layout, LayoutParams, fair_share, fit_cap, proportional};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pins {
    /// Per tile: its share of the usable width of its row (`None` = automatic).
    pub cols: Vec<Option<f64>>,
    /// Heights of the two rows of a 2x2 grid as shares of the available height (`None` = automatic).
    pub rows: Option<[f64; 2]>,
}

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
    let budget = (usable - free * floor).max(1);
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
    let free_caps: Vec<i32> = (0..n)
        .filter(|&i| wants[i].is_none())
        .map(|i| caps[i])
        .collect();
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
            let (Some(&left), Some(&right)) = (row.get(gutter.left), row.get(gutter.left + 1))
            else {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::layout::{LayoutParams, Split, solve};

    const P: LayoutParams = LayoutParams {
        width: 1920,
        height: 1080,
        gutter: 16,
        min_column: 160,
        min_wide_aspect: 1.2,
    };
    const W169: f32 = 16.0 / 9.0;
    const P916: f32 = 9.0 / 16.0;

    #[test]
    fn without_pins_the_automatic_share_applies() {
        let caps = [300, 900, 500];
        assert_eq!(
            split_columns(1000, &caps, &[None, None, None], 160),
            fair_share(1000, &caps)
        );
        assert_eq!(
            split_columns(1000, &caps, &[], 160),
            fair_share(1000, &caps)
        );
    }

    #[test]
    fn a_pinned_tile_keeps_its_share_and_the_rest_adapts() {
        let caps = [900, 900, 900];
        assert_eq!(
            split_columns(1000, &caps, &[Some(0.5), None, None], 100),
            vec![500, 250, 250]
        );
        assert_eq!(
            split_columns(1000, &caps, &[None, Some(0.6), None], 100),
            vec![200, 600, 200]
        );
        assert_eq!(
            split_columns(1000, &[900, 900], &[Some(0.7), None], 100),
            vec![700, 300]
        );
    }

    #[test]
    fn a_new_automatic_tile_leaves_the_pinned_width_alone() {
        let two = split_columns(1000, &[900, 900], &[Some(0.7), None], 100);
        let three = split_columns(1000, &[900, 900, 900], &[Some(0.7), None, None], 100);
        assert_eq!(two[0], three[0], "the pinned tile does not shrink");
        assert_eq!(three[1] + three[2], 300);
    }

    #[test]
    fn pins_that_want_too_much_are_scaled_so_automatic_tiles_keep_the_minimum() {
        let w = split_columns(1000, &[900; 3], &[Some(0.9), None, None], 100);
        assert_eq!(w.iter().sum::<i32>(), 1000);
        assert_eq!(w, vec![800, 100, 100]);
        let w = split_columns(1000, &[900; 3], &[Some(0.5), Some(0.5), None], 100);
        assert_eq!(w, vec![450, 450, 100]);
    }

    #[test]
    fn fully_pinned_rows_are_normalized_and_keep_their_proportions() {
        assert_eq!(
            split_columns(1000, &[900, 900], &[Some(0.2), Some(0.2)], 100),
            vec![500, 500]
        );
        assert_eq!(
            split_columns(1000, &[900, 900], &[Some(0.2), Some(0.6)], 100),
            vec![250, 750]
        );
    }

    #[test]
    fn garbage_pins_count_as_automatic() {
        let caps = [900, 900, 900];
        let auto = fair_share(1000, &caps);
        for bad in [f64::NAN, f64::INFINITY, -0.5, 0.0] {
            assert_eq!(
                split_columns(1000, &caps, &[Some(bad), None, None], 100),
                auto,
                "{bad}"
            );
        }
    }

    #[test]
    fn a_single_tile_ignores_its_pin() {
        assert_eq!(split_columns(1000, &[400], &[Some(0.3)], 100), vec![1000]);
    }

    #[test]
    fn the_result_always_fills_the_row_and_respects_the_minimum() {
        let shares = [None, Some(0.05), Some(0.3), Some(0.7), Some(0.95), Some(3.0)];
        for usable in [400, 1000, 1904] {
            for n in 2..=4usize {
                let caps = vec![usable / 2; n];
                for mask in 0..shares.len().pow(n as u32) {
                    let pins: Vec<Option<f64>> = (0..n)
                        .map(|k| shares[(mask / shares.len().pow(k as u32)) % shares.len()])
                        .collect();
                    let w = split_columns(usable, &caps, &pins, 60);
                    assert_eq!(w.iter().sum::<i32>(), usable, "{pins:?}");
                    assert!(w.iter().all(|&x| x >= 60), "{pins:?} -> {w:?}");
                }
            }
        }
    }

    #[test]
    fn dragging_pins_exactly_the_two_neighbors() {
        let aspects = [W169, W169, W169];
        let layout = solve(&P, &aspects, &Split::Auto);
        let pins = drag_pins(&Pins::default(), &layout, &layout.gutters[0], 100, &P, &aspects, 8);
        assert_eq!(pins.cols.len(), 3);
        let (a, b) = (pins.col(0).unwrap_or(0.0), pins.col(1).unwrap_or(0.0));
        let pair = f64::from(layout.tiles[0].column.w + layout.tiles[1].column.w);
        let usable: f64 = layout.tiles.iter().map(|t| f64::from(t.column.w)).sum();
        assert!((a + b - pair / usable).abs() < 1e-9, "the pair keeps its total");
        assert!(a > f64::from(layout.tiles[0].column.w) / usable, "the left tile grew");
        assert_eq!(pins.col(2), None, "the third tile stays automatic");
        assert!(pins.rows.is_none());
    }

    #[test]
    fn a_drag_is_clamped_to_the_minimum_column() {
        let aspects = [P916, W169];
        let layout = solve(&P, &aspects, &Split::Auto);
        let gutter = layout.gutters[0];
        for delta in [-9999, 9999] {
            let pins = drag_pins(&Pins::default(), &layout, &gutter, delta, &P, &aspects, 0);
            let usable: i32 = layout.tiles.iter().map(|t| t.column.w).sum();
            let caps: Vec<i32> = vec![900, 900];
            let w = split_columns(usable, &caps, &[pins.col(0), pins.col(1)], P.min_column);
            assert!(w.iter().all(|&x| x >= P.min_column), "{delta}: {w:?}");
            assert_eq!(w.iter().sum::<i32>(), usable);
        }
    }

    #[test]
    fn a_drag_snaps_to_an_exactly_fitted_neighbor() {
        let aspects = [P916, W169];
        let layout = solve(&P, &aspects, &Split::Auto);
        let fitted = fit_cap(1080, P916);
        let delta = fitted - layout.tiles[0].column.w - 5;
        let pins = drag_pins(&Pins::default(), &layout, &layout.gutters[0], delta, &P, &aspects, 8);
        let usable: i32 = layout.tiles.iter().map(|t| t.column.w).sum();
        let left = (pins.col(0).unwrap_or(0.0) * f64::from(usable)).round() as i32;
        assert_eq!(left, fitted);
    }

    #[test]
    fn dragging_the_horizontal_gutter_pins_the_row_heights() {
        let aspects = [W169; 4];
        let layout = solve(&P, &aspects, &Split::Auto);
        let gutter = layout
            .gutters
            .iter()
            .copied()
            .find(|g| g.kind == GutterKind::Horizontal)
            .expect("a 2x2 grid has a horizontal gutter");
        let pins = drag_pins(&Pins::default(), &layout, &gutter, 80, &P, &aspects, 8);
        let [top, bottom] = pins.rows.expect("row heights are pinned");
        assert!((top + bottom - 1.0).abs() < 1e-9);
        assert!(top > bottom, "the top row grew");
        assert!(pins.cols.iter().all(Option::is_none), "no column was touched");
    }

    #[test]
    fn existing_pins_of_other_tiles_survive_a_drag() {
        let aspects = [W169, W169, W169, W169];
        let layout = solve(&P, &aspects, &Split::Auto);
        let before = Pins {
            cols: vec![None, None, Some(0.4), None],
            rows: None,
        };
        let pins = drag_pins(&before, &layout, &layout.gutters[0], 40, &P, &aspects, 8);
        assert_eq!(pins.col(2), Some(0.4));
    }

    #[test]
    fn a_stale_gutter_changes_nothing() {
        let aspects = [W169, W169];
        let layout = solve(&P, &aspects, &Split::Auto);
        let stale = Gutter {
            kind: GutterKind::Vertical,
            rect: layout.gutters[0].rect,
            row: 5,
            left: 9,
        };
        let pins = drag_pins(&Pins::default(), &layout, &stale, 100, &P, &aspects, 8);
        assert!(pins.is_auto());
    }
}
