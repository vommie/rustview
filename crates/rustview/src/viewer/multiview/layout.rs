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
    /// Only the swap button of one column of a 2x2 grid (`left` = the column): it exchanges the tile above with the
    /// tile below and sits on the horizontal gutter, centered on the smaller of the two tiles.
    ColumnSwap,
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
        return Layout { tiles, gutters, rows };
    }

    let available = (p.height - p.gutter).max(2);
    let pinned_rows = pins.rows.filter(|r| r.iter().all(|f| f.is_finite() && *f > 0.0));
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
    if let [top, bottom] = rows.as_slice() {
        for (column, (&a, &b)) in top.iter().zip(bottom).enumerate() {
            let smaller = if tiles[a].content.w <= tiles[b].content.w { a } else { b };
            let content = tiles[smaller].content;
            gutters.push(Gutter {
                kind: GutterKind::ColumnSwap,
                rect: Rect::new(content.x + (content.w - p.gutter) / 2, heights[0], p.gutter, p.gutter),
                row: 0,
                left: column,
            });
        }
    }
    Layout { tiles, gutters, rows }
}

impl Layout {
    /// Index of the tile whose medium contains the point (window coordinates).
    pub fn tile_at(&self, x: i32, y: i32) -> Option<usize> {
        self.tiles.iter().position(|t| t.content.contains(x, y))
    }
}
