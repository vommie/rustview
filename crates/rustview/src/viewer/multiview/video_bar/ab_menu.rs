//! The load menu of the A-B repeat group (pure): which saved repeats it lists for a search, on which page, and which page a
//! video's menu opens at (remembered per file while `RustView` runs).

use crate::viewer::ab_repeat::prefs::MenuPaging;
use crate::viewer::ab_repeat::protocol::SavedRange;
use parking_lot::Mutex;
use std::collections::HashMap;

/// A row of the menu.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRow {
    /// The position in the saved list (0-based): what a command names.
    pub index: usize,
    /// The number the script's own menu shows (1-based, saved order).
    pub number: usize,
    pub name: String,
    pub from: f64,
    pub to: f64,
    /// The palette color (`None`: grey).
    pub color: Option<u8>,
    /// The repeat that runs now.
    pub current: bool,
}

/// What the menu shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuPage {
    pub rows: Vec<MenuRow>,
    /// 0-based.
    pub page: usize,
    pub pages: usize,
    /// How many entries match the search (all when it is empty).
    pub matches: usize,
    pub total: usize,
}

/// The rows of `list` that match `query` (case-insensitive part of the name), on page `page` (clamped) or all of them in a
/// scroll list. `colors` are the palette colors of the list's entries (empty: grey), `current` the points that loop now.
pub fn page(
    list: &[SavedRange],
    colors: &[u8],
    query: &str,
    current: Option<(f64, f64)>,
    paging: MenuPaging,
    page_size: usize,
    page: usize,
) -> MenuPage {
    let needle = query.trim().to_lowercase();
    let matching: Vec<MenuRow> = list
        .iter()
        .enumerate()
        .filter(|(_, range)| needle.is_empty() || range.name.to_lowercase().contains(&needle))
        .map(|(index, range)| MenuRow {
            index,
            number: index + 1,
            name: range.name.clone(),
            from: range.a_point,
            to: range.b_point,
            color: colors.get(index).copied(),
            current: current.is_some_and(|(a, b)| range.is_at(a, b)),
        })
        .collect();
    let matches = matching.len();
    if paging == MenuPaging::Scroll {
        return MenuPage {
            rows: matching,
            page: 0,
            pages: 1,
            matches,
            total: list.len(),
        };
    }
    let size = page_size.max(1);
    let pages = matches.div_ceil(size).max(1);
    let page = page.min(pages - 1);
    MenuPage {
        rows: matching.into_iter().skip(page * size).take(size).collect(),
        page,
        pages,
        matches,
        total: list.len(),
    }
}

/// The page of the file's menu that opened last (0 for a file whose menu never opened).
static PAGES: Mutex<Option<HashMap<String, usize>>> = Mutex::new(None);

pub fn remembered_page(path: &str) -> usize {
    PAGES
        .lock()
        .as_ref()
        .and_then(|pages| pages.get(path).copied())
        .unwrap_or(0)
}

pub fn remember_page(path: &str, page: usize) {
    PAGES
        .lock()
        .get_or_insert_with(HashMap::new)
        .insert(path.to_owned(), page);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: usize) -> Vec<SavedRange> {
        (0u32..)
            .take(n)
            .map(|i| SavedRange {
                name: format!("Take {:02}", i + 1),
                a_point: 10.0 * f64::from(i),
                b_point: 10.0 * f64::from(i) + 8.0,
            })
            .collect()
    }

    #[test]
    fn pages_keep_the_script_numbers() {
        let ranges = list(26);
        let first = page(&ranges, &[], "", None, MenuPaging::Pages, 10, 0);
        assert_eq!((first.pages, first.rows.len(), first.matches), (3, 10, 26));
        let last = page(&ranges, &[], "", None, MenuPaging::Pages, 10, 7);
        assert_eq!(last.page, 2, "clamped to the last page");
        assert_eq!(last.rows.len(), 6);
        assert_eq!(last.rows[0].number, 21);
        assert_eq!(last.rows[0].index, 20);
    }

    #[test]
    fn the_search_filters_names_and_keeps_numbers() {
        let ranges = list(26);
        let found = page(&ranges, &[], " take 1", None, MenuPaging::Pages, 10, 0);
        assert_eq!(found.matches, 10, "Take 10 to Take 19");
        assert_eq!(found.rows[0].number, 10);
        let none = page(&ranges, &[], "zzz", None, MenuPaging::Pages, 10, 3);
        assert_eq!((none.pages, none.page, none.rows.len()), (1, 0, 0));
    }

    #[test]
    fn a_scroll_list_has_every_row_and_the_running_one_is_marked() {
        let ranges = list(26);
        let colors: Vec<u8> = (0u8..26).map(|i| i % 8).collect();
        let all = page(&ranges, &colors, "", Some((30.0, 38.0)), MenuPaging::Scroll, 10, 2);
        assert_eq!(all.rows.len(), 26);
        assert!(all.rows[3].current && !all.rows[2].current);
        assert_eq!(all.rows[9].color, Some(1));
    }

    #[test]
    fn the_page_is_remembered_per_file() {
        assert_eq!(remembered_page("/never-opened.mp4"), 0);
        remember_page("/a.mp4", 2);
        assert_eq!(remembered_page("/a.mp4"), 2);
    }
}
