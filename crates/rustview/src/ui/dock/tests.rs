//! Unit and property tests of the dock model and its geometry.

use proptest::prelude::*;

use super::model::{DockLayout, DropTarget, GroupRef, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH, PanelId, Side};
use super::persist;
use super::plan::{self, DockFrame, Rect, Resize, metrics};

const LEFT_0: GroupRef = GroupRef {
    side: Side::Left,
    index: 0,
};
const LEFT_1: GroupRef = GroupRef {
    side: Side::Left,
    index: 1,
};
const RIGHT_0: GroupRef = GroupRef {
    side: Side::Right,
    index: 0,
};

fn tabs(layout: &DockLayout, at: GroupRef) -> Vec<PanelId> {
    layout.group(at).unwrap().tabs.clone()
}

const RIGHT_1: GroupRef = GroupRef {
    side: Side::Right,
    index: 1,
};

#[test]
fn the_default_layout_is_valid_and_matches_rustview_0_6_13() {
    let layout = DockLayout::default();
    assert!(layout.is_valid());
    assert_eq!(tabs(&layout, LEFT_0), [PanelId::Folders, PanelId::Favorites]);
    assert_eq!(tabs(&layout, LEFT_1), [PanelId::Info]);
    assert_eq!(tabs(&layout, RIGHT_0), [PanelId::TagRate]);
    assert_eq!(tabs(&layout, RIGHT_1), [PanelId::TagHistory]);
    assert_eq!(
        PanelId::Folders.key(),
        "folders",
        "the key of view.left_sidebar_panel_tabs stays"
    );
}

#[test]
fn panel_keys_round_trip() {
    for panel in PanelId::ALL {
        assert_eq!(PanelId::from_key(panel.key()), Some(panel));
    }
    assert_eq!(PanelId::from_key("nope"), None);
}

#[test]
fn a_tab_moves_into_another_group_and_becomes_active() {
    let mut layout = DockLayout::default();
    assert!(layout.move_panel(
        PanelId::TagHistory,
        DropTarget::Tabs {
            group: LEFT_1,
            index: 0
        }
    ));
    assert_eq!(tabs(&layout, LEFT_1), [PanelId::TagHistory, PanelId::Info]);
    assert_eq!(layout.group(LEFT_1).unwrap().active, PanelId::TagHistory);
    assert_eq!(layout.right.groups.len(), 1, "the emptied group disappears");
    assert!(layout.is_valid());
}

#[test]
fn moving_the_active_tab_activates_its_neighbor() {
    let mut layout = DockLayout::default();
    layout.click_tab(PanelId::Favorites);
    layout.move_panel(PanelId::Favorites, DropTarget::Sidebar(Side::Right));
    assert_eq!(layout.group(LEFT_0).unwrap().active, PanelId::Folders);
}

#[test]
fn reordering_inside_a_group_counts_the_removed_tab() {
    let mut layout = DockLayout::default();
    // Drop Folders after Favorites: insertion index 2 in the bar that still shows it.
    assert!(layout.move_panel(
        PanelId::Folders,
        DropTarget::Tabs {
            group: LEFT_0,
            index: 2
        }
    ));
    assert_eq!(tabs(&layout, LEFT_0), [PanelId::Favorites, PanelId::Folders]);
}

#[test]
fn an_emptied_group_disappears() {
    let mut layout = DockLayout::default();
    assert!(layout.move_panel(
        PanelId::Info,
        DropTarget::Tabs {
            group: RIGHT_0,
            index: 9
        }
    ));
    assert_eq!(layout.left.groups.len(), 1);
    assert_eq!(tabs(&layout, RIGHT_0).last(), Some(&PanelId::Info));
    assert!(layout.is_valid());
}

#[test]
fn a_split_halves_the_weight_of_the_target() {
    let mut layout = DockLayout::default();
    assert!(layout.move_panel(
        PanelId::TagHistory,
        DropTarget::Split {
            group: LEFT_0,
            below: false
        }
    ));
    assert_eq!(tabs(&layout, LEFT_0), [PanelId::TagHistory]);
    assert_eq!(tabs(&layout, LEFT_1), [PanelId::Folders, PanelId::Favorites]);
    assert!((layout.left.groups[0].weight - 2.0).abs() < f32::EPSILON);
    assert!((layout.left.groups[1].weight - 2.0).abs() < f32::EPSILON);
}

#[test]
fn splitting_a_single_tab_off_itself_changes_nothing() {
    let mut layout = DockLayout::default();
    let before = layout.clone();
    assert!(!layout.move_panel(
        PanelId::Info,
        DropTarget::Split {
            group: LEFT_1,
            below: true
        }
    ));
    assert!(!layout.move_panel(PanelId::Info, DropTarget::Sidebar(Side::Left)));
    assert_eq!(layout, before);
}

#[test]
fn a_sidebar_can_be_emptied_and_filled_again() {
    let mut layout = DockLayout::default();
    for panel in [PanelId::TagRate, PanelId::TagHistory] {
        layout.move_panel(
            panel,
            DropTarget::Tabs {
                group: LEFT_1,
                index: 0,
            },
        );
    }
    assert!(!layout.right.is_shown());
    assert!(layout.is_valid());
    assert!(layout.move_panel(PanelId::TagHistory, DropTarget::Sidebar(Side::Right)));
    assert_eq!(tabs(&layout, RIGHT_0), [PanelId::TagHistory]);
}

#[test]
fn clicking_the_active_tab_collapses_and_expands() {
    let mut layout = DockLayout::default();
    layout.click_tab(PanelId::Folders);
    assert!(layout.left.groups[0].collapsed);
    layout.click_tab(PanelId::Favorites);
    assert!(!layout.left.groups[0].collapsed);
    assert_eq!(layout.left.groups[0].active, PanelId::Favorites);
    layout.click_tab(PanelId::Favorites);
    assert!(layout.left.groups[0].collapsed);
}

#[test]
fn maximize_needs_two_groups_and_toggles() {
    let mut layout = DockLayout::default();
    layout.move_panel(
        PanelId::TagHistory,
        DropTarget::Tabs {
            group: RIGHT_0,
            index: 1,
        },
    );
    layout.toggle_maximized(RIGHT_0);
    assert_eq!(layout.right.maximized, None, "one group cannot be maximized");
    layout.left.groups[1].collapsed = true;
    layout.toggle_maximized(LEFT_1);
    assert_eq!(layout.left.maximized, Some(1));
    assert!(!layout.left.groups[1].collapsed);
    // A click on a tab of the other group restores the sidebar.
    layout.click_tab(PanelId::Favorites);
    assert_eq!(layout.left.maximized, None);
    layout.toggle_maximized(LEFT_0);
    layout.toggle_collapsed(LEFT_0);
    assert_eq!(layout.left.maximized, None, "the chevron of a maximized group restores");
}

#[test]
fn reveal_expands_a_collapsed_sidebar() {
    let mut layout = DockLayout::default();
    layout.set_sidebar_collapsed(Side::Right, true);
    layout.right.groups[1].collapsed = true;
    layout.reveal(PanelId::TagHistory);
    assert!(!layout.right.collapsed);
    assert!(!layout.right.groups[1].collapsed);
}

#[test]
fn a_rail_icon_shows_its_panel_and_leaves_the_sidebar_collapsed() {
    let mut layout = DockLayout::default();
    layout.set_sidebar_collapsed(Side::Left, true);
    layout.left.groups[0].collapsed = true;
    layout.show_panel(PanelId::Favorites);
    assert!(layout.left.collapsed, "the overlay shows it; the sidebar stays a rail");
    assert!(!layout.left.groups[0].collapsed);
    assert_eq!(layout.left.groups[0].active, PanelId::Favorites);
    assert!(layout.is_valid());
}

#[test]
fn widths_are_clamped() {
    let mut layout = DockLayout::default();
    layout.set_sidebar_width(Side::Left, 10.0);
    assert!((layout.left.width - MIN_SIDEBAR_WIDTH).abs() < f32::EPSILON);
    layout.set_sidebar_width(Side::Left, f32::NAN);
    assert!((layout.left.width - MIN_SIDEBAR_WIDTH).abs() < f32::EPSILON);
    layout.set_sidebar_width(Side::Right, 5000.0);
    assert!((layout.right.width - MAX_SIDEBAR_WIDTH).abs() < f32::EPSILON);
}

#[test]
fn the_plan_shares_the_free_height_by_weight() {
    let layout = DockLayout::default();
    let p = plan::plan_sidebar(&layout.left);
    assert_eq!(p.groups.len(), 2);
    assert!((p.groups[0].frac - 0.8).abs() < 1e-6);
    assert!((p.groups[1].frac - 0.2).abs() < 1e-6);
    let height = 700.0;
    let extra = p.extra(height);
    let last = p.groups[1];
    // The groups fill the sidebar down to its bottom padding.
    assert!((last.y(extra) + last.height(extra) + metrics::SIDEBAR_PADDING - height).abs() < 1e-3);
}

#[test]
fn collapsed_groups_keep_only_their_tab_bar() {
    let mut layout = DockLayout::default();
    layout.toggle_collapsed(LEFT_0);
    let p = plan::plan_sidebar(&layout.left);
    assert!((p.groups[0].fixed - metrics::TAB_BAR_HEIGHT).abs() < f32::EPSILON);
    assert!(p.groups[0].frac.abs() < f32::EPSILON);
    assert!((p.groups[1].frac - 1.0).abs() < f32::EPSILON);
}

#[test]
fn a_resize_keeps_the_other_groups_and_the_minimums() {
    let mut layout = DockLayout::default();
    layout.move_panel(
        PanelId::TagHistory,
        DropTarget::Split {
            group: LEFT_1,
            below: true,
        },
    );
    let height = 800.0;
    let before = plan::plan_sidebar(&layout.left);
    let third_before = before.groups[2].height(before.extra(height));
    let resize = Resize::start(&layout, Side::Left, 0, height).unwrap();
    layout.set_weights(Side::Left, 0, resize.weights(-10_000.0));
    let after = plan::plan_sidebar(&layout.left);
    let extra = after.extra(height);
    assert!((after.groups[0].height(extra) - (metrics::TAB_BAR_HEIGHT + metrics::MIN_BODY)).abs() < 1e-3);
    assert!((after.groups[2].height(extra) - third_before).abs() < 1e-3);
    // A drag of 50 px moves the bar by 50 px.
    let resize = Resize::start(&layout, Side::Left, 0, height).unwrap();
    layout.set_weights(Side::Left, 0, resize.weights(50.0));
    let moved = plan::plan_sidebar(&layout.left);
    let first = moved.groups[0].height(moved.extra(height));
    assert!(
        (first - (metrics::TAB_BAR_HEIGHT + metrics::MIN_BODY + 50.0)).abs() < 1e-2,
        "{first}"
    );
}

#[test]
fn no_resize_next_to_a_collapsed_group() {
    let mut layout = DockLayout::default();
    layout.toggle_collapsed(LEFT_1);
    assert!(Resize::start(&layout, Side::Left, 0, 700.0).is_none());
}

#[test]
fn tabs_show_their_titles_then_only_the_selected_one_then_a_count() {
    use plan::metrics::{OVERFLOW_WIDTH, TAB_ICON_WIDTH, TAB_SPACING};
    let panels = [PanelId::TagRate, PanelId::Folders, PanelId::Info, PanelId::Favorites];
    let end = |l: &plan::TabLayout| {
        l.slots
            .iter()
            .filter(|s| s.shown)
            .map(|s| s.x + s.width)
            .fold(0.0, f32::max)
    };
    // Room for everything: every title (Q-P9.15 width C applies only when the tabs do not fit).
    let wide = plan::tab_slots(&panels, Some(PanelId::Info), 2000.0);
    assert!(wide.slots.iter().all(|s| s.titled && s.shown) && wide.overflow_x.is_none());
    // Less room: only the selected tab has its title, the others their symbols.
    let all = end(&wide);
    let medium = plan::tab_slots(&panels, Some(PanelId::Info), all - 10.0);
    assert_eq!(
        medium.slots.iter().map(|s| s.titled).collect::<Vec<_>>(),
        vec![false, false, true, false]
    );
    assert!(medium.slots.iter().all(|s| s.shown));
    assert!((medium.slots[0].width - TAB_ICON_WIDTH).abs() < 1e-3);
    // Even less: the selected tab, the symbols that fit in their order, and "+n" for the rest.
    let info = plan::preferred_tab_width("Info");
    let narrow = plan::tab_slots(
        &panels,
        Some(PanelId::Info),
        info + TAB_ICON_WIDTH + OVERFLOW_WIDTH + 2.0 * TAB_SPACING,
    );
    assert_eq!(
        narrow.slots.iter().map(|s| s.shown).collect::<Vec<_>>(),
        vec![true, false, true, false]
    );
    assert_eq!(narrow.hidden(), 2);
    let button = narrow.overflow_x.unwrap();
    assert!(
        (button - end(&narrow) - TAB_SPACING).abs() < 1e-3,
        "the button follows the last tab"
    );
    assert!(
        narrow
            .slots
            .iter()
            .filter(|s| !s.shown)
            .all(|s| s.width == 0.0 && (s.x - button).abs() < 1e-3)
    );
    assert_eq!(plan::insertion_index(&wide.slots, 0.0), 0);
    assert_eq!(plan::insertion_index(&wide.slots, 5000.0), 4);
}

fn frame() -> DockFrame {
    DockFrame {
        left: Rect {
            x: 0.0,
            y: 30.0,
            w: 330.0,
            h: 700.0,
        },
        right: Rect {
            x: 970.0,
            y: 30.0,
            w: 310.0,
            h: 700.0,
        },
        center: Rect {
            x: 336.0,
            y: 30.0,
            w: 628.0,
            h: 700.0,
        },
    }
}

#[test]
fn hit_testing_finds_tab_bars_splits_and_bodies() {
    let layout = DockLayout::default();
    let f = frame();
    let left0 = plan::group_rect(&layout.left, f.left, 0).unwrap();
    // The tab bar of the first left group, far right: appended there.
    let hit = plan::hit_test(&layout, &f, PanelId::TagHistory, 300.0, left0.y + 10.0).unwrap();
    assert_eq!(
        hit.target,
        DropTarget::Tabs {
            group: LEFT_0,
            index: 2
        }
    );
    // Just below the tab bar: a split above.
    let hit = plan::hit_test(
        &layout,
        &f,
        PanelId::TagHistory,
        100.0,
        left0.y + metrics::TAB_BAR_HEIGHT + 5.0,
    )
    .unwrap();
    assert_eq!(
        hit.target,
        DropTarget::Split {
            group: LEFT_0,
            below: false
        }
    );
    // The middle of the body: into the group.
    let hit = plan::hit_test(&layout, &f, PanelId::TagHistory, 100.0, left0.y + left0.h / 2.0).unwrap();
    assert_eq!(
        hit.target,
        DropTarget::Tabs {
            group: LEFT_0,
            index: 2
        }
    );
    // The center of the window is not a drop target while both sidebars exist.
    assert!(plan::hit_test(&layout, &f, PanelId::TagHistory, 600.0, 300.0).is_none());
}

#[test]
fn hit_testing_skips_targets_that_change_nothing() {
    let layout = DockLayout::default();
    let f = frame();
    let left1 = plan::group_rect(&layout.left, f.left, 1).unwrap();
    // Info is alone in its group: splitting it off itself does nothing.
    assert!(
        plan::hit_test(
            &layout,
            &f,
            PanelId::Info,
            100.0,
            left1.y + metrics::TAB_BAR_HEIGHT + 4.0
        )
        .is_none()
    );
}

#[test]
fn an_empty_sidebar_offers_an_edge_strip() {
    let mut layout = DockLayout::default();
    for panel in [PanelId::TagRate, PanelId::TagHistory] {
        layout.move_panel(
            panel,
            DropTarget::Tabs {
                group: LEFT_1,
                index: 0,
            },
        );
    }
    let f = frame();
    let x = f.center.x + f.center.w - 10.0;
    let hit = plan::hit_test(&layout, &f, PanelId::TagHistory, x, 300.0).unwrap();
    assert_eq!(hit.target, DropTarget::Sidebar(Side::Right));
}

/// One user action on the layout.
#[derive(Debug, Clone)]
enum Op {
    Move(usize, u8, u8, usize, bool),
    Click(usize),
    Reveal(usize),
    Collapse(u8, usize),
    Maximize(u8, usize),
    SidebarCollapsed(u8, bool),
    Width(u8, f32),
    Resize(u8, usize, f32),
}

fn side(n: u8) -> Side {
    if n.is_multiple_of(2) { Side::Left } else { Side::Right }
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        4 => (0..6usize, 0..3u8, 0..2u8, 0..5usize, any::<bool>()).prop_map(|(p, k, s, i, b)| Op::Move(p, k, s, i, b)),
        2 => (0..6usize).prop_map(Op::Click),
        1 => (0..6usize).prop_map(Op::Reveal),
        1 => (0..2u8, 0..4usize).prop_map(|(s, i)| Op::Collapse(s, i)),
        1 => (0..2u8, 0..4usize).prop_map(|(s, i)| Op::Maximize(s, i)),
        1 => (0..2u8, any::<bool>()).prop_map(|(s, c)| Op::SidebarCollapsed(s, c)),
        1 => (0..2u8, -100.0f32..2000.0).prop_map(|(s, w)| Op::Width(s, w)),
        1 => (0..2u8, 0..4usize, -900.0f32..900.0).prop_map(|(s, i, d)| Op::Resize(s, i, d)),
    ]
}

fn apply(layout: &mut DockLayout, op: &Op) {
    let panel = |p: usize| PanelId::ALL[p % PanelId::ALL.len()];
    match *op {
        Op::Move(p, kind, s, index, below) => {
            let group = GroupRef { side: side(s), index };
            let target = match kind {
                0 => DropTarget::Tabs { group, index },
                1 => DropTarget::Split { group, below },
                _ => DropTarget::Sidebar(side(s)),
            };
            layout.move_panel(panel(p), target);
        }
        Op::Click(p) => layout.click_tab(panel(p)),
        Op::Reveal(p) => layout.reveal(panel(p)),
        Op::Collapse(s, index) => layout.toggle_collapsed(GroupRef { side: side(s), index }),
        Op::Maximize(s, index) => layout.toggle_maximized(GroupRef { side: side(s), index }),
        Op::SidebarCollapsed(s, c) => layout.set_sidebar_collapsed(side(s), c),
        Op::Width(s, w) => layout.set_sidebar_width(side(s), w),
        Op::Resize(s, upper, delta) => {
            if let Some(resize) = Resize::start(layout, side(s), upper, 760.0) {
                layout.set_weights(side(s), upper, resize.weights(delta));
            }
        }
    }
}

proptest! {
    #[test]
    fn every_sequence_of_actions_keeps_the_layout_valid(ops in prop::collection::vec(op(), 0..60)) {
        let mut layout = DockLayout::default();
        for op in &ops {
            apply(&mut layout, op);
            prop_assert!(layout.is_valid(), "invalid after {:?}: {:?}", op, layout);
        }
    }

    #[test]
    fn every_reachable_layout_survives_a_save_and_load(ops in prop::collection::vec(op(), 0..40)) {
        let mut layout = DockLayout::default();
        for op in &ops {
            apply(&mut layout, op);
        }
        let value = persist::to_json(&layout).unwrap();
        let text = serde_json::to_string(&value).unwrap();
        let read: serde_json::Value = serde_json::from_str(&text).unwrap();
        prop_assert_eq!(persist::from_json(Some(&read), DockLayout::default()), layout);
    }

    #[test]
    fn sanitize_repairs_any_stored_layout(
        groups in prop::collection::vec(
            (prop::collection::vec(prop::sample::select(vec!["folders", "favorites", "info", "tag-rate",
                "tag-history", "unknown"]), 0..5), -5.0f32..5.0, any::<bool>()),
            0..6),
        split in 0usize..6,
        width in -1000.0f32..3000.0,
        maximized in prop::option::of(0usize..8),
    ) {
        let mut left = Vec::new();
        let mut right = Vec::new();
        for (i, (tabs, weight, collapsed)) in groups.iter().enumerate() {
            let group = serde_json::json!({
                "tabs": tabs,
                "active": tabs.first().copied().unwrap_or("info"),
                "weight": weight,
                "collapsed": collapsed,
            });
            if i < split { left.push(group) } else { right.push(group) }
        }
        let value = serde_json::json!({
            "version": 1,
            "left": { "width": width, "maximized": maximized, "groups": left },
            "right": { "groups": right },
        });
        let mut layout = persist::from_json(Some(&value), DockLayout::default());
        prop_assert!(layout.is_valid(), "{:?}", layout);
        let once = layout.clone();
        layout.sanitize();
        prop_assert_eq!(layout, once);
    }

    #[test]
    fn a_hit_always_names_an_existing_target_and_changes_the_layout(
        ops in prop::collection::vec(op(), 0..20), p in 0usize..5, x in -50.0f32..1330.0, y in 0.0f32..760.0,
    ) {
        let mut layout = DockLayout::default();
        for op in &ops {
            apply(&mut layout, op);
        }
        let panel = PanelId::ALL[p];
        if let Some(hit) = plan::hit_test(&layout, &frame(), panel, x, y) {
            let mut moved = layout.clone();
            prop_assert!(moved.move_panel(panel, hit.target));
            prop_assert!(moved.is_valid());
            prop_assert!(moved != layout);
        }
    }
}
