use heed::BytesDecode;
use rustview::library::dot_hidden::DotHiddenCache;
use rustview::library::Library;
use rustview::library::models::{FolderRecord, FolderRecordCodec};
use rustview::settings::SettingsManager;
use rustview::ui::bridge_ctx_menu::FolderContextMenuBuilder;
use rustview::ui::bridge_folders::{BridgeFolders, FolderFlattenContext, FolderTreeBuilder};
use rustview::ui::bridge_state::{UiState, CTX_FOLDER_REFRESH, CTX_FOLDER_REFRESH_METADATA};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::Instant;
use tempfile::tempdir;

#[test]
fn test_folder_tree_hierarchy_building_and_counts() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    // 1. Create directory hierarchy: root -> Sub1 -> Sub1_A
    //                              root -> Sub2
    let sub1 = root.join("Sub1");
    let sub1_a = sub1.join("Sub1_A");
    let sub2 = root.join("Sub2");

    fs::create_dir_all(&sub1_a).unwrap();
    fs::create_dir_all(&sub2).unwrap();

    // 2. Perform high-speed folder refresh
    let report = lib
        .folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .expect("Folder refresh failed");

    assert_eq!(report.new_folders_added, 4); // root, sub1, sub1_a, sub2

    let folders = lib.folders().list_folders().unwrap();
    assert_eq!(folders.len(), 4);

    let root_folder = lib.folders().get_folder_by_path(root).unwrap().unwrap();
    let sub1_folder = lib.folders().get_folder_by_path(&sub1).unwrap().unwrap();
    let sub1_a_folder = lib.folders().get_folder_by_path(&sub1_a).unwrap().unwrap();
    let sub2_folder = lib.folders().get_folder_by_path(&sub2).unwrap().unwrap();

    // Test single-transaction batch media count lookup
    let batch_counts = lib.folders().get_all_media_counts().unwrap();
    assert!(batch_counts.contains_key(&root_folder.id));

    let mut media_counts: HashMap<u64, usize> = HashMap::new();
    // Simulate 10 files in root, 25 files in sub1_a
    media_counts.insert(root_folder.id, 10);
    media_counts.insert(sub1_folder.id, 0);
    media_counts.insert(sub1_a_folder.id, 25);
    media_counts.insert(sub2_folder.id, 5);

    // 3. Build hierarchy
    let tree = FolderTreeBuilder::build_hierarchy(root, &folders, &media_counts);
    assert_eq!(tree.len(), 1);

    let root_node = &tree[0];
    assert_eq!(root_node.record.id, root_folder.id);
    assert_eq!(root_node.subfolders_count, 2); // Sub1, Sub2
    assert_eq!(root_node.media_files_count, 10);

    // 4. Test Flattening with metrics enabled
    let mut expanded_ids = HashSet::new();
    expanded_ids.insert(root_folder.id);
    expanded_ids.insert(sub1_folder.id);

    let mut flattened = Vec::new();
    let mut dot_hidden_cache = DotHiddenCache::new();
    let mut ctx = FolderFlattenContext {
        expanded_ids: &expanded_ids,
        selected_id: Some(sub1_a_folder.id),
        show_deleted: true,
        show_metrics: true,
        show_hidden: false,
        support_dot_hidden: true,
        dot_hidden_cache: &mut dot_hidden_cache,
    };
    FolderTreeBuilder::flatten(&tree, 0, &mut ctx, &mut flattened);

    assert_eq!(flattened.len(), 4);
    assert_eq!(
        flattened[0].name.as_str(),
        &format!("{} (2, 10)", root.file_name().unwrap().to_string_lossy())
    );
    assert_eq!(flattened[0].depth, 0);
    assert!(flattened[0].is_expanded);

    assert_eq!(flattened[1].name.as_str(), "Sub1 (1, 0)");
    assert_eq!(flattened[1].depth, 1);

    assert_eq!(flattened[2].name.as_str(), "Sub1_A (0, 25)");
    assert_eq!(flattened[2].depth, 2);
    assert!(flattened[2].is_selected);

    assert_eq!(flattened[3].name.as_str(), "Sub2 (0, 5)");
    assert_eq!(flattened[3].depth, 1);
    assert!(!flattened[3].is_expanded);

    // 5. Test Flattening with metrics disabled
    let mut flattened_no_metrics = Vec::new();
    let mut dot_hidden_cache_no_metrics = DotHiddenCache::new();
    let mut ctx_no_metrics = FolderFlattenContext {
        expanded_ids: &expanded_ids,
        selected_id: Some(sub1_a_folder.id),
        show_deleted: true,
        show_metrics: false,
        show_hidden: false,
        support_dot_hidden: true,
        dot_hidden_cache: &mut dot_hidden_cache_no_metrics,
    };
    FolderTreeBuilder::flatten(&tree, 0, &mut ctx_no_metrics, &mut flattened_no_metrics);

    assert_eq!(
        flattened_no_metrics[0].name.as_str(),
        root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(flattened_no_metrics[1].name.as_str(), "Sub1");
    assert_eq!(flattened_no_metrics[2].name.as_str(), "Sub1_A");
    assert_eq!(flattened_no_metrics[3].name.as_str(), "Sub2");
}

#[test]
fn test_folder_context_menu_items_and_refresh_action() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let sub1 = root.join("Sub1");
    fs::create_dir_all(&sub1).unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let folders = lib.folders().list_folders().unwrap();
    let sub1_folder = lib.folders().get_folder_by_path(&sub1).unwrap().unwrap();

    let media_counts = HashMap::new();
    let tree = FolderTreeBuilder::build_hierarchy(root, &folders, &media_counts);

    let mut dot_hidden_cache = DotHiddenCache::new();

    let menu = FolderContextMenuBuilder::build_menu(
        &sub1_folder,
        &tree,
        false,
        false,
        true,
        &mut dot_hidden_cache,
    );

    // Verify "Refresh Folder (Full)" item exists and is enabled
    let refresh_item = menu
        .items
        .iter()
        .find(|item| item.id == CTX_FOLDER_REFRESH)
        .expect("Refresh Folder (Full) menu item must exist in folder context menu");

    assert_eq!(refresh_item.text, "Refresh Folder (Full)");
    assert_eq!(refresh_item.shortcut, Some("F5".to_string()));
    assert!(refresh_item.is_enabled);

    // Verify "Refresh Folder (Metadata)" item exists and is enabled
    let refresh_meta_item = menu
        .items
        .iter()
        .find(|item| item.id == CTX_FOLDER_REFRESH_METADATA)
        .expect("Refresh Folder (Metadata) menu item must exist in folder context menu");

    assert_eq!(refresh_meta_item.text, "Refresh Folder (Metadata)");
    assert!(refresh_meta_item.is_enabled);

    // Verify "Move to..." exists
    let move_item = menu
        .items
        .iter()
        .find(|item| item.text == "Move to...")
        .expect("Move to... item must exist");
    assert!(move_item.is_enabled);
}

#[test]
fn test_folder_context_menu_move_to_submenu_hides_hidden_folders() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let normal_sub1 = root.join("NormalSub1");
    let normal_sub2 = root.join("NormalSub2");
    let dot_hidden_dir = root.join(".hidden_dot_dir");
    let nested_dot = dot_hidden_dir.join("NestedInDot");
    let listed_hidden_dir = root.join("ListedHiddenDir");
    let nested_listed = listed_hidden_dir.join("NestedInListed");

    fs::create_dir_all(&normal_sub1).unwrap();
    fs::create_dir_all(&normal_sub2).unwrap();
    fs::create_dir_all(&nested_dot).unwrap();
    fs::create_dir_all(&nested_listed).unwrap();

    // Create .hidden file in root listing "ListedHiddenDir"
    let dot_hidden_file = root.join(".hidden");
    fs::write(&dot_hidden_file, "ListedHiddenDir\n").unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let folders = lib.folders().list_folders().unwrap();
    let sub1_folder = lib.folders().get_folder_by_path(&normal_sub1).unwrap().unwrap();

    let media_counts = HashMap::new();
    let tree = FolderTreeBuilder::build_hierarchy(root, &folders, &media_counts);

    let mut dot_hidden_cache = DotHiddenCache::new();

    // Helper recursive collector
    fn collect_item_texts(item: &rustview::ctx_menu::CtxMenuItem, out: &mut Vec<String>) {
        if !item.text.is_empty() {
            out.push(item.text.clone());
        }
        for child in &item.children {
            collect_item_texts(child, out);
        }
    }

    // 1. When show_hidden_folders is FALSE
    let menu_hide = FolderContextMenuBuilder::build_menu(
        &sub1_folder,
        &tree,
        false,
        false, // show_hidden_folders = false
        true,  // support_dot_hidden_files = true
        &mut dot_hidden_cache,
    );

    let move_to_hide = menu_hide
        .items
        .iter()
        .find(|item| item.text == "Move to...")
        .expect("Move to... submenu must exist");

    let mut hide_texts = Vec::new();
    for child in &move_to_hide.children {
        collect_item_texts(child, &mut hide_texts);
    }

    assert!(hide_texts.iter().any(|t| t.contains("NormalSub2")));
    assert!(!hide_texts.iter().any(|t| t.contains(".hidden_dot_dir")));
    assert!(!hide_texts.iter().any(|t| t.contains("NestedInDot")));
    assert!(!hide_texts.iter().any(|t| t.contains("ListedHiddenDir")));
    assert!(!hide_texts.iter().any(|t| t.contains("NestedInListed")));

    // 2. When show_hidden_folders is TRUE
    let menu_show = FolderContextMenuBuilder::build_menu(
        &sub1_folder,
        &tree,
        false,
        true, // show_hidden_folders = true
        true, // support_dot_hidden_files = true
        &mut dot_hidden_cache,
    );

    let move_to_show = menu_show
        .items
        .iter()
        .find(|item| item.text == "Move to...")
        .expect("Move to... submenu must exist");

    let mut show_texts = Vec::new();
    for child in &move_to_show.children {
        collect_item_texts(child, &mut show_texts);
    }

    assert!(show_texts.iter().any(|t| t.contains("NormalSub2")));
    assert!(show_texts.iter().any(|t| t.contains(".hidden_dot_dir")));
    assert!(show_texts.iter().any(|t| t.contains("ListedHiddenDir")));
}

#[test]
fn test_folder_context_menu_performance_with_many_folders() {
    let root_path = "/media/archive";
    let mut folders = Vec::with_capacity(10_000);

    // Root folder
    folders.push(FolderRecord {
        id: 1,
        path: root_path.to_string(),
        added_at: 1700000000,
        is_deleted: false,
    });

    let mut id_counter = 2u64;

    // Generate 5,000 synthetic nested and dot folders
    for branch_idx in 0..100 {
        let branch_path = format!("{}/Branch_{:03}", root_path, branch_idx);
        let is_branch_hidden = branch_idx % 5 == 0;
        let final_branch_path = if is_branch_hidden {
            format!("{}/.Branch_{:03}", root_path, branch_idx)
        } else {
            branch_path
        };

        folders.push(FolderRecord {
            id: id_counter,
            path: final_branch_path.clone(),
            added_at: 1700000000,
            is_deleted: false,
        });
        id_counter += 1;

        for sub_idx in 0..49 {
            let sub_path = format!("{}/Sub_{:02}", final_branch_path, sub_idx);
            let is_del = sub_idx % 10 == 0;

            folders.push(FolderRecord {
                id: id_counter,
                path: sub_path,
                added_at: 1700000000,
                is_deleted: is_del,
            });
            id_counter += 1;
        }
    }

    assert_eq!(folders.len(), 5001);

    let media_counts = HashMap::new();
    let tree = FolderTreeBuilder::build_hierarchy(Path::new(root_path), &folders, &media_counts);
    assert_eq!(tree.len(), 1);

    let target_folder = folders[1].clone();
    let mut dot_hidden_cache = DotHiddenCache::new();

    let t0 = Instant::now();
    let menu = FolderContextMenuBuilder::build_menu(
        &target_folder,
        &tree,
        false,
        false,
        true,
        &mut dot_hidden_cache,
    );
    let elapsed = t0.elapsed();

    assert!(
        elapsed.as_millis() < 2,
        "Building folder context menu took too long: {} ms ({} us)",
        elapsed.as_millis(),
        elapsed.as_micros()
    );

    let move_item = menu
        .items
        .iter()
        .find(|item| item.text == "Move to...")
        .expect("Move to... must exist");
    assert!(move_item.is_enabled);
}

#[test]
fn test_hidden_folders_and_dot_hidden_file_support() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let normal_dir = root.join("NormalDir");
    let dot_dir = root.join(".hidden_dot_dir");
    let listed_dir = root.join("ListedHiddenDir");
    let visible_dir = root.join("ExplicitVisibleDir");

    fs::create_dir_all(&normal_dir).unwrap();
    fs::create_dir_all(&dot_dir).unwrap();
    fs::create_dir_all(&listed_dir).unwrap();
    fs::create_dir_all(&visible_dir).unwrap();

    // Create root/.hidden file listing "ListedHiddenDir"
    let dot_hidden_path = root.join(".hidden");
    fs::write(&dot_hidden_path, "# Comments are ignored\nListedHiddenDir\nNonExistentDir\n").unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let folders = lib.folders().list_folders().unwrap();
    assert_eq!(folders.len(), 5);

    let root_folder = lib.folders().get_folder_by_path(root).unwrap().unwrap();
    let media_counts = HashMap::new();
    let tree = FolderTreeBuilder::build_hierarchy(root, &folders, &media_counts);

    let mut expanded_ids = HashSet::new();
    expanded_ids.insert(root_folder.id);

    // 1. When show_hidden = false and support_dot_hidden = true
    let mut dot_hidden_cache = DotHiddenCache::new();
    let mut ctx_hidden = FolderFlattenContext {
        expanded_ids: &expanded_ids,
        selected_id: None,
        show_deleted: true,
        show_metrics: false,
        show_hidden: false,
        support_dot_hidden: true,
        dot_hidden_cache: &mut dot_hidden_cache,
    };
    let mut flattened_hidden = Vec::new();
    FolderTreeBuilder::flatten(&tree, 0, &mut ctx_hidden, &mut flattened_hidden);

    let names: Vec<_> = flattened_hidden.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"NormalDir"));
    assert!(names.contains(&"ExplicitVisibleDir"));
    assert!(!names.contains(&".hidden_dot_dir"));
    assert!(!names.contains(&"ListedHiddenDir"));
    assert_eq!(flattened_hidden.len(), 3);

    // 2. When show_hidden = true
    let mut dot_hidden_cache_all = DotHiddenCache::new();
    let mut ctx_all = FolderFlattenContext {
        expanded_ids: &expanded_ids,
        selected_id: None,
        show_deleted: true,
        show_metrics: false,
        show_hidden: true,
        support_dot_hidden: true,
        dot_hidden_cache: &mut dot_hidden_cache_all,
    };
    let mut flattened_all = Vec::new();
    FolderTreeBuilder::flatten(&tree, 0, &mut ctx_all, &mut flattened_all);

    let all_names: Vec<_> = flattened_all.iter().map(|f| f.name.as_str()).collect();
    assert!(all_names.contains(&"NormalDir"));
    assert!(all_names.contains(&".hidden_dot_dir"));
    assert!(all_names.contains(&"ListedHiddenDir"));
    assert!(all_names.contains(&"ExplicitVisibleDir"));
    assert_eq!(flattened_all.len(), 5);
}

#[test]
fn test_visible_successor_folder_resolution_node_scoped_rules() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    // Directory Structure:
    // Root (ID 1)
    //  ├── copy_tests (ID 2)
    //  │    ├── del_folder_test_1 (ID 3)
    //  │    ├── del_folder_test_2 (ID 4)
    //  │    └── del_folder_test_3 (ID 5)
    //  └── tests (ID 6)
    let copy_tests = root.join("copy_tests");
    let del_1 = copy_tests.join("del_folder_test_1");
    let del_2 = copy_tests.join("del_folder_test_2");
    let del_3 = copy_tests.join("del_folder_test_3");
    let tests_dir = root.join("tests");

    fs::create_dir_all(&del_1).unwrap();
    fs::create_dir_all(&del_2).unwrap();
    fs::create_dir_all(&del_3).unwrap();
    fs::create_dir_all(&tests_dir).unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let root_rec = lib.folders().get_folder_by_path(root).unwrap().unwrap();
    let copy_tests_rec = lib.folders().get_folder_by_path(&copy_tests).unwrap().unwrap();
    let del_1_rec = lib.folders().get_folder_by_path(&del_1).unwrap().unwrap();
    let del_2_rec = lib.folders().get_folder_by_path(&del_2).unwrap().unwrap();
    let del_3_rec = lib.folders().get_folder_by_path(&del_3).unwrap().unwrap();
    let tests_rec = lib.folders().get_folder_by_path(&tests_dir).unwrap().unwrap();

    let settings = SettingsManager::new(tmp_config.path());
    let mut app_settings = settings.get();
    app_settings.general.root_folder = Some(root.to_path_buf());
    settings.save_quiet(&app_settings);

    let state = parking_lot::RwLock::new(UiState::default());

    // Expand Root and copy_tests
    {
        let mut st = state.write();
        st.expanded_folder_ids.insert(root_rec.id);
        st.expanded_folder_ids.insert(copy_tests_rec.id);
    }

    // SCENARIO 1: Delete del_folder_test_1 (has next sibling del_folder_test_2)
    // -> Must select next sibling: del_folder_test_2
    let succ_1 = BridgeFolders::find_visible_successor_folder(
        del_1_rec.id,
        &[del_1_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(succ_1, Some((del_2_rec.id, del_2.clone())));

    // SCENARIO 2: Delete del_folder_test_3 (last sibling under copy_tests, del_1 and del_2 exist)
    // -> Next sibling is None -> Preceding sibling ("nachrückender Ordner") del_folder_test_2 is selected!
    let succ_2 = BridgeFolders::find_visible_successor_folder(
        del_3_rec.id,
        &[del_3_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(succ_2, Some((del_2_rec.id, del_2.clone())));

    // SCENARIO 3: Delete del_folder_test_3 when del_1 and del_2 are already gone (NO SIBLINGS LEFT in copy_tests!)
    // -> MUST SELECT PARENT: copy_tests (ID 2)! MUST NEVER JUMP OUT TO 'tests' (ID 6)!
    let succ_3 = BridgeFolders::find_visible_successor_folder(
        del_3_rec.id,
        &[del_1_rec.id, del_2_rec.id, del_3_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(
        succ_3,
        Some((copy_tests_rec.id, copy_tests.clone())),
        "When no siblings remain in the current node, the parent node itself must be selected!"
    );

    // SCENARIO 4: Delete copy_tests (direct child of Root, sibling 'tests' exists)
    // -> Next sibling under Root is 'tests' (ID 6)
    let succ_4 = BridgeFolders::find_visible_successor_folder(
        copy_tests_rec.id,
        &[copy_tests_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(succ_4, Some((tests_rec.id, tests_dir.clone())));

    // SCENARIO 5: Delete 'tests' when copy_tests is also gone (NO SIBLINGS LEFT under Root)
    // -> MUST SELECT ROOT!
    let succ_5 = BridgeFolders::find_visible_successor_folder(
        tests_rec.id,
        &[copy_tests_rec.id, tests_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(
        succ_5,
        Some((root_rec.id, root.to_path_buf())),
        "When no siblings remain under root, Root must be selected!"
    );
}

#[test]
fn test_folder_deletion_preserves_root_and_sibling_expansion() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let dir_keep = root.join("KeepBranch");
    let dir_keep_sub = dir_keep.join("KeepSub");
    let dir_delete = root.join("DeleteBranch");
    let dir_delete_sub = dir_delete.join("DeleteSub");

    fs::create_dir_all(&dir_keep_sub).unwrap();
    fs::create_dir_all(&dir_delete_sub).unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let root_rec = lib.folders().get_folder_by_path(root).unwrap().unwrap();
    let keep_rec = lib
        .folders()
        .get_folder_by_path(&dir_keep)
        .unwrap()
        .unwrap();
    let delete_rec = lib
        .folders()
        .get_folder_by_path(&dir_delete)
        .unwrap()
        .unwrap();
    let delete_sub_rec = lib
        .folders()
        .get_folder_by_path(&dir_delete_sub)
        .unwrap()
        .unwrap();

    // Expand root, keep, and delete
    let mut expanded_ids = HashSet::new();
    expanded_ids.insert(root_rec.id);
    expanded_ids.insert(keep_rec.id);
    expanded_ids.insert(delete_rec.id);

    // Delete "DeleteBranch"
    let del_rep = lib
        .folder_operations()
        .delete_folder(delete_rec.id, false)
        .unwrap();
    assert!(del_rep.deleted_folder_ids.contains(&delete_rec.id));
    assert!(del_rep.deleted_folder_ids.contains(&delete_sub_rec.id));

    // Remove deleted IDs from expanded set
    for id in &del_rep.deleted_folder_ids {
        expanded_ids.remove(id);
    }

    // Root and Keep must still be expanded!
    assert!(expanded_ids.contains(&root_rec.id));
    assert!(expanded_ids.contains(&keep_rec.id));
    assert!(!expanded_ids.contains(&delete_rec.id));

    let folders = lib.folders().list_folders().unwrap();
    let media_counts = HashMap::new();
    let tree = FolderTreeBuilder::build_hierarchy(root, &folders, &media_counts);

    let mut dot_hidden_cache = DotHiddenCache::new();
    let mut ctx = FolderFlattenContext {
        expanded_ids: &expanded_ids,
        selected_id: None,
        show_deleted: true,
        show_metrics: true,
        show_hidden: false,
        support_dot_hidden: true,
        dot_hidden_cache: &mut dot_hidden_cache,
    };
    let mut flattened = Vec::new();
    FolderTreeBuilder::flatten(&tree, 0, &mut ctx, &mut flattened);

    // Should contain: Root (depth 0), KeepBranch (depth 1), KeepSub (depth 2) -> 3 rows
    assert_eq!(flattened.len(), 3);
    assert_eq!(flattened[0].depth, 0);
    assert_eq!(flattened[1].depth, 1);
    assert_eq!(flattened[2].depth, 2);
}

#[test]
fn test_legacy_folder_record_backward_compatibility() {
    #[derive(serde::Serialize)]
    struct LegacyFormat {
        id: u64,
        path: String,
        added_at: i64,
    }

    let old_data = LegacyFormat {
        id: 42,
        path: "/test/legacy".to_string(),
        added_at: 1700000000,
    };

    let encoded_old = bincode::serialize(&old_data).unwrap();

    let decoded =
        FolderRecordCodec::bytes_decode(&encoded_old).expect("Failed to decode legacy record");
    assert_eq!(decoded.id, 42);
    assert_eq!(decoded.path, "/test/legacy");
    assert_eq!(decoded.added_at, 1700000000);
    assert!(!decoded.is_deleted);
}
