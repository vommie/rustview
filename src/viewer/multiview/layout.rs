//! Pure layout solver of the Multiview: integer physical pixels, no Slint/X11/mpv dependencies.
//!
//! One row with 1..=4 tiles is the normal case; exactly four widescreen media form a 2x2 grid. Every medium is fitted
//! (contain) into its column and aligned towards the window center, so media with spare room sit directly next to
//! each other instead of drifting apart. Column widths follow "fair share with fit cap" (water filling); widths the user
//! pinned by dragging a gutter come from `layout_pins`.

use super::layout_pins::{Pins, split_columns};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutParams {
    pub width: i32,
    pub height: i32,
    pub gutter: i32,
    pub min_column: i32,
    pub min_wide_aspect: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TileLayout {
    /// Space reserved for the tile (full row height, column width).
    pub column: Rect,
    /// The fitted medium inside the column; this is also the rectangle of the X11 video window.
    pub content: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GutterKind {
    Vertical,
    Horizontal,
}

/// A draggable separator. A vertical gutter separates tiles `left` and `left + 1` of `row`; the horizontal gutter
/// separates row 0 from row 1 (2x2 only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gutter {
    pub kind: GutterKind,
    pub rect: Rect,
    pub row: usize,
    pub left: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Layout {
    pub tiles: Vec<TileLayout>,
    pub gutters: Vec<Gutter>,
    /// Tile indices of each row (one row, or two rows for 2x2).
    pub rows: Vec<Vec<usize>>,
}

/// Distributes `total` pixels over elements that each want at most `caps[i]` pixels. If everything fits, the elements
/// get exactly their cap and the spare room goes half to the first and half to the last element (so the group stays
/// centered). Otherwise small elements keep their cap and the rest share equally (water filling).
pub fn fair_share(total: i32, caps: &[i32]) -> Vec<i32> {
    let n = caps.len();
    if n == 0 {
        return Vec::new();
    }
    let total = total.max(0);
    let sum: i64 = caps.iter().map(|&c| i64::from(c)).sum();
    if sum <= i64::from(total) {
        let mut out = caps.to_vec();
        let rest = total - sum as i32;
        out[0] += rest / 2;
        out[n - 1] += rest - rest / 2;
        return out;
    }

    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by_key(|&i| caps[i]);
    let mut out = vec![0; n];
    let mut done = vec![false; n];
    let mut rest = total;
    let mut open = n as i32;
    for &i in &order {
        let share = rest / open;
        if caps[i] > share {
            break;
        }
        out[i] = caps[i];
        done[i] = true;
        rest -= caps[i];
        open -= 1;
    }
    let remaining: Vec<usize> = (0..n).filter(|&i| !done[i]).collect();
    if remaining.is_empty() {
        return out;
    }
    let count = remaining.len() as i32;
    let (base, extra) = (rest / count, rest % count);
    for (rank, &i) in remaining.iter().enumerate() {
        out[i] = base + i32::from((rank as i32) < extra);
    }
    out
}

/// Largest rectangle with the given aspect ratio (width / height) that fits into the box (contain).
pub fn fit(aspect: f32, box_w: i32, box_h: i32) -> (i32, i32) {
    let a = if aspect.is_finite() && aspect > 0.0 {
        f64::from(aspect)
    } else {
        16.0 / 9.0
    };
    let (bw, bh) = (box_w.max(1), box_h.max(1));
    let w = (f64::from(bh) * a).round().min(f64::from(bw));
    let h = (w / a).round().min(f64::from(bh));
    ((w as i32).max(1), (h as i32).max(1))
}

pub(super) fn fit_cap(row_h: i32, aspect: f32) -> i32 {
    let a = if aspect.is_finite() && aspect > 0.0 {
        f64::from(aspect)
    } else {
        16.0 / 9.0
    };
    (f64::from(row_h.max(1)) * a).round() as i32
}

/// Splits `total` by proportions; the last element absorbs the rounding remainder.
pub(super) fn proportional(total: i32, fractions: &[f64]) -> Vec<i32> {
    let n = fractions.len();
    let mut out: Vec<i32> = fractions
        .iter()
        .map(|f| (f64::from(total) * f).round() as i32)
        .collect();
    if n > 0 {
        let head: i32 = out[..n - 1].iter().sum();
        out[n - 1] = (total - head).max(1);
    }
    out
}

/// One row of tiles: columns by pinned shares or fair share, each tile fitted and aligned towards the center.
#[allow(clippy::too_many_arguments)]
fn solve_row(
    x0: i32,
    y0: i32,
    total_w: i32,
    row_h: i32,
    gutter: i32,
    min_column: i32,
    aspects: &[f32],
    pins: &[Option<f64>],
) -> Vec<TileLayout> {
    let n = aspects.len();
    let usable = (total_w - (n as i32 - 1) * gutter).max(n as i32);
    let caps: Vec<i32> = aspects.iter().map(|&a| fit_cap(row_h, a)).collect();
    let columns = split_columns(usable, &caps, pins, min_column);

    let mut out = Vec::with_capacity(n);
    let mut x = x0;
    for (i, (&aspect, &col_w)) in aspects.iter().zip(&columns).enumerate() {
        let (w, h) = fit(aspect, col_w, row_h);
        let content_x = if n == 1 || (n % 2 == 1 && i == n / 2) {
            x + (col_w - w) / 2
        } else if i < n / 2 {
            x + (col_w - w)
        } else {
            x
        };
        out.push(TileLayout {
            column: Rect::new(x, y0, col_w, row_h),
            content: Rect::new(content_x, y0 + (row_h - h) / 2, w, h),
        });
        x += col_w + gutter;
    }
    out
}

/// Row partition: exactly four widescreen media make a 2x2 grid, everything else stays in a single row.
pub fn partition(aspects: &[f32], min_wide_aspect: f32) -> Vec<Vec<usize>> {
    if aspects.len() == 4 && aspects.iter().all(|&a| a >= min_wide_aspect) {
        vec![vec![0, 1], vec![2, 3]]
    } else {
        vec![(0..aspects.len()).collect()]
    }
}

/// Solves the whole layout. Pinned widths (and, in a 2x2 grid, row heights) take precedence over the automatic share.
pub fn solve(p: &LayoutParams, aspects: &[f32], pins: &Pins) -> Layout {
    if aspects.is_empty() {
        return Layout::default();
    }
    let rows = partition(aspects, p.min_wide_aspect);
    let row_pins = |row: &[usize]| -> Vec<Option<f64>> { row.iter().map(|&i| pins.col(i)).collect() };
    let mut tiles = vec![TileLayout::default(); aspects.len()];
    let mut gutters = Vec::new();

    if rows.len() == 1 {
        let sub = solve_row(
            0,
            0,
            p.width,
            p.height,
            p.gutter,
            p.min_column,
            aspects,
            &row_pins(&rows[0]),
        );
        for (i, t) in sub.iter().enumerate() {
            tiles[i] = *t;
            if i + 1 < sub.len() {
                gutters.push(Gutter {
                    kind: GutterKind::Vertical,
                    rect: Rect::new(t.column.x + t.column.w, 0, p.gutter, p.height),
                    row: 0,
                    left: i,
                });
            }
        }
        return Layout {
            tiles,
            gutters,
            rows,
        };
    }

    let available = (p.height - p.gutter).max(2);
    let pinned_rows = pins
        .rows
        .filter(|r| r.iter().all(|f| f.is_finite() && *f > 0.0));
    let heights = match pinned_rows {
        Some(fr) => proportional(available, &fr),
        None => {
            let natural: Vec<i32> = rows
                .iter()
                .map(|r| {
                    let sum: f64 = r.iter().map(|&i| f64::from(aspects[i])).sum();
                    (f64::from(p.width - p.gutter) / sum.max(0.01)).round() as i32
                })
                .collect();
            fair_share(available, &natural)
        }
    };
    let mut y = 0;
    for (ri, (row, &row_h)) in rows.iter().zip(&heights).enumerate() {
        let row_aspects: Vec<f32> = row.iter().map(|&i| aspects[i]).collect();
        let sub = solve_row(
            0,
            y,
            p.width,
            row_h,
            p.gutter,
            p.min_column,
            &row_aspects,
            &row_pins(row),
        );
        for (k, (&idx, t)) in row.iter().zip(&sub).enumerate() {
            tiles[idx] = *t;
            if k + 1 < sub.len() {
                gutters.push(Gutter {
                    kind: GutterKind::Vertical,
                    rect: Rect::new(t.column.x + t.column.w, y, p.gutter, row_h),
                    row: ri,
                    left: k,
                });
            }
        }
        y += row_h + p.gutter;
    }
    gutters.push(Gutter {
        kind: GutterKind::Horizontal,
        rect: Rect::new(0, heights[0], p.width, p.gutter),
        row: 0,
        left: 0,
    });
    Layout {
        tiles,
        gutters,
        rows,
    }
}

impl Layout {
    /// Index of the tile whose medium contains the point (window coordinates).
    pub fn tile_at(&self, x: i32, y: i32) -> Option<usize> {
        self.tiles.iter().position(|t| t.content.contains(x, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const P: LayoutParams = LayoutParams {
        width: 1920,
        height: 1080,
        gutter: 16,
        min_column: 160,
        min_wide_aspect: 1.2,
    };
    const W169: f32 = 16.0 / 9.0;
    const P916: f32 = 9.0 / 16.0;

    fn contents(aspects: &[f32]) -> Vec<(i32, i32)> {
        solve(&P, aspects, &Pins::default())
            .tiles
            .iter()
            .map(|t| (t.content.w, t.content.h))
            .collect()
    }

    #[test]
    fn portrait_and_widescreen_share_the_row_like_in_the_concept() {
        assert_eq!(contents(&[P916, W169]), vec![(608, 1080), (1296, 729)]);
        assert_eq!(
            contents(&[3.0 / 4.0, 3.0 / 4.0]),
            vec![(810, 1080), (810, 1080)]
        );
        assert_eq!(
            contents(&[P916, W169, 3.0 / 4.0]),
            vec![(608, 1080), (640, 360), (640, 853)]
        );
        assert_eq!(contents(&[P916; 4]), vec![(468, 832); 4]);
    }

    #[test]
    fn exactly_four_widescreen_media_form_a_2x2_grid() {
        let l = solve(&P, &[W169; 4], &Pins::default());
        assert_eq!(l.rows, vec![vec![0, 1], vec![2, 3]]);
        assert!(
            l.tiles
                .iter()
                .all(|t| (t.content.w, t.content.h) == (946, 532))
        );
        assert_eq!(
            l.gutters
                .iter()
                .filter(|g| g.kind == GutterKind::Horizontal)
                .count(),
            1
        );
        // three widescreen plus one portrait stays in one row
        assert_eq!(
            solve(&P, &[W169, W169, W169, P916], &Pins::default())
                .rows
                .len(),
            1
        );
        // a square medium is not widescreen
        assert_eq!(
            solve(&P, &[W169, W169, W169, 1.0], &Pins::default()).rows.len(),
            1
        );
    }

    #[test]
    fn media_with_spare_room_sit_next_to_each_other() {
        let l = solve(&P, &[3.0 / 4.0, 3.0 / 4.0], &Pins::default());
        let (a, b) = (l.tiles[0].content, l.tiles[1].content);
        assert_eq!(
            b.x - (a.x + a.w),
            P.gutter,
            "the gap between the two media equals the gutter"
        );
        assert!(
            (a.x - (P.width - (b.x + b.w))).abs() <= 1,
            "the pair is centered in the window"
        );
    }

    #[test]
    fn columns_fill_the_row_without_gaps() {
        for aspects in [
            vec![P916, W169],
            vec![P916, W169, 1.0],
            vec![W169; 3],
            vec![1.0, P916, W169, 0.8],
        ] {
            let l = solve(&P, &aspects, &Pins::default());
            let usable: i32 = l.tiles.iter().map(|t| t.column.w).sum();
            assert_eq!(usable, P.width - (aspects.len() as i32 - 1) * P.gutter);
            for t in &l.tiles {
                assert!(
                    t.content.x >= t.column.x
                        && t.content.x + t.content.w <= t.column.x + t.column.w
                );
                assert!(t.content.h <= t.column.h);
            }
        }
    }

    #[test]
    fn fair_share_is_exact_and_deterministic() {
        assert_eq!(
            fair_share(100, &[10, 20]),
            vec![45, 55],
            "everything fits: the spare room is split between the outer elements"
        );
        assert_eq!(fair_share(50, &[10, 100]), vec![10, 40]);
        assert_eq!(fair_share(50, &[100]), vec![50]);
        assert_eq!(fair_share(7, &[10, 10, 10]), vec![3, 2, 2]);
        assert_eq!(fair_share(0, &[5, 5]), vec![0, 0]);
        assert!(fair_share(10, &[]).is_empty());
    }

    #[test]
    fn degenerate_inputs_never_panic() {
        let tiny = LayoutParams {
            width: 1,
            height: 1,
            gutter: 16,
            min_column: 160,
            min_wide_aspect: 1.2,
        };
        for aspects in [vec![0.0], vec![f32::NAN, -1.0], vec![W169; 4], vec![1.0; 3]] {
            let l = solve(&tiny, &aspects, &Pins::default());
            assert_eq!(l.tiles.len(), aspects.len());
        }
        assert!(solve(&P, &[], &Pins::default()).tiles.is_empty());
    }

    #[test]
    fn a_pinned_width_is_honored_and_the_rest_adapts() {
        let pins = Pins {
            cols: vec![Some(0.7), None],
            rows: None,
        };
        let l = solve(&P, &[W169, W169], &pins);
        let usable = P.width - P.gutter;
        assert_eq!(l.tiles[0].column.w, (0.7 * f64::from(usable)).round() as i32);
        assert_eq!(l.tiles[0].column.w + l.tiles[1].column.w, usable);
    }

    #[test]
    fn a_pin_survives_when_a_tile_joins_and_new_tiles_stay_automatic() {
        let pins = Pins {
            cols: vec![Some(0.5), None],
            rows: None,
        };
        let two = solve(&P, &[W169, W169], &pins);
        let three = solve(&P, &[W169, W169, W169], &pins);
        let share = |l: &Layout| {
            let usable: i32 = l.tiles.iter().map(|t| t.column.w).sum();
            f64::from(l.tiles[0].column.w) / f64::from(usable)
        };
        assert!(
            (share(&two) - share(&three)).abs() < 0.001,
            "the pinned tile keeps its share of the row"
        );
        assert_eq!(three.tiles.len(), 3);
        let usable = P.width - 2 * P.gutter;
        let total: i32 = three.tiles.iter().map(|t| t.column.w).sum();
        assert_eq!(total, usable);
    }

    #[test]
    fn pinned_row_heights_apply_to_a_2x2_grid() {
        let pins = Pins {
            cols: Vec::new(),
            rows: Some([0.7, 0.3]),
        };
        let l = solve(&P, &[W169; 4], &pins);
        let available = P.height - P.gutter;
        assert_eq!(l.tiles[0].column.h, (0.7 * f64::from(available)).round() as i32);
        assert_eq!(l.tiles[2].column.h, available - l.tiles[0].column.h);
        let garbage = Pins {
            cols: Vec::new(),
            rows: Some([f64::NAN, 0.3]),
        };
        assert_eq!(
            solve(&P, &[W169; 4], &garbage),
            solve(&P, &[W169; 4], &Pins::default()),
            "unusable row pins count as automatic"
        );
    }

    #[test]
    fn a_pin_on_a_single_tile_changes_nothing() {
        let pins = Pins {
            cols: vec![Some(0.3)],
            rows: None,
        };
        assert_eq!(solve(&P, &[W169], &pins), solve(&P, &[W169], &Pins::default()));
    }

    #[test]
    fn tile_hit_testing_matches_the_content_rectangles() {
        let l = solve(&P, &[P916, W169], &Pins::default());
        let c1 = l.tiles[1].content;
        assert_eq!(l.tile_at(c1.x, c1.y), Some(1));
        assert_eq!(l.tile_at(c1.x - 1, c1.y), None);
        assert_eq!(l.tile_at(c1.x + c1.w, c1.y), None);
        assert_eq!(l.tile_at(c1.x, c1.y - 1), None);
    }
}
