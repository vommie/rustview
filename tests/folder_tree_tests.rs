use heed::BytesDecode;
use rustview::library::Library;
use rustview::library::models::FolderRecordCodec;
use rustview::ui::bridge_ctx_menu::FolderContextMenuBuilder;
use rustview::ui::bridge_folders::FolderTreeBuilder;
use rustview::ui::bridge_state::CTX_FOLDER_PURGE_DB;
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

    let mut media_counts: HashMap<u64, usize> = HashMap::new();
    let root_folder = lib.folders().get_folder_by_path(root).unwrap().unwrap();
    let sub1_folder = lib.folders().get_folder_by_path(&sub1).unwrap().unwrap();
    let sub1_a_folder = lib.folders().get_folder_by_path(&sub1_a).unwrap().unwrap();
    let sub2_folder = lib.folders().get_folder_by_path(&sub2).unwrap().unwrap();

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
    FolderTreeBuilder::flatten(
        &tree,
        0,
        &expanded_ids,
        Some(sub1_a_folder.id),
        true, // show_deleted
        true, // show_metrics
        &mut flattened,
    );

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
    FolderTreeBuilder::flatten(
        &tree,
        0,
        &expanded_ids,
        Some(sub1_a_folder.id),
        true,  // show_deleted
        false, // show_metrics = false
        &mut flattened_no_metrics,
    );

    assert_eq!(
        flattened_no_metrics[0].name.as_str(),
        root.file_name().unwrap().to_string_lossy()
    );
    assert_eq!(flattened_no_metrics[1].name.as_str(), "Sub1");
    assert_eq!(flattened_no_metrics[2].name.as_str(), "Sub1_A");
    assert_eq!(flattened_no_metrics[3].name.as_str(), "Sub2");
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
    FolderTreeBuilder::flatten(&tree, 0, &expanded_ids, None, true, true, &mut flattened);

    // Should contain: Root (depth 0), KeepBranch (depth 1), KeepSub (depth 2) -> 3 rows
    assert_eq!(flattened.len(), 3);
    assert_eq!(flattened[0].depth, 0);
    assert_eq!(flattened[1].depth, 1);
    assert_eq!(flattened[2].depth, 2);
}

#[test]
fn test_folder_context_menu_move_to_submenu() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let root = tmp_media.path();

    let sub1 = root.join("Sub1");
    let sub2 = root.join("Sub2");
    let sub1_a = sub1.join("Sub1_A");

    fs::create_dir_all(&sub1_a).unwrap();
    fs::create_dir_all(&sub2).unwrap();

    lib.folder_tree_refresher()
        .refresh_tree(root, None::<fn(usize)>)
        .unwrap();

    let folders = lib.folders().list_folders().unwrap();
    let sub1_a_folder = lib.folders().get_folder_by_path(&sub1_a).unwrap().unwrap();

    // Build context menu for sub1_a
    let menu = FolderContextMenuBuilder::build_menu(&sub1_a_folder, &folders, Some(root), false);

    // "Move to..." should be present at index 1 and enabled
    assert_eq!(menu.items[1].text, "Move to...");
    assert!(menu.items[1].is_enabled);
    assert!(!menu.items[1].children.is_empty());
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

    // Decode legacy bytes using the new FolderRecordCodec
    let decoded =
        FolderRecordCodec::bytes_decode(&encoded_old).expect("Failed to decode legacy record");
    assert_eq!(decoded.id, 42);
    assert_eq!(decoded.path, "/test/legacy");
    assert_eq!(decoded.added_at, 1700000000);
    assert!(!decoded.is_deleted);
}
