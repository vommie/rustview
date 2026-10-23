//! The multi-selection of tabs in the tab bar and the tab grid (pure, no Slint).
//!
//! Ctrl+click toggles a tab, Shift+click selects the range from the anchor to the clicked tab (with Ctrl it is added to the
//! selection), a plain click activates the tab and drops the selection. The anchor is the tab the last click went to. A
//! selection that starts with a Ctrl or Shift click in the tab view contains the tab on screen as its first member (`click_from_shown`);
//! in the Browse view and the tab grid no tab counts as shown. What is highlighted is exactly what the menu entries act on.

use crate::viewer::TabId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabSelection {
    selected: Vec<TabId>,
    anchor: Option<TabId>,
}

/// What the context menu of a tab acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuTarget {
    /// The clicked tab alone (a selection that did not include it is dropped).
    Tab,
    /// All selected tabs (the clicked tab is one of at least two).
    Selection,
}

impl TabSelection {
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn contains(&self, tab: TabId) -> bool {
        self.selected.contains(&tab)
    }

    /// The selected tabs in the order of `order` (the tab bar); tabs that are not in `order` are left out.
    pub fn in_order(&self, order: &[TabId]) -> Vec<TabId> {
        order.iter().copied().filter(|id| self.contains(*id)).collect()
    }

    /// A Ctrl/Shift click on `tab` while the tab `shown` is the one on screen: starting a selection, the shown tab is its first
    /// member (the user selects "a second tab" to go with the one they are looking at). With a selection already, or without a
    /// shown tab (Browse view, tab grid), it is `click` as it is.
    pub fn click_from_shown(
        &mut self,
        order: &[TabId],
        shown: Option<TabId>,
        tab: TabId,
        ctrl: bool,
        shift: bool,
    ) -> bool {
        if let Some(shown) = shown.filter(|id| (ctrl || shift) && self.selected.is_empty() && order.contains(id)) {
            self.selected.push(shown);
            self.anchor = Some(shown);
            if tab == shown {
                // Ctrl+click on the shown tab itself: it is the selection (toggling it off again would select nothing)
                return true;
            }
        }
        self.click(order, tab, ctrl, shift)
    }

    /// A click on `tab`. Returns whether the set of selected tabs changed.
    pub fn click(&mut self, order: &[TabId], tab: TabId, ctrl: bool, shift: bool) -> bool {
        match (ctrl, shift) {
            (_, true) => self.select_range(order, tab, ctrl),
            (true, false) => self.toggle(tab),
            (false, false) => self.click_plain(tab),
        }
    }

    /// Ctrl+click: the tab joins the selection or leaves it, and becomes the anchor.
    pub fn toggle(&mut self, tab: TabId) -> bool {
        self.anchor = Some(tab);
        if let Some(position) = self.selected.iter().position(|id| *id == tab) {
            self.selected.remove(position);
        } else {
            self.selected.push(tab);
        }
        true
    }

    /// Shift+click: the tabs from the anchor to `tab` (both included) are selected. Without a usable anchor it selects `tab`
    /// alone. With `additive` (Ctrl+Shift) the range is added to what is selected, otherwise it replaces it. The anchor stays,
    /// so the next Shift+click grows or shrinks the same range.
    pub fn select_range(&mut self, order: &[TabId], tab: TabId, additive: bool) -> bool {
        let Some(end) = order.iter().position(|id| *id == tab) else {
            return false;
        };
        let start = self
            .anchor
            .and_then(|anchor| order.iter().position(|id| *id == anchor))
            .unwrap_or(end);
        if self.anchor.is_none_or(|anchor| !order.contains(&anchor)) {
            self.anchor = Some(tab);
        }
        let (low, high) = if start <= end { (start, end) } else { (end, start) };
        let before = self.in_order(order);
        if !additive {
            self.selected.clear();
        }
        for id in &order[low..=high] {
            if !self.contains(*id) {
                self.selected.push(*id);
            }
        }
        self.in_order(order) != before
    }

    /// A plain click: the selection is dropped and the clicked tab is the anchor of the next Shift+click.
    pub fn click_plain(&mut self, tab: TabId) -> bool {
        self.anchor = Some(tab);
        self.drop_selected()
    }

    /// Esc or a click on nothing. Returns whether anything was selected.
    pub fn clear(&mut self) -> bool {
        self.anchor = None;
        self.drop_selected()
    }

    /// A right click on `tab`: with two or more tabs selected and `tab` among them the menu acts on the selection, otherwise on
    /// the tab and a selection that did not include it is dropped. The second value tells whether the selection changed.
    pub fn context_menu(&mut self, tab: TabId) -> (MenuTarget, bool) {
        if self.selected.len() >= 2 && self.contains(tab) {
            (MenuTarget::Selection, false)
        } else if self.contains(tab) {
            (MenuTarget::Tab, false)
        } else {
            (MenuTarget::Tab, self.drop_selected())
        }
    }

    /// Forgets tabs that do not exist any more (closed, merged). Returns whether the selection changed.
    pub fn retain_existing(&mut self, order: &[TabId]) -> bool {
        let before = self.selected.len();
        self.selected.retain(|id| order.contains(id));
        if self.anchor.is_some_and(|anchor| !order.contains(&anchor)) {
            self.anchor = None;
        }
        self.selected.len() != before
    }

    fn drop_selected(&mut self) -> bool {
        let had = !self.selected.is_empty();
        self.selected.clear();
        had
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORDER: [TabId; 6] = [10, 11, 12, 13, 14, 15];

    fn selected(selection: &TabSelection) -> Vec<TabId> {
        selection.in_order(&ORDER)
    }

    #[test]
    fn ctrl_click_toggles_one_tab_at_a_time() {
        let mut selection = TabSelection::default();
        assert!(selection.click(&ORDER, 12, true, false));
        assert!(selection.click(&ORDER, 14, true, false));
        assert_eq!(selected(&selection), vec![12, 14]);
        assert!(selection.click(&ORDER, 12, true, false));
        assert_eq!(selected(&selection), vec![14]);
        assert!(selection.click(&ORDER, 14, true, false));
        assert!(selection.is_empty());
    }

    #[test]
    fn the_selection_is_reported_in_tab_order_whatever_the_click_order() {
        let mut selection = TabSelection::default();
        for tab in [15, 11, 13] {
            selection.click(&ORDER, tab, true, false);
        }
        assert_eq!(selected(&selection), vec![11, 13, 15]);
        assert_eq!(selection.len(), 3);
    }

    #[test]
    fn a_plain_click_drops_the_selection_and_sets_the_anchor() {
        let mut selection = TabSelection::default();
        selection.click(&ORDER, 11, true, false);
        selection.click(&ORDER, 12, true, false);
        assert!(selection.click(&ORDER, 14, false, false));
        assert!(selection.is_empty());
        // the plain click is the anchor of the range that follows
        assert!(selection.click(&ORDER, 12, false, true));
        assert_eq!(selected(&selection), vec![12, 13, 14]);
        assert!(
            selection.click_plain(15),
            "the range was selected, a plain click drops it"
        );
        assert!(!selection.click_plain(15), "nothing was selected, nothing changes");
    }

    #[test]
    fn shift_click_selects_the_range_in_both_directions() {
        let mut selection = TabSelection::default();
        selection.click(&ORDER, 12, true, false);
        selection.click(&ORDER, 14, false, true);
        assert_eq!(selected(&selection), vec![12, 13, 14]);
        // the anchor stays: the next shift click moves the end of the same range, replacing the old one
        selection.click(&ORDER, 10, false, true);
        assert_eq!(selected(&selection), vec![10, 11, 12]);
    }

    #[test]
    fn ctrl_shift_click_adds_the_range_to_the_selection() {
        let mut selection = TabSelection::default();
        selection.click(&ORDER, 10, true, false);
        selection.click(&ORDER, 13, true, false);
        selection.click(&ORDER, 15, true, true);
        assert_eq!(selected(&selection), vec![10, 13, 14, 15]);
    }

    #[test]
    fn shift_click_without_an_anchor_selects_that_tab_alone() {
        let mut selection = TabSelection::default();
        assert!(selection.click(&ORDER, 13, false, true));
        assert_eq!(selected(&selection), vec![13]);
        assert!(
            !selection.click(&ORDER, 99, false, true),
            "an unknown tab changes nothing"
        );
    }

    #[test]
    fn escape_clears_and_reports_whether_anything_was_selected() {
        let mut selection = TabSelection::default();
        assert!(!selection.clear());
        selection.click(&ORDER, 11, true, false);
        assert!(selection.clear());
        assert!(selection.is_empty());
        assert!(!selection.click(&ORDER, 13, false, true) || selected(&selection) == vec![13]);
    }

    #[test]
    fn the_context_menu_acts_on_the_selection_only_for_a_selected_tab_of_several() {
        let mut selection = TabSelection::default();
        selection.click(&ORDER, 11, true, false);
        selection.click(&ORDER, 13, true, false);
        assert_eq!(selection.context_menu(13), (MenuTarget::Selection, false));
        assert_eq!(selection.len(), 2, "the selection stays");
        // a tab outside the selection: the menu is that tab's own and the selection is gone
        assert_eq!(selection.context_menu(15), (MenuTarget::Tab, true));
        assert!(selection.is_empty());
        assert_eq!(selection.context_menu(15), (MenuTarget::Tab, false));
    }

    #[test]
    fn the_shown_tab_is_the_first_member_of_a_selection_that_starts_with_a_ctrl_click() {
        let mut selection = TabSelection::default();
        assert!(selection.click_from_shown(&ORDER, Some(11), 14, true, false));
        assert_eq!(selected(&selection), vec![11, 14]);
        // the selection exists now: further clicks do not add the shown tab again, and it can be toggled off
        selection.click_from_shown(&ORDER, Some(11), 12, true, false);
        assert_eq!(selected(&selection), vec![11, 12, 14]);
        selection.click_from_shown(&ORDER, Some(11), 11, true, false);
        assert_eq!(selected(&selection), vec![12, 14]);
    }

    #[test]
    fn ctrl_clicking_the_shown_tab_alone_selects_just_that_tab() {
        let mut selection = TabSelection::default();
        selection.click_from_shown(&ORDER, Some(12), 12, true, false);
        assert_eq!(selected(&selection), vec![12]);
    }

    #[test]
    fn a_shift_click_selects_the_range_from_the_shown_tab() {
        let mut selection = TabSelection::default();
        selection.click_from_shown(&ORDER, Some(11), 14, false, true);
        assert_eq!(selected(&selection), vec![11, 12, 13, 14]);
    }

    #[test]
    fn without_a_shown_tab_nothing_is_added() {
        let mut selection = TabSelection::default();
        selection.click_from_shown(&ORDER, None, 13, true, false);
        assert_eq!(selected(&selection), vec![13]);
        let mut selection = TabSelection::default();
        selection.click_from_shown(&ORDER, Some(99), 13, true, false);
        assert_eq!(selected(&selection), vec![13], "a tab that does not exist is ignored");
    }

    #[test]
    fn a_single_selected_tab_keeps_the_menu_of_the_tab() {
        let mut selection = TabSelection::default();
        selection.click(&ORDER, 12, true, false);
        assert_eq!(selection.context_menu(12), (MenuTarget::Tab, false));
        assert_eq!(selection.len(), 1);
    }

    #[test]
    fn closed_tabs_leave_the_selection_and_the_anchor() {
        let mut selection = TabSelection::default();
        for tab in [10, 12, 14] {
            selection.click(&ORDER, tab, true, false);
        }
        let remaining = [10, 11, 13, 14, 15];
        assert!(selection.retain_existing(&remaining));
        assert_eq!(selection.in_order(&remaining), vec![10, 14]);
        assert!(!selection.retain_existing(&remaining));
        // the anchor (14) is still there: a range runs from it
        selection.click(&remaining, 15, false, true);
        assert_eq!(selection.in_order(&remaining), vec![14, 15]);
        selection.retain_existing(&[10, 11]);
        assert!(selection.is_empty());
        // the anchor went with its tab: a range starts at the clicked tab
        selection.click(&[10, 11], 11, false, true);
        assert_eq!(selection.in_order(&[10, 11]), vec![11]);
    }
}
