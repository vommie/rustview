use rustview::ctx_menu::{
    CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher, CtxMenuItemKind,
};
use slint::Model;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[test]
fn test_ctx_menu_builder_hierarchy_and_properties() {
    let menu = CtxMenuBuilder::new()
        .with_title("Test Context Menu")
        .header("Actions")
        .action(101, "Rename", Some("F2"), Some("edit"), true)
        .separator()
        .checkable(102, "Pinned", true, None, None, true)
        .radio(103, "Option A", false, None, None, false)
        .submenu("Ratings", Some("star"), true, |sub| {
            sub.action(201, "5 Stars", None, None, true)
                .action(202, "4 Stars", None, None, true)
        })
        .build();

    assert_eq!(menu.title, Some("Test Context Menu".to_string()));
    assert_eq!(menu.items.len(), 6);

    assert_eq!(menu.items[0].kind, CtxMenuItemKind::Header);
    assert_eq!(menu.items[1].kind, CtxMenuItemKind::Action);
    assert_eq!(menu.items[1].shortcut, Some("F2".to_string()));

    assert_eq!(menu.items[2].kind, CtxMenuItemKind::Separator);
    assert_eq!(menu.items[3].kind, CtxMenuItemKind::Checkable);
    assert!(menu.items[3].is_checked);

    assert_eq!(menu.items[4].kind, CtxMenuItemKind::Radio);
    assert!(!menu.items[4].is_enabled);

    assert_eq!(menu.items[5].kind, CtxMenuItemKind::Submenu);
    assert_eq!(menu.items[5].children.len(), 2);
    assert_eq!(menu.items[5].children[0].id, 201);
}

#[test]
fn test_ctx_menu_dispatcher_execution_and_flattening() {
    let dispatcher = CtxMenuDispatcher::new();
    let invoked_flag = Arc::new(AtomicBool::new(false));
    let invoked_flag_clone = invoked_flag.clone();
    let received_target_id = Arc::new(AtomicU64::new(0));
    let received_target_clone = received_target_id.clone();

    dispatcher.register(42 as CtxMenuActionId, move |action_id, ctx| {
        if action_id == 42 {
            invoked_flag_clone.store(true, Ordering::SeqCst);
            if let Some(&first_target) = ctx.target_ids.first() {
                received_target_clone.store(first_target, Ordering::SeqCst);
            }
        }
    });

    let context = CtxMenuContext {
        position_x: 100.0,
        position_y: 200.0,
        selected_item_count: 1,
        target_ids: vec![999],
    };

    assert!(dispatcher.dispatch(42, &context));
    assert!(invoked_flag.load(Ordering::SeqCst));
    assert_eq!(received_target_id.load(Ordering::SeqCst), 999);

    assert!(!dispatcher.dispatch(9999, &context));

    let menu = CtxMenuBuilder::new()
        .action(1, "Item 1", None, None, true)
        .submenu("Sub", None, true, |sub| {
            sub.action(2, "Sub Item 1", None, None, true)
        })
        .build();

    let (top_model, sub_model) = CtxMenuDispatcher::to_slint_models(&menu);
    assert_eq!(top_model.row_count(), 2);
    assert_eq!(sub_model.row_count(), 1);
}
