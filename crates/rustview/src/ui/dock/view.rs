//! The rows the Slint dock shows, built from the layout and the tab being dragged.

use slint::ModelRc;

use super::model::{DockLayout, DropTarget, GroupRef, PanelId, Side};
use super::plan::{self, metrics::TAB_SPACING};
use crate::ui::{DockGroupView, DockPanelPlace, DockRailItem, DockTabView};

/// The tab being dragged, as far as the rows need it.
#[derive(Debug, Clone, Copy)]
pub struct DraggedTab {
    pub panel: PanelId,
    pub width: f32,
    pub target: Option<DropTarget>,
}

/// One group row and the rows of its tab bar (the bridge puts the tabs into the group's own model).
pub struct GroupRows {
    pub group: DockGroupView,
    pub tabs: Vec<DockTabView>,
}

pub fn group_rows(layout: &DockLayout, side: Side, drag: Option<&DraggedTab>) -> Vec<GroupRows> {
    let sidebar = layout.sidebar(side);
    let plan = plan::plan_sidebar(sidebar);
    let placements = &plan.groups;
    sidebar
        .groups
        .iter()
        .zip(placements)
        .enumerate()
        .map(|(index, (group, placement))| {
            let at = GroupRef { side, index };
            let layout = plan::tab_slots(&group.tabs, Some(group.active), plan::tabs_area_width(sidebar, index));
            let slots = &layout.slots;
            let shifts = shifts(&group.tabs, slots, at, drag);
            let tabs = group
                .tabs
                .iter()
                .enumerate()
                .zip(slots)
                .zip(shifts)
                .map(|(((i, panel), slot), shift)| {
                    let active = *panel == group.active;
                    // A thin line between two tabs that are not selected (P9.15).
                    let next = group.tabs.get(i + 1).zip(slots.get(i + 1));
                    let separator = !active && next.is_some_and(|(p, s)| s.shown && *p != group.active);
                    DockTabView {
                        key: panel.key().into(),
                        title: panel.title().into(),
                        icon: panel.icon().to_slint(),
                        active,
                        x: slot.x,
                        width: slot.width,
                        shift,
                        placeholder: drag.is_some_and(|d| d.panel == *panel),
                        titled: slot.titled,
                        shown: slot.shown,
                        separator,
                    }
                })
                .collect();
            let next_shows_body = placements.get(index + 1).is_some_and(|p| p.shows_body);
            GroupRows {
                group: DockGroupView {
                    tabs: ModelRc::default(),
                    active_key: group.active.key().into(),
                    shows_body: placement.shows_body,
                    collapsed: group.collapsed,
                    maximized: sidebar.maximized == Some(index),
                    can_maximize: sidebar.groups.len() > 1,
                    first: index == 0,
                    fixed_before: placement.fixed_before,
                    frac_before: placement.frac_before,
                    fixed: placement.fixed,
                    frac: placement.frac,
                    resizable_below: placement.shows_body && next_shows_body,
                    overflow: i32::try_from(layout.hidden()).unwrap_or(0),
                    overflow_x: layout.overflow_x.unwrap_or_default(),
                },
                tabs,
            }
        })
        .collect()
}

/// How far each tab of the group at `at` moves aside while a tab is dragged over its bar.
fn shifts(tabs: &[PanelId], slots: &[plan::TabSlot], at: GroupRef, drag: Option<&DraggedTab>) -> Vec<f32> {
    let mut shifts = vec![0.0; tabs.len()];
    let Some(drag) = drag else {
        return shifts;
    };
    let Some(DropTarget::Tabs { group, index }) = drag.target else {
        return shifts;
    };
    if group != at {
        return shifts;
    }
    match tabs.iter().position(|p| *p == drag.panel) {
        // A reorder inside the bar: the tabs between the old and the new place close the gap.
        Some(source) => {
            let gap = slots.get(source).map_or(drag.width, |s| s.width) + TAB_SPACING;
            let target = if index > source { index - 1 } else { index };
            for (i, shift) in shifts.iter_mut().enumerate() {
                if source < i && i <= target {
                    *shift = -gap;
                } else if target <= i && i < source {
                    *shift = gap;
                }
            }
        }
        // A tab from elsewhere: the tabs from the insertion point on make room for it.
        None => {
            for shift in shifts.iter_mut().skip(index) {
                *shift = drag.width + TAB_SPACING;
            }
        }
    }
    shifts
}

/// The icons of a collapsed sidebar, group by group.
pub fn rail_rows(layout: &DockLayout, side: Side) -> Vec<DockRailItem> {
    let mut rows = Vec::new();
    for (index, group) in layout.sidebar(side).groups.iter().enumerate() {
        for (tab, panel) in group.tabs.iter().enumerate() {
            rows.push(DockRailItem {
                key: panel.key().into(),
                title: panel.title().into(),
                icon: panel.icon().to_slint(),
                active: *panel == group.active,
                group_start: index > 0 && tab == 0,
                group: i32::try_from(index).unwrap_or(i32::MAX),
            });
        }
    }
    rows
}

/// Where every panel is (in the order of [`PanelId::ALL`]): the main window places each panel's body over its group.
pub fn panel_places(layout: &DockLayout) -> Vec<DockPanelPlace> {
    PanelId::ALL
        .iter()
        .map(|panel| match layout.locate(*panel) {
            Some((at, _)) => DockPanelPlace {
                side: match at.side {
                    Side::Left => 0,
                    Side::Right => 1,
                },
                group: i32::try_from(at.index).unwrap_or(i32::MAX),
                active: layout.group(at).is_some_and(|g| g.active == *panel),
            },
            None => DockPanelPlace {
                side: 0,
                group: -1,
                active: false,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drag(panel: PanelId, group: GroupRef, index: usize) -> DraggedTab {
        DraggedTab {
            panel,
            width: 100.0,
            target: Some(DropTarget::Tabs { group, index }),
        }
    }

    const LEFT: GroupRef = GroupRef {
        side: Side::Left,
        index: 0,
    };

    #[test]
    fn a_foreign_tab_pushes_the_tabs_after_the_insertion_point() {
        let layout = DockLayout::default();
        let rows = group_rows(&layout, Side::Left, Some(&drag(PanelId::Info, LEFT, 1)));
        let shifts: Vec<f32> = rows[0].tabs.iter().map(|t| t.shift).collect();
        let room = 100.0 + TAB_SPACING;
        assert_eq!(shifts, [0.0, room]);
        assert!(rows[0].tabs.iter().all(|t| !t.placeholder));
    }

    #[test]
    fn a_reorder_closes_the_gap_behind_the_dragged_tab() {
        let layout = DockLayout::default();
        // Folders (first) dragged behind Favorites (insertion index 2).
        let rows = group_rows(&layout, Side::Left, Some(&drag(PanelId::Folders, LEFT, 2)));
        let tabs = &rows[0].tabs;
        assert!(tabs[0].placeholder);
        assert!(tabs[0].shift.abs() < f32::EPSILON);
        assert!(tabs[1].shift < 0.0);
    }

    #[test]
    fn rows_follow_the_plan() {
        let mut layout = DockLayout::default();
        layout.toggle_collapsed(GroupRef {
            side: Side::Left,
            index: 1,
        });
        let rows = group_rows(&layout, Side::Left, None);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].group.first && rows[0].group.can_maximize);
        assert!(!rows[1].group.shows_body);
        assert!(!rows[0].group.resizable_below);
        assert_eq!(rows[0].group.active_key, "folders");
    }

    #[test]
    fn the_rail_marks_group_starts() {
        let layout = DockLayout::default();
        let rail = rail_rows(&layout, Side::Left);
        let starts: Vec<bool> = rail.iter().map(|r| r.group_start).collect();
        assert_eq!(starts, [false, false, true]);
        assert!(rail[0].active && !rail[1].active && rail[2].active);
    }

    #[test]
    fn every_panel_has_a_place_in_the_order_of_all() {
        let mut layout = DockLayout::default();
        layout.click_tab(PanelId::Favorites);
        let places = panel_places(&layout);
        assert_eq!(places.len(), PanelId::ALL.len());
        assert_eq!((places[0].side, places[0].group, places[0].active), (0, 0, false));
        assert_eq!((places[1].side, places[1].group, places[1].active), (0, 0, true));
        assert_eq!((places[2].side, places[2].group), (0, 1));
        assert_eq!((places[4].side, places[4].group, places[4].active), (1, 1, true));
    }
}
