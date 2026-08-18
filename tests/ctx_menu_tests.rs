use rustview::ctx_menu::{
    CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher, CtxMenuItemKind,
};
use rustview::library::models::FolderRecord;
use rustview::system::DesktopApp;
use rustview::ui::bridge_ctx_menu_folder::FolderContextMenu;
use rustview::ui::bridge_ctx_menu_media::MediaContextMenu;
use rustview::ui::bridge_state::{
    CTX_ACTION_COPY_IMAGE, CTX_ACTION_COPY_NAME, CTX_ACTION_COPY_PATH, CTX_ACTION_OPEN,
    CTX_ACTION_OPEN_IN_FILE_MANAGER, CTX_ACTION_OPEN_WITH_APP_BASE, CTX_ACTION_OPEN_WITH_OTHER,
    CTX_ACTION_REFRESH_FILES, CTX_ACTION_RENAME, CTX_FOLDER_OPEN_IN_FILE_MANAGER,
    CTX_FOLDER_REFRESH,
};
use slint::Model;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[test]
fn test_ctx_menu_builder_hierarchy_and_properties() {
    let menu = CtxMenuBuilder::new()
        .with_title("Test Context Menu")
        .header("Actions")
        .action(101, "Rename", Some("F2"), None, true)
        .separator()
        .checkable(102, "Pinned", true, None, None, true)
        .radio(103, "Option A", false, None, None, false)
        .submenu("Ratings", None, true, |sub| {
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

#[test]
fn test_media_context_menu_order_and_open_with_submenu() {
    let mock_apps = vec![
        DesktopApp {
            id: "org.kde.gwenview.desktop".to_string(),
            name: "Gwenview".to_string(),
            icon_name: "gwenview".to_string(),
            icon_path: None,
            exec: "gwenview %u".to_string(),
        },
        DesktopApp {
            id: "gimp.desktop".to_string(),
            name: "GNU Image Manipulation Program".to_string(),
            icon_name: "gimp".to_string(),
            icon_path: None,
            exec: "gimp-2.10 %U".to_string(),
        },
    ];

    let menu = MediaContextMenu::build_menu(1, true, false, &mock_apps);

    // 0: Header ("Selected File")
    assert_eq!(menu.items[0].kind, CtxMenuItemKind::Header);
    // 1: Open
    assert_eq!(menu.items[1].id, CTX_ACTION_OPEN);
    assert_eq!(menu.items[1].text, "Open");

    // 2: Open With... (Submenu directly below Open)
    assert_eq!(menu.items[2].text, "Open With...");
    assert_eq!(menu.items[2].kind, CtxMenuItemKind::Submenu);

    let open_with_sub = &menu.items[2];
    assert_eq!(open_with_sub.children.len(), 4); // 2 apps + 1 separator + 1 Other...
    assert_eq!(open_with_sub.children[0].id, CTX_ACTION_OPEN_WITH_APP_BASE);
    assert_eq!(open_with_sub.children[0].text, "Gwenview");
    assert_eq!(open_with_sub.children[1].id, CTX_ACTION_OPEN_WITH_APP_BASE + 1);
    assert_eq!(open_with_sub.children[1].text, "GNU Image Manipulation Program");
    assert_eq!(open_with_sub.children[2].kind, CtxMenuItemKind::Separator);
    assert_eq!(open_with_sub.children[3].id, CTX_ACTION_OPEN_WITH_OTHER);
    assert_eq!(open_with_sub.children[3].text, "Other Application...");

    // 3: Open in File Manager
    assert_eq!(menu.items[3].id, CTX_ACTION_OPEN_IN_FILE_MANAGER);
    assert_eq!(menu.items[3].text, "Open in File Manager");

    // 4: Separator
    assert_eq!(menu.items[4].kind, CtxMenuItemKind::Separator);
    // 5: Rename...
    assert_eq!(menu.items[5].id, CTX_ACTION_RENAME);
    // 6: Copy...
    assert_eq!(menu.items[6].text, "Copy...");

    let copy_sub = &menu.items[6];
    assert_eq!(copy_sub.children.len(), 3);
    assert_eq!(copy_sub.children[0].id, CTX_ACTION_COPY_NAME);
    assert_eq!(copy_sub.children[1].id, CTX_ACTION_COPY_PATH);
    assert_eq!(copy_sub.children[2].id, CTX_ACTION_COPY_IMAGE);

    // Menu also contains Refresh Files
    assert!(menu.items.iter().any(|i| i.id == CTX_ACTION_REFRESH_FILES));
}

#[test]
fn test_folder_context_menu_open_in_file_manager_at_top() {
    let folder = FolderRecord {
        id: 1,
        path: "/home/user/Pictures".to_string(),
        added_at: 0,
        is_deleted: false,
    };
    let all = vec![folder.clone()];

    let menu = FolderContextMenu::build_menu(&folder, &all, None, false);

    // 0: Header
    assert_eq!(menu.items[0].kind, CtxMenuItemKind::Header);
    // 1: Open in File Manager (first action item at top)
    assert_eq!(menu.items[1].id, CTX_FOLDER_OPEN_IN_FILE_MANAGER);
    assert_eq!(menu.items[1].text, "Open in File Manager");
    assert!(menu.items[1].is_enabled);

    // 2: Separator
    assert_eq!(menu.items[2].kind, CtxMenuItemKind::Separator);

    // 3: Rename Folder...
    assert_eq!(menu.items[3].text, "Rename Folder...");

    // Menu also contains Refresh Folder
    assert!(menu.items.iter().any(|i| i.id == CTX_FOLDER_REFRESH));
}
