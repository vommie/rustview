use rustview::ctx_menu::{
    CtxMenuActionId, CtxMenuBuilder, CtxMenuContext, CtxMenuDispatcher, CtxMenuItemKind,
};
use rustview::library::dot_hidden::DotHiddenCache;
use rustview::library::models::FolderRecord;
use rustview::system::DesktopApp;
use rustview::ui::bridge_ctx_menu_folder::FolderContextMenu;
use rustview::ui::bridge_ctx_menu_media::MediaContextMenu;
use rustview::ui::bridge_folders::FolderTreeNode;
use rustview::ui::bridge_state::{
    CTX_ACTION_CLEAR_SELECTION, CTX_ACTION_COPY_IMAGE, CTX_ACTION_COPY_NAME, CTX_ACTION_COPY_PATH,
    CTX_ACTION_DELETE_MEDIA, CTX_ACTION_OPEN, CTX_ACTION_OPEN_IN_BG_TAB,
    CTX_ACTION_OPEN_IN_FILE_MANAGER, CTX_ACTION_OPEN_IN_FULLSCREEN, CTX_ACTION_OPEN_IN_TAB,
    CTX_ACTION_OPEN_WITH_APP_BASE, CTX_ACTION_OPEN_WITH_OTHER, CTX_ACTION_REFRESH_FILES,
    CTX_ACTION_REFRESH_METADATA, CTX_ACTION_REMOVE_FROM_DB, CTX_ACTION_RENAME,
    CTX_ACTION_SELECT_ALL, CTX_FOLDER_OPEN_IN_FILE_MANAGER, CTX_FOLDER_REFRESH_FILES_FULL,
    CTX_FOLDER_REFRESH_FILES_META, CTX_FOLDER_REFRESH_TREE_FULL,
    CTX_FOLDER_REFRESH_TREE_RELATIVE,
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

    let menu = MediaContextMenu::build_menu(1, true, false, false, &mock_apps);

    // 0: Header ("Selected File")
    assert_eq!(menu.items[0].kind, CtxMenuItemKind::Header);
    // 1: Open
    assert_eq!(menu.items[1].id, CTX_ACTION_OPEN);
    assert_eq!(menu.items[1].text, "Open");

    // 2: Open With... (Submenu directly below Open)
    assert_eq!(menu.items[2].text, "Open With...");
    assert_eq!(menu.items[2].kind, CtxMenuItemKind::Submenu);

    let open_with_sub = &menu.items[2];
    assert_eq!(open_with_sub.children.len(), 4);
    assert_eq!(open_with_sub.children[0].id, CTX_ACTION_OPEN_WITH_APP_BASE);
    assert_eq!(open_with_sub.children[0].text, "Gwenview");
    assert_eq!(open_with_sub.children[1].id, CTX_ACTION_OPEN_WITH_APP_BASE + 1);
    assert_eq!(open_with_sub.children[1].text, "GNU Image Manipulation Program");
    assert_eq!(open_with_sub.children[2].kind, CtxMenuItemKind::Separator);
    assert_eq!(open_with_sub.children[3].id, CTX_ACTION_OPEN_WITH_OTHER);
    assert_eq!(open_with_sub.children[3].text, "Other Application...");

    // 3: Open in Tab (Directly below Open With...)
    assert_eq!(menu.items[3].id, CTX_ACTION_OPEN_IN_TAB);
    assert_eq!(menu.items[3].text, "Open in Tab");
    assert_eq!(menu.items[3].shortcut, Some("DoubleClick".to_string()));

    // 4: Open in Background-Tab (Directly below Open in Tab)
    assert_eq!(menu.items[4].id, CTX_ACTION_OPEN_IN_BG_TAB);
    assert_eq!(menu.items[4].text, "Open in Background-Tab");
    assert_eq!(menu.items[4].shortcut, Some("Ctrl+MiddleClick".to_string()));

    // 5: Open in Fullscreen (Directly below Open in Background-Tab and above Open in File Manager)
    assert_eq!(menu.items[5].id, CTX_ACTION_OPEN_IN_FULLSCREEN);
    assert_eq!(menu.items[5].text, "Open in Fullscreen");
    assert_eq!(menu.items[5].shortcut, Some("MiddleClick".to_string()));

    // 6: Open in File Manager
    assert_eq!(menu.items[6].id, CTX_ACTION_OPEN_IN_FILE_MANAGER);
    assert_eq!(menu.items[6].text, "Open in File Manager");

    // 7: Separator
    assert_eq!(menu.items[7].kind, CtxMenuItemKind::Separator);
    // 8: Rename...
    assert_eq!(menu.items[8].id, CTX_ACTION_RENAME);
    // 9: Copy...
    assert_eq!(menu.items[9].text, "Copy...");

    let copy_sub = &menu.items[9];
    assert_eq!(copy_sub.children.len(), 3);
    assert_eq!(copy_sub.children[0].id, CTX_ACTION_COPY_NAME);
    assert_eq!(copy_sub.children[1].id, CTX_ACTION_COPY_PATH);
    assert_eq!(copy_sub.children[2].id, CTX_ACTION_COPY_IMAGE);

    // 10: Delete File... (Directly below Copy... without separator)
    assert_eq!(menu.items[10].id, CTX_ACTION_DELETE_MEDIA);
    assert_eq!(menu.items[10].text, "Delete File...");

    // 11: Separator
    assert_eq!(menu.items[11].kind, CtxMenuItemKind::Separator);

    // Refresh entries present
    assert!(menu.items.iter().any(|i| i.id == CTX_ACTION_REFRESH_FILES));
    assert!(menu.items.iter().any(|i| i.id == CTX_ACTION_REFRESH_METADATA));

    // Select All and Clear Selection entries present
    assert!(menu.items.iter().any(|i| i.id == CTX_ACTION_SELECT_ALL));
    assert!(menu.items.iter().any(|i| i.id == CTX_ACTION_CLEAR_SELECTION));

    // Context-sensitive Remove from Database for deleted files
    let deleted_menu = MediaContextMenu::build_menu(1, false, true, false, &mock_apps);
    let remove_item = deleted_menu.items.iter().find(|i| i.id == CTX_ACTION_REMOVE_FROM_DB).unwrap();
    assert_eq!(remove_item.text, "Remove from Database...");
}

#[test]
fn test_media_context_menu_multiple_selection_plural_labels() {
    let menu = MediaContextMenu::build_menu(4, false, false, false, &[]);

    // Verify header
    assert_eq!(menu.items[0].text, "Selected Files");

    // Verify Open in Tabs / Background-Tabs plural formatting
    let open_tab_item = menu.items.iter().find(|i| i.id == CTX_ACTION_OPEN_IN_TAB).unwrap();
    assert_eq!(open_tab_item.text, "Open in Tabs (4 Files)");
    assert_eq!(open_tab_item.shortcut, None); // Shortcuts omitted on multi-selection

    let open_bg_tab_item = menu.items.iter().find(|i| i.id == CTX_ACTION_OPEN_IN_BG_TAB).unwrap();
    assert_eq!(open_bg_tab_item.text, "Open in Background-Tabs (4 Files)");
    assert_eq!(open_bg_tab_item.shortcut, None);

    let open_fs_item = menu.items.iter().find(|i| i.id == CTX_ACTION_OPEN_IN_FULLSCREEN).unwrap();
    assert_eq!(open_fs_item.text, "Open in Fullscreen");

    let open_fm_item = menu.items.iter().find(|i| i.id == CTX_ACTION_OPEN_IN_FILE_MANAGER).unwrap();
    assert_eq!(open_fm_item.text, "Open in File Manager (4 Files)");
}

#[test]
fn test_folder_context_menu_open_in_file_manager_at_top() {
    let folder = FolderRecord {
        id: 1,
        path: "/home/user/Pictures".to_string(),
        added_at: 0,
        is_deleted: false,
    };
    let tree = vec![FolderTreeNode {
        record: folder.clone(),
        subfolders_count: 0,
        media_files_count: 0,
        children: Vec::new(),
    }];

    let mut dot_hidden_cache = DotHiddenCache::new();
    let menu = FolderContextMenu::build_menu_from_cached_tree(
        &folder,
        &tree,
        false,
        false,
        true,
        &mut dot_hidden_cache,
        false, // is_busy = false
    );

    assert_eq!(menu.items[0].kind, CtxMenuItemKind::Header);
    assert_eq!(menu.items[1].id, CTX_FOLDER_OPEN_IN_FILE_MANAGER);
    assert_eq!(menu.items[1].text, "Open in File Manager");
    assert!(menu.items[1].is_enabled);

    assert_eq!(menu.items[2].kind, CtxMenuItemKind::Separator);
    assert_eq!(menu.items[3].text, "Rename Folder...");

    // Verify "Refresh..." submenu and nested actions
    assert!(menu.items.iter().any(|i| i.text == "Refresh..."));
    let refresh_sub = menu.items.iter().find(|i| i.text == "Refresh...").unwrap();
    assert_eq!(refresh_sub.kind, CtxMenuItemKind::Submenu);
    assert!(refresh_sub.is_enabled);

    assert!(refresh_sub.children.iter().any(|i| i.id == CTX_FOLDER_REFRESH_FILES_FULL));
    assert!(refresh_sub.children.iter().any(|i| i.id == CTX_FOLDER_REFRESH_FILES_META));
    assert!(refresh_sub.children.iter().any(|i| i.id == CTX_FOLDER_REFRESH_TREE_FULL));
    assert!(refresh_sub.children.iter().any(|i| i.id == CTX_FOLDER_REFRESH_TREE_RELATIVE));

    // Verify shortcuts are None (no F5 claim)
    for child in &refresh_sub.children {
        if child.kind == CtxMenuItemKind::Action {
            assert_eq!(child.shortcut, None);
        }
    }
}
