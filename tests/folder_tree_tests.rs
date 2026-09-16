use heed::BytesDecode;
use rustview::library::dot_hidden::DotHiddenCache;
use rustview::library::models::FolderRecordCodec;
use rustview::library::Library;
use rustview::settings::SettingsManager;
use rustview::ui::bridge_ctx_menu::FolderContextMenuBuilder;
use rustview::ui::bridge_folders::{BridgeFolders, FolderFlattenContext, FolderTreeBuilder};
use rustview::ui::bridge_state::{
    UiState, CTX_FOLDER_REFRESH_FILES_FULL, CTX_FOLDER_REFRESH_FILES_META,
    CTX_FOLDER_REFRESH_TREE_FULL, CTX_FOLDER_REFRESH_TREE_RELATIVE,
};
use std::collections::{HashMap, HashSet};
use std::fs;
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
    assert!(report.has_changes());

    let folders = lib.folders().list_folders().unwrap();
    assert_eq!(folders.len(), 4);

    let root_folder = lib.folders().get_folder_by_path(root).unwrap().unwrap();
    let sub1_folder = lib.folders().get_folder_by_path(&sub1).unwrap().unwrap();
    let sub1_a_folder = lib.folders().get_folder_by_path(&sub1_a).unwrap().unwrap();
    let sub2_folder = lib.folders().get_folder_by_path(&sub2).unwrap().unwrap();

    // Test descendant IDs recursive query
    let descendants_root = lib.folders().get_descendant_folder_ids(root_folder.id).unwrap();
    assert_eq!(descendants_root.len(), 4);
    assert!(descendants_root.contains(&root_folder.id));
    assert!(descendants_root.contains(&sub1_folder.id));
    assert!(descendants_root.contains(&sub1_a_folder.id));
    assert!(descendants_root.contains(&sub2_folder.id));

    let descendants_sub1 = lib.folders().get_descendant_folder_ids(sub1_folder.id).unwrap();
    assert_eq!(descendants_sub1.len(), 2);
    assert!(descendants_sub1.contains(&sub1_folder.id));
    assert!(descendants_sub1.contains(&sub1_a_folder.id));
    assert!(!descendants_sub1.contains(&sub2_folder.id));

    // Test single-transaction batch media count lookup
    let batch_counts = lib.folders().get_all_media_counts().unwrap();
    assert!(batch_counts.contains_key(&root_folder.id));

    let mut media_counts: HashMap<u64, usize> = HashMap::new();
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
}

#[test]
fn test_folder_tree_refresher_subtree_and_has_changes() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let sub1 = root.join("Sub1");
    let sub2 = root.join("Sub2");
    fs::create_dir_all(&sub1).unwrap();
    fs::create_dir_all(&sub2).unwrap();

    // Initial full tree refresh
    let report_initial = lib
        .folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();
    assert_eq!(report_initial.new_folders_added, 3);
    assert!(report_initial.has_changes());

    // 1. Relative scan on Sub1 with no changes on disk
    let report_unchanged = lib
        .folder_tree_refresher()
        .refresh_subtree(&sub1, None::<fn(usize)>)
        .unwrap();
    assert_eq!(report_unchanged.total_scanned_dirs, 1);
    assert_eq!(report_unchanged.new_folders_added, 0);
    assert_eq!(report_unchanged.folders_marked_deleted, 0);
    assert_eq!(report_unchanged.folders_restored, 0);
    assert!(!report_unchanged.has_changes());

    // 2. Add sub-folder inside Sub1 on disk
    let sub1_nested = sub1.join("Nested_A");
    fs::create_dir_all(&sub1_nested).unwrap();

    let report_changed = lib
        .folder_tree_refresher()
        .refresh_subtree(&sub1, None::<fn(usize)>)
        .unwrap();
    assert_eq!(report_changed.total_scanned_dirs, 2);
    assert_eq!(report_changed.new_folders_added, 1);
    assert!(report_changed.has_changes());

    // Sub2 outside Sub1 must be completely unaffected
    let sub2_folder = lib.folders().get_folder_by_path(&sub2).unwrap().unwrap();
    assert!(!sub2_folder.is_deleted);
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

    // 1. Test when system is NOT busy
    let menu = FolderContextMenuBuilder::build_menu(
        &sub1_folder,
        &tree,
        false,
        false,
        true,
        &mut dot_hidden_cache,
        false, // is_busy = false
    );

    let refresh_sub = menu
        .items
        .iter()
        .find(|item| item.text == "Refresh...")
        .expect("Refresh... submenu must exist in folder context menu");

    assert!(refresh_sub.is_enabled);

    let files_full = refresh_sub
        .children
        .iter()
        .find(|item| item.id == CTX_FOLDER_REFRESH_FILES_FULL)
        .expect("Refresh Files (Full) must exist in Refresh... submenu");
    assert_eq!(files_full.text, "Refresh Files (Full)");
    assert_eq!(files_full.shortcut, None); // F5 hotkey claim removed
    assert!(files_full.is_enabled);

    let files_meta = refresh_sub
        .children
        .iter()
        .find(|item| item.id == CTX_FOLDER_REFRESH_FILES_META)
        .expect("Refresh Files (Meta) must exist in Refresh... submenu");
    assert_eq!(files_meta.text, "Refresh Files (Meta)");
    assert_eq!(files_meta.shortcut, None);
    assert!(files_meta.is_enabled);

    let tree_full = refresh_sub
        .children
        .iter()
        .find(|item| item.id == CTX_FOLDER_REFRESH_TREE_FULL)
        .expect("Refresh Tree (Full) must exist in Refresh... submenu");
    assert_eq!(tree_full.text, "Refresh Tree (Full)");
    assert_eq!(tree_full.shortcut, None);
    assert!(tree_full.is_enabled);

    let tree_rel = refresh_sub
        .children
        .iter()
        .find(|item| item.id == CTX_FOLDER_REFRESH_TREE_RELATIVE)
        .expect("Refresh Tree (Relative) must exist in Refresh... submenu");
    assert_eq!(tree_rel.text, "Refresh Tree (Relative)");
    assert_eq!(tree_rel.shortcut, None);
    assert!(tree_rel.is_enabled);

    let move_item = menu
        .items
        .iter()
        .find(|item| item.text == "Move to...")
        .expect("Move to... item must exist");
    assert!(move_item.is_enabled);

    // 2. Test when system IS busy -> Refresh submenu and items must be disabled!
    let menu_busy = FolderContextMenuBuilder::build_menu(
        &sub1_folder,
        &tree,
        false,
        false,
        true,
        &mut dot_hidden_cache,
        true, // is_busy = true
    );

    let refresh_sub_busy = menu_busy
        .items
        .iter()
        .find(|item| item.text == "Refresh...")
        .expect("Refresh... submenu must exist");
    assert!(!refresh_sub_busy.is_enabled);
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
}

#[test]
fn test_visible_successor_folder_resolution_node_scoped_rules() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

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

    {
        let mut st = state.write();
        st.expanded_folder_ids.insert(root_rec.id);
        st.expanded_folder_ids.insert(copy_tests_rec.id);
    }

    let succ_1 = BridgeFolders::find_visible_successor_folder(
        del_1_rec.id,
        &[del_1_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(succ_1, Some((del_2_rec.id, del_2.clone())));

    let succ_2 = BridgeFolders::find_visible_successor_folder(
        del_3_rec.id,
        &[del_3_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(succ_2, Some((del_2_rec.id, del_2.clone())));

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

    let succ_4 = BridgeFolders::find_visible_successor_folder(
        copy_tests_rec.id,
        &[copy_tests_rec.id],
        &lib,
        &settings,
        &state,
    );
    assert_eq!(succ_4, Some((tests_rec.id, tests_dir.clone())));

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
