use heed::BytesDecode;
use rustview::library::dot_hidden::DotHiddenCache;
use rustview::library::Library;
use rustview::library::models::FolderRecordCodec;
use rustview::settings::SettingsManager;
use rustview::ui::bridge_ctx_menu::FolderContextMenuBuilder;
use rustview::ui::bridge_folders::{FolderFlattenContext, FolderTreeBuilder};
use rustview::ui::bridge_state::{UiState, CTX_FOLDER_PURGE_DB, CTX_FOLDER_REFRESH};
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

    // Build context menu for non-root folder
    let menu = FolderContextMenuBuilder::build_menu(&sub1_folder, &folders, Some(root), false);

    // Verify "Refresh Folder" item exists and is enabled
    let refresh_item = menu
        .items
        .iter()
        .find(|item| item.id == CTX_FOLDER_REFRESH)
        .expect("Refresh Folder menu item must exist in folder context menu");

    assert_eq!(refresh_item.text, "Refresh Folder");
    assert_eq!(refresh_item.shortcut, Some("F5".to_string()));
    assert!(refresh_item.is_enabled);

    // Verify "Move to..." exists
    let move_item = menu
        .items
        .iter()
        .find(|item| item.text == "Move to...")
        .expect("Move to... item must exist");
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
fn test_collapse_all_except_active_path() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    // Branch A: root -> A1 -> A2 -> A3 (Target folder is A3)
    let a1 = root.join("BranchA");
    let a2 = a1.join("SubA2");
    let a3 = a2.join("SubA3");

    // Branch B: root -> B1 -> B2
    let b1 = root.join("BranchB");
    let b2 = b1.join("SubB2");

    // Branch C: root -> C1
    let c1 = root.join("BranchC");

    fs::create_dir_all(&a3).unwrap();
    fs::create_dir_all(&b2).unwrap();
    fs::create_dir_all(&c1).unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let root_rec = lib.folders().get_folder_by_path(root).unwrap().unwrap();
    let a1_rec = lib.folders().get_folder_by_path(&a1).unwrap().unwrap();
    let a2_rec = lib.folders().get_folder_by_path(&a2).unwrap().unwrap();
    let a3_rec = lib.folders().get_folder_by_path(&a3).unwrap().unwrap();
    let b1_rec = lib.folders().get_folder_by_path(&b1).unwrap().unwrap();
    let b2_rec = lib.folders().get_folder_by_path(&b2).unwrap().unwrap();
    let c1_rec = lib.folders().get_folder_by_path(&c1).unwrap().unwrap();

    let settings = SettingsManager::new(tmp_config.path());
    let mut app_settings = settings.get();
    app_settings.general.root_folder = Some(root.to_path_buf());
    settings.save_quiet(&app_settings);

    let state = parking_lot::RwLock::new(UiState::default());

    // Initially expand ALL branches (root, A1, A2, B1, B2, C1)
    {
        let mut st = state.write();
        st.expanded_folder_ids.insert(root_rec.id);
        st.expanded_folder_ids.insert(a1_rec.id);
        st.expanded_folder_ids.insert(a2_rec.id);
        st.expanded_folder_ids.insert(b1_rec.id);
        st.expanded_folder_ids.insert(b2_rec.id);
        st.expanded_folder_ids.insert(c1_rec.id);
        st.current_folder_id = Some(a3_rec.id);
        st.current_folder_path = Some(a3.clone());
    }

    // Call collapse logic (headless simulation)
    let app_settings = settings.get();
    let root_path_opt = app_settings
        .general
        .root_folder
        .as_ref()
        .map(|p| rustview::library::folders::normalize_path(p));

    let mut keep_expanded_ids = HashSet::new();
    if let Some(ref root_p) = root_path_opt {
        if let Ok(Some(root_r)) = lib.folders().get_folder_by_path(root_p) {
            keep_expanded_ids.insert(root_r.id);
        }
        let target_p = a3;
        let mut curr_ancestor = target_p.parent();
        while let Some(parent_p) = curr_ancestor {
            if parent_p.starts_with(root_p) {
                if let Ok(Some(parent_rec)) = lib.folders().get_folder_by_path(parent_p) {
                    keep_expanded_ids.insert(parent_rec.id);
                }
                if parent_p == root_p.as_path() {
                    break;
                }
                curr_ancestor = parent_p.parent();
            } else {
                break;
            }
        }
    }

    {
        let mut st = state.write();
        st.expanded_folder_ids.retain(|id| keep_expanded_ids.contains(id));
        for id in &keep_expanded_ids {
            st.expanded_folder_ids.insert(*id);
        }
    }

    // Assert that ONLY the active path (Root, A1, A2) is expanded!
    let final_expanded = state.read().expanded_folder_ids.clone();
    assert!(final_expanded.contains(&root_rec.id));
    assert!(final_expanded.contains(&a1_rec.id));
    assert!(final_expanded.contains(&a2_rec.id));

    // Branch B and Branch C must be collapsed
    assert!(!final_expanded.contains(&b1_rec.id));
    assert!(!final_expanded.contains(&b2_rec.id));
    assert!(!final_expanded.contains(&c1_rec.id));
    assert_eq!(final_expanded.len(), 3);
}

#[test]
fn test_in_place_count_update_in_state() {
    let mut state = UiState::default();

    let root_rec = rustview::library::models::FolderRecord {
        id: 1,
        path: "/root".to_string(),
        added_at: 0,
        is_deleted: false,
    };
    let child_rec = rustview::library::models::FolderRecord {
        id: 2,
        path: "/root/child".to_string(),
        added_at: 0,
        is_deleted: false,
    };

    let tree = vec![rustview::ui::bridge_folders::FolderTreeNode {
        record: root_rec,
        subfolders_count: 1,
        media_files_count: 0,
        children: vec![rustview::ui::bridge_folders::FolderTreeNode {
            record: child_rec,
            subfolders_count: 0,
            media_files_count: 0,
            children: Vec::new(),
        }],
    }];

    state.cached_folder_tree = Some(tree);

    // Update count in-place
    state.update_folder_media_count_in_cache(2, 42);

    assert_eq!(state.cached_media_counts.get(&2), Some(&42));
    let root_node = &state.cached_folder_tree.as_ref().unwrap()[0];
    assert_eq!(root_node.children[0].media_files_count, 42);
}

#[test]
fn test_folder_operations_rename_move_delete() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let dir_a = root.join("DirA");
    let dir_b = root.join("DirB");
    fs::create_dir_all(&dir_a).unwrap();
    fs::create_dir_all(&dir_b).unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let folder_a = lib.folders().get_folder_by_path(&dir_a).unwrap().unwrap();
    let folder_b = lib.folders().get_folder_by_path(&dir_b).unwrap().unwrap();

    // 1. Test Folder Rename
    let renamed = lib
        .folder_operations()
        .rename_folder(folder_a.id, "DirA_Renamed")
        .expect("Rename failed");

    assert_eq!(renamed.id, folder_a.id);
    assert!(root.join("DirA_Renamed").exists());
    assert!(!dir_a.exists());

    // 2. Test Folder Move (Move DirA_Renamed under DirB)
    let moved = lib
        .folder_operations()
        .move_folder(folder_a.id, folder_b.id)
        .expect("Move failed");

    assert_eq!(moved.moved_folder_id, folder_a.id);
    assert!(dir_b.join("DirA_Renamed").exists());
    assert!(!root.join("DirA_Renamed").exists());

    // 3. Test Permanent Folder Deletion (removes from LMDB completely)
    let del_rep = lib
        .folder_operations()
        .delete_folder(folder_b.id, false)
        .expect("Permanent delete failed");

    assert_eq!(del_rep.deleted_folder_ids.len(), 2); // DirB and DirA_Renamed
    assert!(!dir_b.exists());
    assert!(lib.folders().get_folder(folder_b.id).unwrap().is_none());
    assert!(lib.folders().get_folder(folder_a.id).unwrap().is_none());
}

#[test]
fn test_orphaned_folder_context_menu_and_db_purge_workflow() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let sub_orphaned = root.join("OrphanedDir");
    fs::create_dir_all(&sub_orphaned).unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let mut orphaned_rec = lib
        .folders()
        .get_folder_by_path(&sub_orphaned)
        .unwrap()
        .unwrap();

    // 1. Delete directory on physical disk and mark is_deleted = true
    fs::remove_dir_all(&sub_orphaned).unwrap();
    lib.folders().set_deleted(orphaned_rec.id, true).unwrap();
    orphaned_rec.is_deleted = true;

    let folders = lib.folders().list_folders().unwrap();

    // 2. Check Context Menu: must contain "Remove from Database..."
    let menu = FolderContextMenuBuilder::build_menu(&orphaned_rec, &folders, Some(root), false);
    let purge_action = menu
        .items
        .iter()
        .find(|item| item.id == CTX_FOLDER_PURGE_DB);
    assert!(
        purge_action.is_some(),
        "Context menu should have 'Remove from Database...' for orphaned folders"
    );
    assert_eq!(
        purge_action.unwrap().text,
        "Remove from Database...".to_string()
    );

    // 3. Test Safety Check & Rebirth: If recreated on disk, check_and_restore_if_exists restores it
    fs::create_dir_all(&sub_orphaned).unwrap();
    let restored = lib
        .folder_operations()
        .check_and_restore_if_exists(orphaned_rec.id)
        .unwrap();
    assert!(
        restored,
        "Folder was reborn on disk and should be detected"
    );

    let reloaded = lib.folders().get_folder(orphaned_rec.id).unwrap().unwrap();
    assert!(
        !reloaded.is_deleted,
        "is_deleted should be reset to false after rebirth"
    );

    // 4. Test Actual Purge when directory is truly gone
    fs::remove_dir_all(&sub_orphaned).unwrap();
    lib.folders().set_deleted(orphaned_rec.id, true).unwrap();

    let is_still_there = lib
        .folder_operations()
        .check_and_restore_if_exists(orphaned_rec.id)
        .unwrap();
    assert!(!is_still_there, "Folder is truly missing from disk");

    let purge_report = lib
        .folder_operations()
        .purge_folder_from_db(orphaned_rec.id)
        .expect("Purge from database failed");

    assert_eq!(purge_report.deleted_folder_ids, vec![orphaned_rec.id]);
    assert!(
        lib.folders().get_folder(orphaned_rec.id).unwrap().is_none(),
        "Folder must be removed completely from LMDB"
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

    let mut flattened = Vec::new();
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
