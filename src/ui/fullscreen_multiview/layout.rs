//! Pure layout solver of the Multiview: integer physical pixels, no Slint/X11/mpv dependencies.
//!
//! One row with 1..=4 tiles is the normal case; exactly four widescreen media form a 2x2 grid. Every medium is fitted
//! (contain) into its column and aligned towards the window center, so media with spare room sit directly next to
//! each other instead of drifting apart. Column widths follow "fair share with fit cap" (water filling).

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

/// Proportions after the user dragged a gutter: `col_fr[row][column]` sums to 1 per row, `row_fr` sums to 1.
#[derive(Debug, Clone, PartialEq)]
pub struct ManualSplit {
    pub col_fr: Vec<Vec<f64>>,
    pub row_fr: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Split {
    #[default]
    Auto,
    Manual(ManualSplit),
}

impl ManualSplit {
    /// Exchanges two neighboring tiles of a row together with their column shares.
    pub fn swap_columns(&mut self, row: usize, left: usize) {
        if let Some(fr) = self.col_fr.get_mut(row)
            && left + 1 < fr.len()
        {
            fr.swap(left, left + 1);
        }
    }

    /// Exchanges the two rows of a 2x2 grid together with their heights and column shares.
    pub fn swap_rows(&mut self) {
        if self.col_fr.len() == 2 {
            self.col_fr.swap(0, 1);
            self.row_fr.swap(0, 1);
        }
    }
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

fn fit_cap(row_h: i32, aspect: f32) -> i32 {
    let a = if aspect.is_finite() && aspect > 0.0 {
        f64::from(aspect)
    } else {
        16.0 / 9.0
    };
    (f64::from(row_h.max(1)) * a).round() as i32
}

/// Splits `total` by proportions; the last element absorbs the rounding remainder.
fn proportional(total: i32, fractions: &[f64]) -> Vec<i32> {
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

/// One row of tiles: columns by manual fractions or fair share, each tile fitted and aligned towards the center.
fn solve_row(
    x0: i32,
    y0: i32,
    total_w: i32,
    row_h: i32,
    gutter: i32,
    aspects: &[f32],
    col_fr: Option<&[f64]>,
) -> Vec<TileLayout> {
    let n = aspects.len();
    let usable = (total_w - (n as i32 - 1) * gutter).max(n as i32);
    let columns = match col_fr {
        Some(fr) if fr.len() == n => proportional(usable, fr),
        _ => {
            let caps: Vec<i32> = aspects.iter().map(|&a| fit_cap(row_h, a)).collect();
            fair_share(usable, &caps)
        }
    };

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

fn manual_matches<'a>(split: &'a Split, rows: &[Vec<usize>]) -> Option<&'a ManualSplit> {
    match split {
        Split::Manual(m)
            if m.col_fr.len() == rows.len()
                && m.col_fr.iter().zip(rows).all(|(fr, r)| fr.len() == r.len())
                && m.row_fr.len() == rows.len() =>
        {
            Some(m)
        }
        _ => None,
    }
}

/// True if `split` is manual and still matches the row/column structure of `rows` (a changed partition invalidates it).
pub fn manual_fits(split: &Split, rows: &[Vec<usize>]) -> bool {
    manual_matches(split, rows).is_some()
}

/// Solves the whole layout. A manual split is used only while it still fits the current partition.
pub fn solve(p: &LayoutParams, aspects: &[f32], split: &Split) -> Layout {
    if aspects.is_empty() {
        return Layout::default();
    }
    let rows = partition(aspects, p.min_wide_aspect);
    let manual = manual_matches(split, &rows);
    let mut tiles = vec![TileLayout::default(); aspects.len()];
    let mut gutters = Vec::new();

    if rows.len() == 1 {
        let sub = solve_row(
            0,
            0,
            p.width,
            p.height,
            p.gutter,
            aspects,
            manual.map(|m| m.col_fr[0].as_slice()),
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
    let heights = match manual {
        Some(m) => proportional(available, &m.row_fr),
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
        let fr = manual.map(|m| m.col_fr[ri].as_slice());
        let sub = solve_row(0, y, p.width, row_h, p.gutter, &row_aspects, fr);
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
    /// The current arrangement as manual proportions; converting does not move any rectangle (up to 1 px of rounding).
    pub fn to_manual(&self) -> ManualSplit {
        let col_fr = self
            .rows
            .iter()
            .map(|r| {
                let usable: i32 = r.iter().map(|&i| self.tiles[i].column.w).sum();
                r.iter()
                    .map(|&i| f64::from(self.tiles[i].column.w) / f64::from(usable.max(1)))
                    .collect()
            })
            .collect();
        let row_fr = if self.rows.len() > 1 {
            let heights: Vec<i32> = self
                .rows
                .iter()
                .map(|r| self.tiles[r[0]].column.h)
                .collect();
            let sum: i32 = heights.iter().sum();
            heights
                .iter()
                .map(|&h| f64::from(h) / f64::from(sum.max(1)))
                .collect()
        } else {
            vec![1.0]
        };
        ManualSplit { col_fr, row_fr }
    }

    /// Index of the tile whose medium contains the point (window coordinates).
    pub fn tile_at(&self, x: i32, y: i32) -> Option<usize> {
        self.tiles.iter().position(|t| t.content.contains(x, y))
    }
}

/// Moves a gutter by `delta` pixels relative to the proportions at drag start (`start`, solved as `start_layout`).
/// Both neighbors keep at least `min_column` pixels; the result snaps to "neighbor exactly fitted" within `snap` pixels.
pub fn drag(
    start: &ManualSplit,
    start_layout: &Layout,
    gutter: &Gutter,
    delta: i32,
    p: &LayoutParams,
    aspects: &[f32],
    snap: i32,
) -> ManualSplit {
    let mut next = start.clone();
    match gutter.kind {
        GutterKind::Vertical => {
            let Some(row) = start_layout.rows.get(gutter.row) else {
                return next;
            };
            let (Some(&left_tile), Some(&right_tile)) =
                (row.get(gutter.left), row.get(gutter.left + 1))
            else {
                return next;
            };
            let usable: i32 = row.iter().map(|&i| start_layout.tiles[i].column.w).sum();
            let left_w = start_layout.tiles[left_tile].column.w;
            let pair = left_w + start_layout.tiles[right_tile].column.w;
            let min_col = p.min_column.min(pair / 3).max(1);
            let row_h = start_layout.tiles[left_tile].column.h;
            let mut new_left = (left_w + delta).clamp(min_col, (pair - min_col).max(min_col));
            let left_fit = fit_cap(row_h, aspects[left_tile]);
            let right_fit = fit_cap(row_h, aspects[right_tile]);
            if (new_left - left_fit).abs() <= snap {
                new_left = left_fit;
            } else if ((pair - new_left) - right_fit).abs() <= snap {
                new_left = pair - right_fit;
            }
            let new_left = new_left.clamp(min_col, (pair - min_col).max(min_col));
            let fr = &mut next.col_fr[gutter.row];
            fr[gutter.left] = f64::from(new_left) / f64::from(usable.max(1));
            fr[gutter.left + 1] = f64::from(pair - new_left) / f64::from(usable.max(1));
        }
        GutterKind::Horizontal => {
            if start.row_fr.len() != 2 {
                return next;
            }
            let available = (p.height - p.gutter).max(2);
            let top = (start.row_fr[0] * f64::from(available)).round() as i32;
            let min_row = p.min_column.min(available / 3).max(1);
            let new_top = (top + delta).clamp(min_row, (available - min_row).max(min_row));
            next.row_fr = vec![
                f64::from(new_top) / f64::from(available),
                f64::from(available - new_top) / f64::from(available),
            ];
        }
    }
    next
}

/// Rectangle of the mute badge inside a tile (top right corner), or `None` if the tile is too small for it.
pub fn badge_rect(
    content: Rect,
    scale: f32,
    size: f32,
    inset: f32,
    min_w: f32,
    min_h: f32,
) -> Option<Rect> {
    if (content.w as f32) < min_w * scale || (content.h as f32) < min_h * scale {
        return None;
    }
    let side = (size * scale).round() as i32;
    let margin = (inset * scale).round() as i32;
    Some(Rect::new(
        content.x + content.w - margin - side,
        content.y + margin,
        side,
        side,
    ))
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
        solve(&P, aspects, &Split::Auto)
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
        let l = solve(&P, &[W169; 4], &Split::Auto);
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
            solve(&P, &[W169, W169, W169, P916], &Split::Auto)
                .rows
                .len(),
            1
        );
        // a square medium is not widescreen
        assert_eq!(
            solve(&P, &[W169, W169, W169, 1.0], &Split::Auto).rows.len(),
            1
        );
    }

    #[test]
    fn media_with_spare_room_sit_next_to_each_other() {
        let l = solve(&P, &[3.0 / 4.0, 3.0 / 4.0], &Split::Auto);
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
            let l = solve(&P, &aspects, &Split::Auto);
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
            let l = solve(&tiny, &aspects, &Split::Auto);
            assert_eq!(l.tiles.len(), aspects.len());
        }
        assert!(solve(&P, &[], &Split::Auto).tiles.is_empty());
    }

    #[test]
    fn converting_auto_to_manual_moves_nothing() {
        for aspects in [vec![P916, W169], vec![P916, W169, 1.0], vec![W169; 4]] {
            let auto = solve(&P, &aspects, &Split::Auto);
            let manual = solve(&P, &aspects, &Split::Manual(auto.to_manual()));
            for (a, m) in auto.tiles.iter().zip(&manual.tiles) {
                assert!(
                    (a.column.x - m.column.x).abs() <= 2 && (a.column.w - m.column.w).abs() <= 2,
                    "{a:?} vs {m:?}"
                );
            }
        }
    }

    #[test]
    fn dragging_keeps_the_minimum_and_the_sum() {
        let aspects = [P916, W169];
        let start_layout = solve(&P, &aspects, &Split::Auto);
        let start = start_layout.to_manual();
        let gutter = start_layout.gutters[0];
        for delta in [-5000, -300, 0, 120, 5000] {
            let next = drag(&start, &start_layout, &gutter, delta, &P, &aspects, 8);
            let l = solve(&P, &aspects, &Split::Manual(next));
            assert!(
                l.tiles[0].column.w
                    >= P.min_column
                        .min((l.tiles[0].column.w + l.tiles[1].column.w) / 3)
            );
            assert!(l.tiles[1].column.w >= 1);
            assert_eq!(
                l.tiles[0].column.w + l.tiles[1].column.w,
                P.width - P.gutter
            );
        }
    }

    #[test]
    fn dragging_snaps_to_an_exactly_fitted_neighbor() {
        let aspects = [P916, W169];
        let start_layout = solve(&P, &aspects, &Split::Auto);
        let start = start_layout.to_manual();
        let gutter = start_layout.gutters[0];
        let fitted = fit_cap(1080, P916);
        let next = drag(
            &start,
            &start_layout,
            &gutter,
            fitted - start_layout.tiles[0].column.w - 5,
            &P,
            &aspects,
            8,
        );
        let l = solve(&P, &aspects, &Split::Manual(next));
        assert_eq!(l.tiles[0].column.w, fitted);
    }

    #[test]
    fn swapping_exchanges_shares_and_rows() {
        let mut m = ManualSplit {
            col_fr: vec![vec![0.3, 0.7], vec![0.4, 0.6]],
            row_fr: vec![0.6, 0.4],
        };
        m.swap_columns(0, 0);
        assert_eq!(m.col_fr[0], vec![0.7, 0.3]);
        m.swap_rows();
        assert_eq!(
            (m.row_fr.clone(), m.col_fr[0].clone()),
            (vec![0.4, 0.6], vec![0.4, 0.6])
        );
        m.swap_columns(5, 0);
    }

    #[test]
    fn a_stale_manual_split_is_ignored_when_the_partition_changes() {
        let two = solve(&P, &[P916, W169], &Split::Auto).to_manual();
        let l = solve(&P, &[P916, W169, W169], &Split::Manual(two));
        assert_eq!(l.tiles.len(), 3);
        assert_eq!(l, solve(&P, &[P916, W169, W169], &Split::Auto));
    }

    #[test]
    fn tile_hit_testing_matches_the_content_rectangles() {
        let l = solve(&P, &[P916, W169], &Split::Auto);
        let c1 = l.tiles[1].content;
        assert_eq!(l.tile_at(c1.x, c1.y), Some(1));
        assert_eq!(l.tile_at(c1.x - 1, c1.y), None);
        assert_eq!(l.tile_at(c1.x + c1.w, c1.y), None);
        assert_eq!(l.tile_at(c1.x, c1.y - 1), None);
    }

    #[test]
    fn badge_sits_in_the_top_right_corner_and_hides_in_small_tiles() {
        let c = Rect::new(100, 50, 800, 450);
        let b = badge_rect(c, 1.0, 28.0, 8.0, 120.0, 80.0).unwrap();
        assert_eq!(b, Rect::new(100 + 800 - 8 - 28, 50 + 8, 28, 28));
        assert!(badge_rect(Rect::new(0, 0, 100, 200), 1.0, 28.0, 8.0, 120.0, 80.0).is_none());
        let scaled = badge_rect(c, 1.5, 28.0, 8.0, 120.0, 80.0).unwrap();
        assert_eq!((scaled.w, scaled.x + scaled.w), (42, 100 + 800 - 12));
    }
}
