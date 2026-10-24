//! The keyboard mark of the tab grid (pure): the item an arrow key moves it to in the masonry layout.

use super::geometry::MasonryRect;

/// An arrow key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Left,
    Right,
    Up,
    Down,
}

/// What a key of the tab grid does (Settings > Hotkeys > Tab-Grid); the code is what the main window's `tab-grid-key` carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridKey {
    Move(Step),
    /// Opens the marked item (like a click on it).
    Open,
    /// Closes the tab of the marked item.
    Close,
    /// Shows the marked item in the fullscreen.
    Fullscreen,
}

impl GridKey {
    pub const fn code(self) -> i32 {
        match self {
            Self::Move(Step::Left) => 0,
            Self::Move(Step::Right) => 1,
            Self::Move(Step::Up) => 2,
            Self::Move(Step::Down) => 3,
            Self::Open => 4,
            Self::Close => 5,
            Self::Fullscreen => 6,
        }
    }

    pub const fn from_code(code: i32) -> Option<Self> {
        Some(match code {
            0 => Self::Move(Step::Left),
            1 => Self::Move(Step::Right),
            2 => Self::Move(Step::Up),
            3 => Self::Move(Step::Down),
            4 => Self::Open,
            5 => Self::Close,
            6 => Self::Fullscreen,
            _ => return None,
        })
    }
}

/// Items closer than this (logical pixels) count as touching.
const EPS: f32 = 0.5;

/// The item an arrow key moves the mark to from item `from` (indexes into `rects`), `None` at the edge of the grid. An item
/// that lies in the direction and shares rows (left/right) or columns (up/down) with `from` comes first, then the smallest gap,
/// then the one whose middle is closest: right goes to the neighbor in the next column, down to the next item in the column.
pub fn neighbor(rects: &[MasonryRect], from: usize, step: Step) -> Option<usize> {
    let a = rects.get(from)?;
    let (ax, ay) = middle(a);
    rects
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != from)
        .filter_map(|(i, r)| {
            let (rx, ry) = middle(r);
            let (ahead, gap, off, shares) = match step {
                Step::Right => (
                    r.x >= a.x + a.width - EPS,
                    r.x - (a.x + a.width),
                    (ry - ay).abs(),
                    spans_meet(a.y, a.height, r.y, r.height),
                ),
                Step::Left => (
                    r.x + r.width <= a.x + EPS,
                    a.x - (r.x + r.width),
                    (ry - ay).abs(),
                    spans_meet(a.y, a.height, r.y, r.height),
                ),
                Step::Down => (
                    r.y >= a.y + a.height - EPS,
                    r.y - (a.y + a.height),
                    (rx - ax).abs(),
                    spans_meet(a.x, a.width, r.x, r.width),
                ),
                Step::Up => (
                    r.y + r.height <= a.y + EPS,
                    a.y - (r.y + r.height),
                    (rx - ax).abs(),
                    spans_meet(a.x, a.width, r.x, r.width),
                ),
            };
            ahead.then_some((i, !shares, gap.max(0.0), off))
        })
        .min_by(|p, q| p.1.cmp(&q.1).then(p.2.total_cmp(&q.2)).then(p.3.total_cmp(&q.3)))
        .map(|(i, ..)| i)
}

fn middle(r: &MasonryRect) -> (f32, f32) {
    (r.x + r.width / 2.0, r.y + r.height / 2.0)
}

/// Whether the span from `a` (length `a_len`) and the span from `b` (length `b_len`) overlap.
fn spans_meet(a: f32, a_len: f32, b: f32, b_len: f32) -> bool {
    a < b + b_len - EPS && b < a + a_len - EPS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> MasonryRect {
        MasonryRect { x, y, width, height }
    }

    /// Three columns of a masonry grid: 0 and 3 in the first, 1 and 4 in the second, 2 in the third.
    fn grid() -> Vec<MasonryRect> {
        vec![
            rect(0.0, 0.0, 100.0, 60.0),
            rect(112.0, 0.0, 100.0, 120.0),
            rect(224.0, 0.0, 100.0, 80.0),
            rect(0.0, 72.0, 100.0, 90.0),
            rect(112.0, 132.0, 100.0, 50.0),
        ]
    }

    #[test]
    fn key_codes_round_trip() {
        for code in 0..7 {
            assert_eq!(GridKey::from_code(code).map(GridKey::code), Some(code));
        }
        assert_eq!(GridKey::from_code(7), None);
    }

    #[test]
    fn left_and_right_go_to_the_neighbor_in_the_next_column() {
        let g = grid();
        assert_eq!(neighbor(&g, 0, Step::Right), Some(1));
        assert_eq!(neighbor(&g, 1, Step::Right), Some(2));
        assert_eq!(neighbor(&g, 2, Step::Right), None);
        assert_eq!(
            neighbor(&g, 3, Step::Right),
            Some(4),
            "both neighbors touch it; item 4 is closer to its middle"
        );
        assert_eq!(neighbor(&g, 4, Step::Left), Some(3));
        assert_eq!(neighbor(&g, 0, Step::Left), None);
    }

    #[test]
    fn up_and_down_stay_in_the_column() {
        let g = grid();
        assert_eq!(neighbor(&g, 0, Step::Down), Some(3));
        assert_eq!(neighbor(&g, 3, Step::Up), Some(0));
        assert_eq!(neighbor(&g, 1, Step::Down), Some(4));
        assert_eq!(neighbor(&g, 3, Step::Down), None);
        assert_eq!(
            neighbor(&g, 2, Step::Down),
            Some(4),
            "nothing below in its column: the closest item further down"
        );
        assert_eq!(neighbor(&g, 7, Step::Down), None);
    }
}
