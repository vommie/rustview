use rustview::library::Library;
use rustview::settings::SettingsManager;
use rustview::ui::bridge_tags::{BridgeTags, TagSelectionStats};
use std::collections::HashSet;
use tempfile::tempdir;

#[test]
fn test_hierarchical_tags_crud_and_cycle_prevention() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    // 1. Create hierarchical structure: Animals -> Mammals -> Dogs
    let animals = lib.tags().create_tag("Animals", None).unwrap();
    assert_eq!(animals.parent_id, None);

    let mammals = lib.tags().create_tag("Mammals", Some(animals.id)).unwrap();
    assert_eq!(mammals.parent_id, Some(animals.id));

    let dogs = lib.tags().create_tag("Dogs", Some(mammals.id)).unwrap();
    assert_eq!(dogs.parent_id, Some(mammals.id));

    // 2. Reject duplicate tag names under same parent
    assert!(lib.tags().create_tag("Mammals", Some(animals.id)).is_err());
    // Allowed duplicate name under different parent
    assert!(lib.tags().create_tag("Mammals", None).is_ok());

    // 3. Rename Tag
    let renamed_dogs = lib.tags().rename_tag(dogs.id, "Canines").unwrap();
    assert_eq!(renamed_dogs.name, "Canines");
    assert_eq!(
        lib.tags().get_tag(dogs.id).unwrap().unwrap().name,
        "Canines"
    );

    // 4. Reparent / Move Tag: Move "Canines" directly to Root
    let moved_dogs = lib.tags().move_tag(dogs.id, None).unwrap();
    assert_eq!(moved_dogs.parent_id, None);

    // Move back under Mammals
    let _ = lib.tags().move_tag(dogs.id, Some(mammals.id)).unwrap();

    // 5. Prevent Cyclic Reparenting: Cannot move "Animals" under its own child "Mammals" or grandchild "Canines"
    assert!(lib.tags().move_tag(animals.id, Some(mammals.id)).is_err());
    assert!(lib.tags().move_tag(animals.id, Some(dogs.id)).is_err());

    // 6. Test Descendants Query
    let desc = lib.tags().get_descendants(animals.id).unwrap();
    let desc_names: Vec<_> = desc.iter().map(|t| t.name.as_str()).collect();
    assert!(desc_names.contains(&"Mammals"));
    assert!(desc_names.contains(&"Canines"));

    // 7. Test Media Assignment & Bulk Subtree Deletion
    let img_path = tmp_media.path().join("dog.png");
    let img = image::RgbImage::new(100, 100);
    img.save(&img_path).unwrap();
    lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    lib.tags().assign_tag(media.id, dogs.id).unwrap();
    assert_eq!(lib.tags().get_tags_for_media(media.id).unwrap().len(), 1);

    // Delete "Animals" subtree -> should delete Animals, Mammals, Canines, and unassign from media
    let deleted = lib.tags().delete_tag_subtree(animals.id).unwrap();
    assert_eq!(deleted.len(), 3);

    assert!(lib.tags().get_tag(animals.id).unwrap().is_none());
    assert!(lib.tags().get_tag(mammals.id).unwrap().is_none());
    assert!(lib.tags().get_tag(dogs.id).unwrap().is_none());

    // Media should now have 0 tags
    assert_eq!(lib.tags().get_tags_for_media(media.id).unwrap().len(), 0);
}

#[test]
fn test_tag_expansion_persistence_in_settings() {
    let tmp_config = tempdir().unwrap();
    let settings = SettingsManager::new(tmp_config.path());

    // Initially empty
    assert!(settings.get().tags.expanded_tag_ids.is_empty());

    // Persist set of expanded tags [1, 42, 100]
    let mut expanded_set = HashSet::new();
    expanded_set.insert(1);
    expanded_set.insert(42);
    expanded_set.insert(100);

    BridgeTags::persist_expanded_tags(&settings, &expanded_set);

    // Verify in-memory state
    assert_eq!(settings.get().tags.expanded_tag_ids, vec![1, 42, 100]);

    // Reload new manager from same config dir to verify disk persistence
    let reloaded_settings = SettingsManager::new(tmp_config.path());
    assert_eq!(
        reloaded_settings.get().tags.expanded_tag_ids,
        vec![1, 42, 100]
    );
}

#[test]
fn test_tag_selection_stats_calculation() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    // Create tags
    let t1 = lib.tags().create_tag("Nature", None).unwrap();
    let t2 = lib.tags().create_tag("Urban", None).unwrap();
    let t3 = lib.tags().create_tag("Night", None).unwrap();

    // Create 3 media files
    let p1 = tmp_media.path().join("f1.png");
    let p2 = tmp_media.path().join("f2.png");
    let p3 = tmp_media.path().join("f3.png");

    image::RgbImage::new(10, 10).save(&p1).unwrap();
    image::RgbImage::new(10, 10).save(&p2).unwrap();
    image::RgbImage::new(10, 10).save(&p3).unwrap();

    lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 3);

    let m1 = media[0].id;
    let m2 = media[1].id;
    let m3 = media[2].id;

    // Assign:
    // Nature: m1, m2, m3 (3/3)
    // Urban:  m1, m2     (2/3)
    // Night:  m3         (1/3)
    lib.tags().assign_tag(m1, t1.id).unwrap();
    lib.tags().assign_tag(m2, t1.id).unwrap();
    lib.tags().assign_tag(m3, t1.id).unwrap();

    lib.tags().assign_tag(m1, t2.id).unwrap();
    lib.tags().assign_tag(m2, t2.id).unwrap();

    lib.tags().assign_tag(m3, t3.id).unwrap();

    // Selection containing m1 and m2 (2 files)
    let mut sel = HashSet::new();
    sel.insert(m1);
    sel.insert(m2);

    let stats = TagSelectionStats::compute(&sel, &lib);
    assert_eq!(stats.selected_files_count, 2);
    assert_eq!(stats.tag_counts.get(&t1.id), Some(&2)); // Nature: 2/2 -> Full
    assert_eq!(stats.tag_counts.get(&t2.id), Some(&2)); // Urban: 2/2 -> Full
    assert_eq!(stats.tag_counts.get(&t3.id), None); // Night: 0/2 -> None

    // Selection containing all 3 files
    sel.insert(m3);
    let stats3 = TagSelectionStats::compute(&sel, &lib);
    assert_eq!(stats3.selected_files_count, 3);
    assert_eq!(stats3.tag_counts.get(&t1.id), Some(&3)); // Nature: 3/3 -> Full
    assert_eq!(stats3.tag_counts.get(&t2.id), Some(&2)); // Urban: 2/3 -> Partial
    assert_eq!(stats3.tag_counts.get(&t3.id), Some(&1)); // Night: 1/3 -> Partial
}
