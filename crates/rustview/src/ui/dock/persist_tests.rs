use super::*;
use crate::ui::dock::model::{DropTarget, GroupRef, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH};

fn read(text: &str) -> DockLayout {
    let value: serde_json::Value = serde_json::from_str(text).expect("test JSON");
    from_json(Some(&value), DockLayout::default())
}

#[test]
fn no_value_gives_the_fallback_layout() {
    let mut fallback = DockLayout::default();
    fallback.left.collapsed = true;
    assert_eq!(from_json(None, fallback.clone()), fallback);
}

#[test]
fn a_changed_layout_round_trips() {
    let mut layout = DockLayout::default();
    layout.move_panel(
        PanelId::TagHistory,
        DropTarget::Split {
            group: GroupRef {
                side: Side::Left,
                index: 1,
            },
            below: true,
        },
    );
    layout.set_sidebar_width(Side::Right, 400.0);
    layout.left.collapsed = true;
    layout.toggle_maximized(GroupRef {
        side: Side::Left,
        index: 0,
    });
    layout.left.set_pinned_group(Some(1));
    assert!(layout.left.pinned.is_some());
    let value = to_json(&layout).expect("stored");
    let text = serde_json::to_string(&value).expect("text");
    assert_eq!(read(&text), layout);
}

#[test]
fn unknown_panels_and_fields_are_ignored_and_missing_ones_return() {
    let layout = read(
        r#"{"version": 1, "future_field": "x",
            "left": {"width": 250.0, "groups": [{"tabs": ["folders", "timeline-v9", "folders"], "active": "timeline-v9", "weight": -3.0}]}}"#,
    );
    assert!(layout.is_valid(), "{layout:?}");
    assert!((layout.left.width - 250.0).abs() < f32::EPSILON);
    let first = &layout.left.groups[0];
    assert_eq!(first.tabs[0], PanelId::Folders);
    assert_eq!(first.active, PanelId::Folders);
    assert!((first.weight - 1.0).abs() < f32::EPSILON);
    assert!(
        first.tabs.contains(&PanelId::Favorites),
        "a missing panel joins its neighbor"
    );
    // The right sidebar was not in the value: the default one is used.
    assert_eq!(layout.right, DockLayout::default().right);
    assert_eq!(layout.left.pinned, None);
}

#[test]
fn a_held_group_follows_its_panel_and_ends_when_the_panel_leaves() {
    let mut layout = DockLayout::default();
    layout.left.set_pinned_group(Some(0));
    let panel = layout.left.pinned.expect("held");
    assert_eq!(layout.left.pinned_group(), Some(0));
    layout.left.set_pinned_group(Some(99));
    assert_eq!(layout.left.pinned, None, "an index out of range holds nothing");
    layout.left.pinned = Some(panel);
    layout.move_panel(panel, DropTarget::Sidebar(Side::Right));
    assert_eq!(layout.left.pinned_group(), None, "the panel went to the other side");
}

#[test]
fn a_value_of_the_wrong_shape_gives_the_default_layout() {
    assert_eq!(read(r#"{"left": 5}"#), DockLayout::default());
}

#[test]
fn widths_out_of_range_are_clamped() {
    let layout = read(r#"{"left": {"width": 5.0}, "right": {"width": 99999.0}}"#);
    assert!(layout.is_valid());
    assert!((layout.left.width - MIN_SIDEBAR_WIDTH).abs() < f32::EPSILON);
    assert!((layout.right.width - MAX_SIDEBAR_WIDTH).abs() < f32::EPSILON);
}

#[test]
fn the_old_fields_give_the_layout_of_0_6_13() {
    let tabs = ["favorites".to_owned(), "folders".to_owned()];
    let layout = legacy_layout(&LegacySidebars {
        left_width: 300.0,
        right_width: 320.0,
        left_tabs: &tabs,
        left_active: "folders",
        info_collapsed: true,
        history_collapsed: false,
    });
    assert!(layout.is_valid());
    assert!((layout.left.width - 300.0).abs() < f32::EPSILON);
    assert!((layout.right.width - 320.0).abs() < f32::EPSILON);
    assert_eq!(layout.left.groups[0].tabs, [PanelId::Favorites, PanelId::Folders]);
    assert_eq!(layout.left.groups[0].active, PanelId::Folders);
    assert!(layout.left.groups[1].collapsed, "Info stays folded");
    assert!(!layout.right.groups[1].collapsed);
}

#[test]
fn broken_old_fields_still_give_a_valid_layout() {
    let tabs = ["nonsense".to_owned()];
    let layout = legacy_layout(&LegacySidebars {
        left_width: f32::NAN,
        right_width: 1.0,
        left_tabs: &tabs,
        left_active: "",
        info_collapsed: false,
        history_collapsed: true,
    });
    assert!(layout.is_valid(), "{layout:?}");
    assert_eq!(layout.left.groups[0].tabs, [PanelId::Folders, PanelId::Favorites]);
    assert!(layout.right.groups[1].collapsed);
}
