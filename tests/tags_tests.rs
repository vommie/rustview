use regex::RegexBuilder;
use rustview::library::models::{MediaSearchParams, TagRecord};
use rustview::library::Library;
use rustview::query::{MatchMode, MediaTypeFilter, ScopeMode};
use rustview::settings::SettingsManager;
use rustview::ui::bridge_state::UiState;
use rustview::ui::bridge_tags::{
    BridgeTags, TagFlattenContext, TagSelectionStats, TagTreeBuilder, MAX_TAG_ITEM_WIDTH,
    MIN_TAG_ITEM_WIDTH,
};
use rustview::ui::TagNodeView;
use slint::Model;
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
fn test_tag_selection_stats_and_partial_ratings() {
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

    // Set ratings: m1 = 4, m2 = 4, m3 = 5
    lib.media().set_rating(m1, 4).unwrap();
    lib.media().set_rating(m2, 4).unwrap();
    lib.media().set_rating(m3, 5).unwrap();

    // Assign tags:
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
    assert_eq!(stats.ratings_in_selection, HashSet::from([4])); // Uniform rating 4

    // Selection containing all 3 files (mixed ratings 4 and 5)
    sel.insert(m3);
    let stats3 = TagSelectionStats::compute(&sel, &lib);
    assert_eq!(stats3.selected_files_count, 3);
    assert_eq!(stats3.tag_counts.get(&t1.id), Some(&3)); // Nature: 3/3 -> Full
    assert_eq!(stats3.tag_counts.get(&t2.id), Some(&2)); // Urban: 2/3 -> Partial
    assert_eq!(stats3.tag_counts.get(&t3.id), Some(&1)); // Night: 1/3 -> Partial
    assert_eq!(stats3.ratings_in_selection, HashSet::from([4, 5])); // Partial ratings 4 and 5
}

#[test]
fn test_database_search_all_vs_any_and_scope_modes() {
    let tmp_db = tempdir().unwrap();
    let tmp_media1 = tempdir().unwrap();
    let tmp_media2 = tempdir().unwrap();

    let lib = Library::open(tmp_db.path()).expect("Failed to open LMDB library");
    let folder1 = lib.folders().add_folder(tmp_media1.path()).unwrap();
    let folder2 = lib.folders().add_folder(tmp_media2.path()).unwrap();

    // Subfolder in Folder 1
    let subfolder_path = tmp_media1.path().join("sub");
    std::fs::create_dir_all(&subfolder_path).unwrap();
    let folder1_sub = lib.folders().add_folder(&subfolder_path).unwrap();

    // Tags
    let tag_cat = lib.tags().create_tag("Cat", None).unwrap();
    let tag_cute = lib.tags().create_tag("Cute", None).unwrap();
    let tag_wild = lib.tags().create_tag("Wild", None).unwrap();

    // Folder 1 Root Files:
    // f1_1: Cat + Cute, Rating = 5
    let p1_1 = tmp_media1.path().join("cat_cute.png");
    image::RgbImage::new(10, 10).save(&p1_1).unwrap();
    lib.refresher().refresh_folder(folder1.id).unwrap();
    let m1_1 = lib.media().get_by_folder(folder1.id).unwrap()[0].id;
    lib.media().set_rating(m1_1, 5).unwrap();
    lib.tags().assign_tag(m1_1, tag_cat.id).unwrap();
    lib.tags().assign_tag(m1_1, tag_cute.id).unwrap();

    // Folder 1 Subfolder Files:
    // f1_sub: Cat + Cute, Rating = 5
    let p1_sub = subfolder_path.join("kitten.png");
    image::RgbImage::new(10, 10).save(&p1_sub).unwrap();
    lib.refresher().refresh_folder(folder1_sub.id).unwrap();
    let m1_sub = lib.media().get_by_folder(folder1_sub.id).unwrap()[0].id;
    lib.media().set_rating(m1_sub, 5).unwrap();
    lib.tags().assign_tag(m1_sub, tag_cat.id).unwrap();
    lib.tags().assign_tag(m1_sub, tag_cute.id).unwrap();

    // Folder 2 Unrelated Files:
    // f2_1: Cat + Wild, Rating = 5
    let p2_1 = tmp_media2.path().join("tiger.png");
    image::RgbImage::new(10, 10).save(&p2_1).unwrap();
    lib.refresher().refresh_folder(folder2.id).unwrap();
    let m2_1 = lib.media().get_by_folder(folder2.id).unwrap()[0].id;
    lib.media().set_rating(m2_1, 5).unwrap();
    lib.tags().assign_tag(m2_1, tag_cat.id).unwrap();
    lib.tags().assign_tag(m2_1, tag_wild.id).unwrap();

    // Test 1: Scope Folder -> Only matches m1_1 (not the subfolder)
    let params_folder = MediaSearchParams {
        rating: Some(5),
        tag_ids: HashSet::from([tag_cat.id, tag_cute.id]),
        match_mode: MatchMode::All,
        scope_mode: ScopeMode::Folder,
        folder_id: Some(folder1.id),
        media_type_filter: MediaTypeFilter::All,
    };
    let res_folder = lib.media().search(&params_folder).unwrap();
    assert_eq!(res_folder.len(), 1);
    assert_eq!(res_folder[0].id, m1_1);

    // Test 2: Scope Folder & Subfolders -> Matches m1_1 AND m1_sub (subtree of Folder 1)
    let params_subfolders = MediaSearchParams {
        rating: Some(5),
        tag_ids: HashSet::from([tag_cat.id, tag_cute.id]),
        match_mode: MatchMode::All,
        scope_mode: ScopeMode::FolderAndSubfolders,
        folder_id: Some(folder1.id),
        media_type_filter: MediaTypeFilter::All,
    };
    let res_subfolders = lib.media().search(&params_subfolders).unwrap();
    let sub_ids: HashSet<_> = res_subfolders.iter().map(|m| m.id).collect();
    assert_eq!(sub_ids, HashSet::from([m1_1, m1_sub]));

    // Test 3: Scope Global -> Matches m1_1, m1_sub, AND m2_1 (where applicable)
    let params_global = MediaSearchParams {
        rating: Some(5),
        tag_ids: HashSet::from([tag_cat.id]),
        match_mode: MatchMode::All,
        scope_mode: ScopeMode::Global,
        folder_id: None,
        media_type_filter: MediaTypeFilter::All,
    };
    let res_global = lib.media().search(&params_global).unwrap();
    let glob_ids: HashSet<_> = res_global.iter().map(|m| m.id).collect();
    assert_eq!(glob_ids, HashSet::from([m1_1, m1_sub, m2_1]));

    // Test 4: MediaTypeFilter Video vs Image check
    let params_videos_only = MediaSearchParams {
        rating: Some(5),
        tag_ids: HashSet::from([tag_cat.id]),
        match_mode: MatchMode::All,
        scope_mode: ScopeMode::Global,
        folder_id: None,
        media_type_filter: MediaTypeFilter::Videos,
    };
    let res_videos = lib.media().search(&params_videos_only).unwrap();
    assert_eq!(res_videos.len(), 0); // All test fixtures were images (.png)
}

#[test]
fn test_tag_tree_filter_substring_and_regex() {
    let tags = vec![
        TagRecord {
            id: 1,
            parent_id: None,
            name: "Animals".to_string(),
        },
        TagRecord {
            id: 2,
            parent_id: Some(1),
            name: "Mammals".to_string(),
        },
        TagRecord {
            id: 3,
            parent_id: Some(2),
            name: "Canines".to_string(),
        },
        TagRecord {
            id: 4,
            parent_id: None,
            name: "Landscapes".to_string(),
        },
    ];

    let tree = TagTreeBuilder::build_hierarchy(&tags);
    let expanded = HashSet::from([1, 2]);
    let selected = HashSet::new();
    let stats = TagSelectionStats::default();

    // 1. Substring Filter ("can") -> Animals & Mammals dimmed ancestors, Canines matched
    let ctx_sub = TagFlattenContext {
        expanded_set: &expanded,
        selected_query_tags: &selected,
        filter_text: "can",
        filter_regex: None,
        is_regex: false,
        stats: &stats,
        is_assign_mode: false,
    };
    let mut out_sub = Vec::new();
    TagTreeBuilder::flatten(&tree, 0, &ctx_sub, &mut out_sub);

    assert_eq!(out_sub.len(), 3);
    assert_eq!(out_sub[0].name, "Animals");
    assert!(out_sub[0].is_dimmed);
    assert_eq!(out_sub[1].name, "Mammals");
    assert!(out_sub[1].is_dimmed);
    assert_eq!(out_sub[2].name, "Canines");
    assert!(!out_sub[2].is_dimmed);

    // 2. Regex Filter (r"^Land.*") -> Matches Landscapes only
    let regex_land = RegexBuilder::new(r"^Land.*")
        .case_insensitive(true)
        .build()
        .unwrap();
    let ctx_regex = TagFlattenContext {
        expanded_set: &expanded,
        selected_query_tags: &selected,
        filter_text: "^land.*",
        filter_regex: Some(&regex_land),
        is_regex: true,
        stats: &stats,
        is_assign_mode: false,
    };
    let mut out_regex = Vec::new();
    TagTreeBuilder::flatten(&tree, 0, &ctx_regex, &mut out_regex);

    assert_eq!(out_regex.len(), 1);
    assert_eq!(out_regex[0].name, "Landscapes");
    assert!(!out_regex[0].is_dimmed);

    // 3. Regex Filter matching suffix (r".*s$") -> Matches Animals, Mammals, Canines, Landscapes
    let regex_suffix = RegexBuilder::new(r".*s$")
        .case_insensitive(true)
        .build()
        .unwrap();
    let ctx_suffix = TagFlattenContext {
        expanded_set: &expanded,
        selected_query_tags: &selected,
        filter_text: ".*s$",
        filter_regex: Some(&regex_suffix),
        is_regex: true,
        stats: &stats,
        is_assign_mode: false,
    };
    let mut out_suffix = Vec::new();
    TagTreeBuilder::flatten(&tree, 0, &ctx_suffix, &mut out_suffix);

    assert_eq!(out_suffix.len(), 4);
    assert!(!out_suffix[0].is_dimmed);
    assert!(!out_suffix[1].is_dimmed);
    assert!(!out_suffix[2].is_dimmed);
    assert!(!out_suffix[3].is_dimmed);
}

#[test]
fn test_tag_tree_filter_invalid_regex_safety() {
    let tags = vec![
        TagRecord {
            id: 1,
            parent_id: None,
            name: "Animals".to_string(),
        },
        TagRecord {
            id: 2,
            parent_id: Some(1),
            name: "Canines".to_string(),
        },
    ];
    let tree = TagTreeBuilder::build_hierarchy(&tags);
    let expanded = HashSet::from([1]);
    let selected = HashSet::new();
    let stats = TagSelectionStats::default();

    // Invalid regex syntax "\" (fails compilation)
    let filter_text = "\\";
    let regex_matcher = RegexBuilder::new(filter_text)
        .case_insensitive(true)
        .build()
        .ok();
    assert!(regex_matcher.is_none());

    let ctx = TagFlattenContext {
        expanded_set: &expanded,
        selected_query_tags: &selected,
        filter_text: "\\",
        filter_regex: regex_matcher.as_ref(),
        is_regex: true,
        stats: &stats,
        is_assign_mode: false,
    };
    let mut out = Vec::new();
    TagTreeBuilder::flatten(&tree, 0, &ctx, &mut out);

    // Matches 0 rows without panic, loop or stack overflow
    assert_eq!(out.len(), 0);
}

#[test]
fn test_query_reset_sensitivity_spot() {
    let mut state = UiState::default();

    // 1. Initial default state -> Not resettable
    assert!(!state.is_query_resettable());

    // 2. Select rating 4 in Query mode -> Resettable
    state.selected_rating = 4;
    assert!(state.is_query_resettable());

    // 3. Reset rating back to 0 -> Not resettable
    state.selected_rating = 0;
    assert!(!state.is_query_resettable());

    // 4. Select a tag in Query mode -> Resettable
    state.filter.tag_ids.insert(101);
    assert!(state.is_query_resettable());

    // 5. Clear tags -> Not resettable
    state.filter.tag_ids.clear();
    assert!(!state.is_query_resettable());

    // 6. Active query flag set -> Resettable
    state.is_query_active = true;
    assert!(state.is_query_resettable());

    // 7. In Assign mode -> Always false (disabled) regardless of rating or tags
    state.is_assign_mode = true;
    assert!(!state.is_query_resettable());
}

#[test]
fn test_query_fast_path_when_inactive() {
    let mut state = UiState::default();
    assert!(!state.is_query_resettable());
    assert!(!state.is_query_active);

    state.filter.match_mode = MatchMode::Any;
    assert!(!state.is_query_resettable());

    state.filter.scope_mode = ScopeMode::Global;
    assert!(!state.is_query_resettable());
}

#[test]
fn test_tag_tree_column_chunking_and_spot_constants() {
    // Verify Single Point of Truth boundary constants
    assert_eq!(MAX_TAG_ITEM_WIDTH, 180.0);
    assert_eq!(MIN_TAG_ITEM_WIDTH, 60.0);

    let nodes = vec![
        TagNodeView {
            id: 1,
            name: "Short".into(),
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_selected: false,
            is_assigned: false,
            is_partial: false,
            is_dimmed: false,
        },
        TagNodeView {
            id: 2,
            name: "Medium Name Tag".into(),
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_selected: false,
            is_assigned: false,
            is_partial: false,
            is_dimmed: false,
        },
        TagNodeView {
            id: 3,
            name: "Cat".into(),
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_selected: false,
            is_assigned: false,
            is_partial: false,
            is_dimmed: false,
        },
        TagNodeView {
            id: 4,
            name: "This is an extremely long tag name that exceeds the limit".into(),
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_selected: false,
            is_assigned: false,
            is_partial: false,
            is_dimmed: false,
        },
        TagNodeView {
            id: 5,
            name: "Dog".into(),
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_selected: false,
            is_assigned: false,
            is_partial: false,
            is_dimmed: false,
        },
        TagNodeView {
            id: 6,
            name: "Bird".into(),
            depth: 0,
            has_children: false,
            is_expanded: false,
            is_selected: false,
            is_assigned: false,
            is_partial: false,
            is_dimmed: false,
        },
    ];

    // Compute column chunks for a container height fitting 3 rows (3 * 24px = 72px)
    let available_height = 80.0;
    let (columns, rows_per_col) = TagTreeBuilder::chunk_into_columns(nodes, available_height);

    assert_eq!(rows_per_col, 3);
    assert_eq!(columns.len(), 2);
    assert_eq!(columns[0].tags.row_count(), 3);
    assert_eq!(columns[1].tags.row_count(), 3);
}
