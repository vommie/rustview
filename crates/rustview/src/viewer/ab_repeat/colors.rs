//! The colors of saved ranges (playbar and load menu): every range keeps its color while `RustView` runs, whatever is deleted
//! or added around it, and in every player that shows the file. A color is an index into the palette of the control bar
//! (`ui/multiview_video_bar.slint`); yellow is reserved for the active A-B and is not in it.

use super::protocol::SavedRange;
use parking_lot::Mutex;
use std::collections::HashMap;

/// How many colors the palette of the window has.
pub const PALETTE_SIZE: u8 = 8;

/// A range as the colors know it: its points in milliseconds and which one of equal ranges it is (renaming keeps the color).
type RangeKey = (i64, i64, u32);

#[derive(Debug, Default)]
struct FileColors {
    known: HashMap<RangeKey, u8>,
    /// The next color to try when all are in use.
    next: u8,
}

#[derive(Debug, Default)]
pub struct ColorBook {
    files: HashMap<String, FileColors>,
}

impl ColorBook {
    /// The palette index of every range of `list` (the saved ranges of `path`).
    pub fn colors(&mut self, path: &str, list: &[SavedRange]) -> Vec<u8> {
        let file = self.files.entry(path.to_owned()).or_default();
        let keys = keys_of(list);
        let mut used: Vec<u8> = keys.iter().filter_map(|key| file.known.get(key).copied()).collect();
        keys.iter()
            .map(|key| {
                if let Some(color) = file.known.get(key) {
                    return *color;
                }
                let color = (0..PALETTE_SIZE)
                    .map(|step| (file.next + step) % PALETTE_SIZE)
                    .find(|candidate| !used.contains(candidate))
                    .unwrap_or(file.next % PALETTE_SIZE);
                file.next = (color + 1) % PALETTE_SIZE;
                file.known.insert(*key, color);
                used.push(color);
                color
            })
            .collect()
    }
}

fn keys_of(list: &[SavedRange]) -> Vec<RangeKey> {
    let mut seen: HashMap<(i64, i64), u32> = HashMap::new();
    list.iter()
        .map(|range| {
            let point = (millis(range.a_point), millis(range.b_point));
            let nth = seen.entry(point).or_insert(0);
            let key = (point.0, point.1, *nth);
            *nth += 1;
            key
        })
        .collect()
}

fn millis(secs: f64) -> i64 {
    use rustview_core::util::cast::FloatToInt;
    (secs * 1000.0).round().to_int::<i64>()
}

static BOOK: Mutex<Option<ColorBook>> = Mutex::new(None);

/// The colors of the saved ranges of `path` (the book every player shares).
pub fn colors(path: &str, list: &[SavedRange]) -> Vec<u8> {
    BOOK.lock().get_or_insert_with(ColorBook::default).colors(path, list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(name: &str, a: f64, b: f64) -> SavedRange {
        SavedRange {
            name: name.into(),
            a_point: a,
            b_point: b,
        }
    }

    #[test]
    fn colors_stay_when_ranges_come_and_go() {
        let mut book = ColorBook::default();
        let first = vec![
            range("Intro", 0.0, 6.0),
            range("Middle", 40.0, 50.0),
            range("End", 80.0, 90.0),
        ];
        assert_eq!(book.colors("/v", &first), vec![0, 1, 2]);
        // "Middle" deleted, a new one added: the others keep theirs, the new one takes a free color.
        let second = vec![
            range("Intro", 0.0, 6.0),
            range("End", 80.0, 90.0),
            range("New", 10.0, 12.0),
        ];
        assert_eq!(book.colors("/v", &second), vec![0, 2, 3]);
        // Renamed: same points, same color.
        let third = vec![
            range("Start", 0.0, 6.0),
            range("End", 80.0, 90.0),
            range("New", 10.0, 12.0),
        ];
        assert_eq!(book.colors("/v", &third), vec![0, 2, 3]);
        // Another file has its own colors.
        assert_eq!(book.colors("/w", &first), vec![0, 1, 2]);
    }

    #[test]
    fn equal_ranges_get_different_colors_and_many_ranges_wrap() {
        let mut book = ColorBook::default();
        let twins = vec![range("A", 1.0, 2.0), range("B", 1.0, 2.0)];
        let colors = book.colors("/v", &twins);
        assert_ne!(colors[0], colors[1]);
        let many: Vec<SavedRange> = (0..20).map(|i| range("x", f64::from(i), f64::from(i) + 0.5)).collect();
        let colors = book.colors("/many", &many);
        assert!(colors.iter().all(|c| *c < PALETTE_SIZE));
        assert_eq!(&colors[..8], &[0, 1, 2, 3, 4, 5, 6, 7]);
    }
}
