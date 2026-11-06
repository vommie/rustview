//! Where the media tabs sit in the top bar (browser tabs like `RustCut`'s, questionnaire 2026-11-06): all tabs have the
//! same width, as wide as `TAB_MAX_WIDTH` while there is room, narrower down to `TAB_MIN_WIDTH`; when even that does not
//! fit, the tabs that fit are shown around the active one and the rest wait behind a "+n" button. Equal widths keep the
//! positions exact for dragging and need no text measuring (pure, the bar's width comes from Slint).

use rustview_core::util::cast::{Az, FloatToInt as _, SaturatingAs as _};

/// The widest and the narrowest media tab (logical pixels).
pub const TAB_MAX_WIDTH: f32 = 200.0;
pub const TAB_MIN_WIDTH: f32 = 110.0;
/// Between two tabs (`RustCut`'s dock tab bar).
pub const TAB_SPACING: f32 = 2.0;
/// The "+n" button after the last shown tab.
pub const OVERFLOW_WIDTH: f32 = 34.0;

/// The close buttons of `viewer.tabs.close_buttons` (an unknown value means a button on every tab).
pub fn close_buttons(mode: &str) -> crate::ui::TabCloseButtons {
    match mode {
        "active_and_hovered" => crate::ui::TabCloseButtons::ActiveAndHovered,
        "none" => crate::ui::TabCloseButtons::None,
        _ => crate::ui::TabCloseButtons::EveryTab,
    }
}

/// The tabs' places in a bar of a given width.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripLayout {
    /// The width of every tab.
    pub tab_width: f32,
    /// The first shown tab and how many are shown from it on.
    pub first: usize,
    pub shown: usize,
    /// Where the "+n" button starts (`None` = all tabs are shown).
    pub overflow_x: Option<f32>,
    count: usize,
}

impl StripLayout {
    /// Places `count` tabs in `width` pixels, keeping tab `active` among the shown ones.
    pub fn new(count: usize, active: Option<usize>, width: f32) -> Self {
        let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
        if count == 0 {
            return Self {
                tab_width: TAB_MAX_WIDTH,
                first: 0,
                shown: 0,
                overflow_x: None,
                count,
            };
        }
        let n = count.az::<f32>();
        let fitting = (width - TAB_SPACING * (n - 1.0)) / n;
        if fitting >= TAB_MIN_WIDTH {
            return Self {
                tab_width: fitting.min(TAB_MAX_WIDTH),
                first: 0,
                shown: count,
                overflow_x: None,
                count,
            };
        }
        // Not all fit: as many minimum-width tabs as fit next to the "+n" button (at least one).
        let room = width - OVERFLOW_WIDTH;
        let shown = ((room + TAB_SPACING) / (TAB_MIN_WIDTH + TAB_SPACING))
            .floor()
            .to_int::<usize>()
            .clamp(1, count);
        let active = active.unwrap_or(0).min(count - 1);
        let first = active.saturating_sub(shown - 1).min(count - shown);
        let tab_width = TAB_MIN_WIDTH;
        let overflow_x = shown.az::<f32>() * (tab_width + TAB_SPACING);
        Self {
            tab_width,
            first,
            shown,
            overflow_x: Some(overflow_x),
            count,
        }
    }

    /// Whether tab `index` is in the bar (the others are behind "+n").
    pub fn is_shown(&self, index: usize) -> bool {
        index >= self.first && index < self.first + self.shown
    }

    /// How many tabs wait behind "+n".
    pub fn hidden(&self) -> usize {
        self.count - self.shown
    }

    /// The left edge of tab `index` in the bar (a hidden tab continues the row outside the bar, so dragging has a
    /// place for every tab).
    pub fn x(&self, index: usize) -> f32 {
        let offset = index.saturating_as::<i64>() - self.first.saturating_as::<i64>();
        offset.az::<f32>() * (self.tab_width + TAB_SPACING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn few_tabs_get_the_full_width() {
        let l = StripLayout::new(3, Some(1), 2000.0);
        assert!((l.tab_width - TAB_MAX_WIDTH).abs() < f32::EPSILON);
        assert_eq!((l.first, l.shown, l.hidden()), (0, 3, 0));
        assert!(l.overflow_x.is_none());
        assert!((l.x(2) - 2.0 * (TAB_MAX_WIDTH + TAB_SPACING)).abs() < 1e-3);
    }

    #[test]
    fn more_tabs_share_the_width() {
        let l = StripLayout::new(5, None, 800.0);
        let expected = (800.0 - 4.0 * TAB_SPACING) / 5.0;
        assert!((l.tab_width - expected).abs() < 1e-3);
        assert!(l.is_shown(4));
        let last_end = l.x(4) + l.tab_width;
        assert!(last_end <= 800.0 + 1e-3, "the tabs fit exactly");
    }

    #[test]
    fn too_many_tabs_overflow_and_keep_the_active_one() {
        let l = StripLayout::new(20, Some(15), 600.0);
        assert!((l.tab_width - TAB_MIN_WIDTH).abs() < f32::EPSILON);
        assert!(l.shown >= 1 && l.shown < 20);
        assert!(l.is_shown(15), "the active tab is shown");
        assert_eq!(l.first + l.shown - 1, 15, "it is the last shown one");
        let button = l.overflow_x.expect("a +n button");
        assert!(button + OVERFLOW_WIDTH <= 600.0 + 1e-3, "the button fits");
        assert_eq!(l.hidden(), 20 - l.shown);
    }

    #[test]
    fn the_active_tab_near_the_start_keeps_the_start() {
        let l = StripLayout::new(20, Some(2), 600.0);
        assert_eq!(l.first, 0);
        assert!(l.is_shown(2));
    }

    #[test]
    fn a_tiny_bar_still_shows_one_tab() {
        let l = StripLayout::new(4, Some(3), 10.0);
        assert_eq!(l.shown, 1);
        assert!(l.is_shown(3));
        let none = StripLayout::new(0, None, f32::NAN);
        assert_eq!((none.shown, none.hidden()), (0, 0));
    }

    #[test]
    fn hidden_tabs_continue_the_row() {
        let l = StripLayout::new(10, Some(9), 400.0);
        assert!(l.x(0) < 0.0, "tabs before the shown ones lie left of the bar");
        assert!(l.x(l.first).abs() < f32::EPSILON);
    }
}
