use rustview::library::models::MediaRecord;
use rustview::library::Library;
use rustview::media::{MediaFormat, MediaType};
use rustview::ui::bridge_ctx_menu_tab::TabContextMenu;
use rustview::ui::bridge_state::{
    CTX_TAB_CLOSE, CTX_TAB_DUPLICATE, CTX_TAB_MOVE_LEFT, CTX_TAB_MOVE_RIGHT, CTX_TAB_MOVE_TO_END,
    CTX_TAB_MOVE_TO_START,
};
use rustview::viewer::tab_item::{ImageBufferEvictor, TabCollection, TabTitleFormatter};
use rustview::viewer::{
    find_window_by_pid_and_title, find_window_by_title, HighResImageLoader, ImageTabPayload,
    MediaTabSession, MpvPlayer, TabPayload, ViewerAction, ViewerError, ViewerTabManager,
    MAX_CACHED_HIGH_RES_BUFFERS,
};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tempfile::tempdir;

fn create_test_session(id: u64, media_id: u64, filename: &str, title: &str) -> MediaTabSession {
    MediaTabSession {
        id,
        media_id,
        filename: filename.to_string(),
        title: title.to_string(),
        format: MediaFormat::Png,
        file_path: PathBuf::from(format!("/media/{}", filename)),
        record: MediaRecord {
            id: media_id,
            folder_id: 1,
            filename: filename.to_string(),
            file_hash: [0xAA; 16],
            file_size: 1024,
            modification_time: 1700000000,
            creation_time: 1690000000,
            duration_ms: 0,
            media_type: MediaType::Image,
            format: MediaFormat::Png,
            width: 100,
            height: 100,
            rating: 0,
            last_seen: 1,
            is_deleted: false,
            is_corrupted: false,
        },
        payload: TabPayload::Image(ImageTabPayload {
            high_res_pixels: None,
            is_loading: false,
            error_message: String::new(),
            orig_width: 100,
            orig_height: 100,
        }),
        created_at: Instant::now(),
        last_accessed: Instant::now(),
    }
}

#[test]
fn test_tab_title_lifetime_preservation_and_reordering() {
    let t1 = create_test_session(1, 100, "clip.mp4", "clip.mp4");
    let t2 = create_test_session(2, 100, "clip.mp4", "clip.mp4 (1)");
    let t3 = create_test_session(3, 100, "clip.mp4", "clip.mp4 (2)");
    let t4 = create_test_session(4, 200, "photo.jpg", "photo.jpg");

    let tabs = vec![t1, t2, t3, t4];

    // Titles must be immutable and permanent for the entire lifetime
    assert_eq!(TabTitleFormatter::compute_title(&tabs, 1), "clip.mp4");
    assert_eq!(TabTitleFormatter::compute_title(&tabs, 2), "clip.mp4 (1)");
    assert_eq!(TabTitleFormatter::compute_title(&tabs, 3), "clip.mp4 (2)");
    assert_eq!(TabTitleFormatter::compute_title(&tabs, 4), "photo.jpg");

    // Title allocation finds lowest available unique suffix
    let new_title = TabTitleFormatter::allocate_tab_title(&tabs, 100, "clip.mp4");
    assert_eq!(new_title, "clip.mp4 (3)");

    // If tab 2 ("clip.mp4 (1)") is closed, allocating a new title reuses the freed "clip.mp4 (1)"
    let tabs_subset = vec![tabs[0].clone(), tabs[2].clone()];
    let reused_title = TabTitleFormatter::allocate_tab_title(&tabs_subset, 100, "clip.mp4");
    assert_eq!(reused_title, "clip.mp4 (1)");
}

#[test]
fn test_tab_collection_isolated_crud_and_reordering() {
    let mut collection = TabCollection::new();
    assert!(collection.is_empty());
    assert_eq!(collection.len(), 0);

    let t1 = create_test_session(10, 100, "a.png", "a.png");
    let t2 = create_test_session(20, 200, "b.png", "b.png");
    let t3 = create_test_session(30, 300, "c.png", "c.png");

    collection.push(t1);
    collection.push(t2);
    collection.push(t3);

    assert_eq!(collection.len(), 3);
    assert_eq!(collection.index_of(10), Some(0));
    assert_eq!(collection.index_of(20), Some(1));
    assert_eq!(collection.index_of(30), Some(2));

    // Test Move to Start: tab 30 moves to index 0, keeping its permanent title "c.png"
    assert!(collection.move_to_start(30)); // [30 ("c.png"), 10 ("a.png"), 20 ("b.png")]
    assert_eq!(collection.index_of(30), Some(0));
    assert_eq!(collection.get(30).unwrap().title, "c.png");
    assert_eq!(collection.index_of(10), Some(1));
    assert_eq!(collection.get(10).unwrap().title, "a.png");

    // Test Move Left
    assert!(collection.move_left(20)); // [30 ("c.png"), 20 ("b.png"), 10 ("a.png")]
    assert_eq!(collection.index_of(20), Some(1));
    assert_eq!(collection.get(20).unwrap().title, "b.png");

    // Test Move Right
    assert!(collection.move_right(30)); // [20 ("b.png"), 30 ("c.png"), 10 ("a.png")]
    assert_eq!(collection.index_of(20), Some(0));
    assert_eq!(collection.index_of(30), Some(1));

    // Test Move to End
    assert!(collection.move_to_end(20)); // [30 ("c.png"), 10 ("a.png"), 20 ("b.png")]
    assert_eq!(collection.index_of(20), Some(2));
    assert_eq!(collection.get(20).unwrap().title, "b.png");

    // Test insert_after (adjacent insertion for duplication)
    let t_dup = create_test_session(99, 100, "a.png", "a.png (1)");
    let insert_pos = collection.insert_after(10, t_dup);
    assert_eq!(insert_pos, 2); // [30, 10, 99, 20]
    assert_eq!(collection.index_of(99), Some(2));
    assert_eq!(collection.get(99).unwrap().title, "a.png (1)");
    assert_eq!(collection.len(), 4);

    // Test remove
    let removed = collection.remove(99);
    assert!(removed.is_some());
    assert_eq!(removed.unwrap().1.id, 99);
    assert_eq!(collection.len(), 3);
}

#[test]
fn test_image_buffer_evictor_capacity_limits() {
    let mut sessions = vec![
        create_test_session(1, 10, "img1.png", "img1.png"),
        create_test_session(2, 20, "img2.png", "img2.png"),
        create_test_session(3, 30, "img3.png", "img3.png"),
    ];

    for s in &mut sessions {
        if let TabPayload::Image(ref mut img) = s.payload {
            img.high_res_pixels = Some(SharedPixelBuffer::<Rgb8Pixel>::new(10, 10));
        }
    }

    // Limit to 2 buffers, active tab = 3
    ImageBufferEvictor::evict_excess(&mut sessions, Some(3), 2);

    let active_has_buf = matches!(&sessions[2].payload, TabPayload::Image(img) if img.high_res_pixels.is_some());
    assert!(active_has_buf, "Active tab must never have its buffer evicted");

    let count_cached = sessions
        .iter()
        .filter(|s| matches!(&s.payload, TabPayload::Image(img) if img.high_res_pixels.is_some()))
        .count();
    assert_eq!(count_cached, 2);
}

#[test]
fn test_mpv_player_initialization_and_control_api() {
    let player_res = MpvPlayer::new();
    assert!(
        player_res.is_ok(),
        "MpvPlayer must initialize cleanly with libmpv2 on Linux"
    );

    let player = player_res.unwrap();
    let state = player.get_playback_state();
    assert!(state.is_idle);
    assert!(!state.is_playing);

    let _ = player.set_volume(85.0);
    assert_eq!(player.get_volume().round() as i32, 85);

    assert!(player.pause().is_ok());
    assert!(player.play().is_ok());

    assert!(player.seek_relative(5.0).is_ok());
    assert!(player.seek_absolute(10.0).is_ok());

    player.stop();
    let after_stop = player.get_playback_state();
    assert!(!after_stop.is_playing);
}

#[test]
fn test_mpv_player_user_pause_vs_auto_pause_semantics() {
    let player = MpvPlayer::new().unwrap();

    assert!(player.is_view_active());
    assert!(!player.is_user_paused());

    player.pause().unwrap();
    assert!(player.is_user_paused());
    assert!(player.is_paused());

    player.set_view_active(false);
    assert!(!player.is_view_active());
    assert!(player.is_user_paused());
    assert!(player.is_paused());

    player.set_view_active(true);
    assert!(player.is_view_active());
    assert!(player.is_user_paused());
    assert!(player.is_paused());

    player.play().unwrap();
    assert!(!player.is_user_paused());

    player.set_view_active(false);
    assert!(!player.is_view_active());
    assert!(!player.is_user_paused(), "Auto-pause must not flag is_user_paused");
    assert!(player.is_paused(), "Background video must be auto-paused");

    player.set_view_active(true);
    assert!(player.is_view_active());
    assert!(!player.is_user_paused());
}

#[test]
fn test_viewer_tab_manager_lifecycle_and_focus_reuse() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img1_path = tmp_media.path().join("photo1.png");
    let img2_path = tmp_media.path().join("photo2.png");
    image::RgbImage::new(100, 100).save(&img1_path).unwrap();
    image::RgbImage::new(200, 200).save(&img2_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 2);

    let m1 = media.iter().find(|m| m.filename == "photo1.png").unwrap();
    let m2 = media.iter().find(|m| m.filename == "photo2.png").unwrap();

    let tab_mgr = ViewerTabManager::new(lib.clone());

    assert_eq!(tab_mgr.active_tab_id(), None);
    assert_eq!(tab_mgr.list_tabs().len(), 0);

    let (tab1_id, is_new1) = tab_mgr.open_or_focus_media(m1.id, false).unwrap();
    assert!(is_new1);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));
    assert_eq!(tab_mgr.list_tabs().len(), 1);

    let (tab2_id, is_new2) = tab_mgr.open_or_focus_media(m2.id, false).unwrap();
    assert!(is_new2);
    assert_ne!(tab1_id, tab2_id);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab2_id));
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    let (focused_id, is_new_reopen) = tab_mgr.open_or_focus_media(m1.id, false).unwrap();
    assert!(!is_new_reopen);
    assert_eq!(focused_id, tab1_id);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    let active_tab = tab_mgr.get_active_tab().unwrap();
    assert_eq!(active_tab.id, tab1_id);
    assert_eq!(active_tab.media_id, m1.id);
    assert_eq!(active_tab.title, "photo1.png");
    assert_eq!(tab_mgr.get_active_media_id(), Some(m1.id));

    tab_mgr.activate_browse();
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    assert!(tab_mgr.set_active_tab(tab2_id));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab2_id));
}

#[test]
fn test_viewer_tab_manager_allow_multiple_tabs_option_and_iteration_suffixes() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let vid_path = tmp_media.path().join("clip_sample.mp4");
    std::fs::write(&vid_path, b"video clip payload").unwrap();

    let img_path = tmp_media.path().join("single_image.png");
    image::RgbImage::new(100, 100).save(&img_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();

    let vid_media = media.iter().find(|m| m.filename == "clip_sample.mp4").unwrap();
    let img_media = media.iter().find(|m| m.filename == "single_image.png").unwrap();

    let tab_mgr = ViewerTabManager::new(lib.clone());

    let (tab1_id, is_new1) = tab_mgr.open_or_focus_media(vid_media.id, true).unwrap();
    assert!(is_new1);
    assert_eq!(tab_mgr.list_tabs().len(), 1);

    let (tab2_id, is_new2) = tab_mgr.open_or_focus_media(vid_media.id, true).unwrap();
    assert!(is_new2);
    assert_ne!(tab1_id, tab2_id);
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    let (tab3_id, is_new3) = tab_mgr.open_or_focus_media(vid_media.id, true).unwrap();
    assert!(is_new3);
    assert_ne!(tab2_id, tab3_id);
    assert_eq!(tab_mgr.list_tabs().len(), 3);

    let tabs = tab_mgr.list_tabs();
    let title1 = ViewerTabManager::compute_tab_title(&tabs, tab1_id);
    let title2 = ViewerTabManager::compute_tab_title(&tabs, tab2_id);
    let title3 = ViewerTabManager::compute_tab_title(&tabs, tab3_id);

    assert_eq!(title1, "clip_sample.mp4");
    assert_eq!(title2, "clip_sample.mp4 (1)");
    assert_eq!(title3, "clip_sample.mp4 (2)");

    let (img_tab1_id, img_is_new1) = tab_mgr.open_or_focus_media(img_media.id, false).unwrap();
    assert!(img_is_new1);

    let (img_tab_reuse_id, img_is_new2) = tab_mgr.open_or_focus_media(img_media.id, false).unwrap();
    assert!(!img_is_new2);
    assert_eq!(img_tab_reuse_id, img_tab1_id);

    let tabs_after_img = tab_mgr.list_tabs();
    let img_title = ViewerTabManager::compute_tab_title(&tabs_after_img, img_tab1_id);
    assert_eq!(img_title, "single_image.png");

    // Move Tab 3 ("clip_sample.mp4 (2)") to the start -> It MUST retain its permanent title!
    tab_mgr.move_tab_to_start(tab3_id);
    let tabs_reordered = tab_mgr.list_tabs();
    assert_eq!(ViewerTabManager::compute_tab_title(&tabs_reordered, tab3_id), "clip_sample.mp4 (2)");
    assert_eq!(ViewerTabManager::compute_tab_title(&tabs_reordered, tab1_id), "clip_sample.mp4");

    // Close Tab 2 -> Remaining tabs keep their exact titles!
    tab_mgr.close_tab(tab2_id);
    let tabs_remaining = tab_mgr.list_tabs();
    assert_eq!(tabs_remaining.len(), 3);

    let title1_rem = ViewerTabManager::compute_tab_title(&tabs_remaining, tab1_id);
    let title3_rem = ViewerTabManager::compute_tab_title(&tabs_remaining, tab3_id);

    assert_eq!(title1_rem, "clip_sample.mp4");
    assert_eq!(title3_rem, "clip_sample.mp4 (2)");
}

#[test]
fn test_viewer_tab_repositioning_helpers() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("f1.png");
    let p2 = tmp_media.path().join("f2.png");
    let p3 = tmp_media.path().join("f3.png");
    let p4 = tmp_media.path().join("f4.png");
    image::RgbImage::new(10, 10).save(&p1).unwrap();
    image::RgbImage::new(10, 10).save(&p2).unwrap();
    image::RgbImage::new(10, 10).save(&p3).unwrap();
    image::RgbImage::new(10, 10).save(&p4).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();

    let tab_mgr = ViewerTabManager::new(lib);

    let (t1, _) = tab_mgr.open_or_focus_media(media[0].id, false).unwrap();
    let (t2, _) = tab_mgr.open_or_focus_media(media[1].id, false).unwrap();
    let (t3, _) = tab_mgr.open_or_focus_media(media[2].id, false).unwrap();
    let (t4, _) = tab_mgr.open_or_focus_media(media[3].id, false).unwrap();

    assert_eq!(tab_mgr.tab_count(), 4);
    assert_eq!(tab_mgr.get_tab_index(t1), Some(0));
    assert_eq!(tab_mgr.get_tab_index(t2), Some(1));
    assert_eq!(tab_mgr.get_tab_index(t3), Some(2));
    assert_eq!(tab_mgr.get_tab_index(t4), Some(3));

    assert!(tab_mgr.move_tab_to_start(t4));
    assert_eq!(tab_mgr.get_tab_index(t4), Some(0));
    assert_eq!(tab_mgr.get_tab_index(t1), Some(1));
    assert!(!tab_mgr.move_tab_to_start(t4));

    assert!(tab_mgr.move_tab_left(t2));
    assert_eq!(tab_mgr.get_tab_index(t2), Some(1));
    assert_eq!(tab_mgr.get_tab_index(t1), Some(2));

    assert!(tab_mgr.move_tab_right(t4));
    assert_eq!(tab_mgr.get_tab_index(t2), Some(0));
    assert_eq!(tab_mgr.get_tab_index(t4), Some(1));

    assert!(tab_mgr.move_tab_to_end(t2));
    assert_eq!(tab_mgr.get_tab_index(t2), Some(3));
    assert!(!tab_mgr.move_tab_to_end(t2));
}

#[test]
fn test_viewer_tab_duplicate_helper() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("alpha.png");
    let p2 = tmp_media.path().join("beta.png");
    image::RgbImage::new(10, 10).save(&p1).unwrap();
    image::RgbImage::new(10, 10).save(&p2).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();

    let tab_mgr = ViewerTabManager::new(lib);

    let (t1, _) = tab_mgr.open_or_focus_media(media[0].id, false).unwrap();
    let (t2, _) = tab_mgr.open_or_focus_media(media[1].id, false).unwrap();

    assert_eq!(tab_mgr.tab_count(), 2);

    let t1_dup = tab_mgr.duplicate_tab(t1).expect("Tab duplication failed");

    assert_eq!(tab_mgr.tab_count(), 3);
    assert_eq!(tab_mgr.get_tab_index(t1), Some(0));
    assert_eq!(tab_mgr.get_tab_index(t1_dup), Some(1));
    assert_eq!(tab_mgr.get_tab_index(t2), Some(2));
    assert_eq!(tab_mgr.active_tab_id(), Some(t1_dup));

    let tabs = tab_mgr.list_tabs();
    assert_eq!(ViewerTabManager::compute_tab_title(&tabs, t1), "alpha.png");
    assert_eq!(ViewerTabManager::compute_tab_title(&tabs, t1_dup), "alpha.png (1)");
    assert_eq!(ViewerTabManager::compute_tab_title(&tabs, t2), "beta.png");
}

#[test]
fn test_tab_context_menu_builder_items_and_states() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("photo_test.png");
    image::RgbImage::new(10, 10).save(&p1).unwrap();
    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();

    let tab_mgr = ViewerTabManager::new(lib);
    let (_t1, _) = tab_mgr.open_or_focus_media(media[0].id, false).unwrap();
    let (_t2, _) = tab_mgr.open_or_focus_media(media[0].id, true).unwrap();
    let (_t3, _) = tab_mgr.open_or_focus_media(media[0].id, true).unwrap();

    let tabs = tab_mgr.list_tabs();
    assert_eq!(tabs.len(), 3);

    // 1. Middle Tab (index 1 of 3): Move Left and Move Right must both be ENABLED
    let menu_middle = TabContextMenu::build_menu(&tabs[1], 1, 3, &tabs);

    assert_eq!(menu_middle.items[0].text, "Tab: photo_test.png (1)");
    assert_eq!(menu_middle.items[1].id, CTX_TAB_DUPLICATE);
    assert_eq!(menu_middle.items[1].text, "Duplicate Tab");
    assert!(menu_middle.items[1].is_enabled);
    assert!(menu_middle.items[1].icon_name.is_none());

    let move_sub = &menu_middle.items[3];
    assert_eq!(move_sub.text, "Move Tab...");
    assert!(move_sub.is_enabled);
    assert_eq!(move_sub.children.len(), 4);

    assert_eq!(move_sub.children[0].id, CTX_TAB_MOVE_TO_START);
    assert!(move_sub.children[0].is_enabled);
    assert_eq!(move_sub.children[1].id, CTX_TAB_MOVE_LEFT);
    assert!(move_sub.children[1].is_enabled);
    assert_eq!(move_sub.children[2].id, CTX_TAB_MOVE_RIGHT);
    assert!(move_sub.children[2].is_enabled);
    assert_eq!(move_sub.children[3].id, CTX_TAB_MOVE_TO_END);
    assert!(move_sub.children[3].is_enabled);

    assert_eq!(menu_middle.items[5].id, CTX_TAB_CLOSE);
    assert_eq!(menu_middle.items[5].text, "Close Tab");
    assert!(menu_middle.items[5].is_enabled);

    // 2. First Tab (index 0 of 3): Move Left and Move to Start must be DISABLED
    let menu_first = TabContextMenu::build_menu(&tabs[0], 0, 3, &tabs);
    let move_sub_first = &menu_first.items[3];
    assert!(!move_sub_first.children[0].is_enabled);
    assert!(!move_sub_first.children[1].is_enabled);
    assert!(move_sub_first.children[2].is_enabled);
    assert!(move_sub_first.children[3].is_enabled);

    // 3. Last Tab (index 2 of 3): Move Right and Move to End must be DISABLED
    let menu_last = TabContextMenu::build_menu(&tabs[2], 2, 3, &tabs);
    let move_sub_last = &menu_last.items[3];
    assert!(move_sub_last.children[0].is_enabled);
    assert!(move_sub_last.children[1].is_enabled);
    assert!(!move_sub_last.children[2].is_enabled);
    assert!(!move_sub_last.children[3].is_enabled);

    // 4. Single Tab (index 0 of 1): Entire "Move Tab..." submenu must be DISABLED
    let menu_single = TabContextMenu::build_menu(&tabs[0], 0, 1, &tabs[0..1]);
    let move_sub_single = &menu_single.items[3];
    assert!(!move_sub_single.is_enabled);
}

#[test]
fn test_viewer_tab_manager_isolated_video_tabs_and_state_preservation() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let vid1_path = tmp_media.path().join("clip_a.mp4");
    let vid2_path = tmp_media.path().join("clip_b.mp4");
    std::fs::write(&vid1_path, b"test video a bytes").unwrap();
    std::fs::write(&vid2_path, b"test video b bytes").unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 2);

    let tab_mgr = ViewerTabManager::new(lib.clone());

    let (tab1_id, is_new1) = tab_mgr.open_or_focus_media(media[0].id, false).unwrap();
    assert!(is_new1);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));

    let tab1_player = tab_mgr.get_active_player().expect("Tab 1 must have active MpvPlayer");
    let _ = tab1_player.set_volume(42.0);
    assert_eq!(tab1_player.get_volume().round() as i32, 42);

    let (tab2_id, is_new2) = tab_mgr.open_or_focus_media(media[1].id, false).unwrap();
    assert!(is_new2);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab2_id));

    let tab2_player = tab_mgr.get_active_player().expect("Tab 2 must have active MpvPlayer");
    let _ = tab2_player.set_volume(90.0);
    assert_eq!(tab2_player.get_volume().round() as i32, 90);

    assert_eq!(tab1_player.get_volume().round() as i32, 42);

    tab_mgr.activate_browse();
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert_eq!(tab_mgr.list_tabs().len(), 2);
    assert_eq!(tab1_player.get_volume().round() as i32, 42);
    assert_eq!(tab2_player.get_volume().round() as i32, 90);

    assert!(tab_mgr.set_active_tab(tab1_id));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));
    assert_eq!(tab_mgr.get_active_player().unwrap().get_volume().round() as i32, 42);

    let after_close = tab_mgr.close_tab(tab1_id);
    assert_eq!(after_close, Some(tab2_id));
    assert_eq!(tab_mgr.list_tabs().len(), 1);
    assert_eq!(tab_mgr.get_active_player().unwrap().get_volume().round() as i32, 90);
}

#[test]
fn test_viewer_tab_manager_navigate_active_tab_in_place() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img1_path = tmp_media.path().join("first.png");
    let img2_path = tmp_media.path().join("second.png");
    image::RgbImage::new(100, 100).save(&img1_path).unwrap();
    image::RgbImage::new(200, 200).save(&img2_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 2);

    let m1 = media.iter().find(|m| m.filename == "first.png").unwrap();
    let m2 = media.iter().find(|m| m.filename == "second.png").unwrap();

    let tab_mgr = ViewerTabManager::new(lib);

    let (tab_id, _) = tab_mgr.open_or_focus_media(m1.id, false).unwrap();
    assert_eq!(tab_mgr.list_tabs().len(), 1);
    assert_eq!(tab_mgr.get_active_media_id(), Some(m1.id));

    let nav_tab_id = tab_mgr.navigate_active_tab_to_media(m2.id).unwrap();
    assert_eq!(nav_tab_id, tab_id);
    assert_eq!(tab_mgr.list_tabs().len(), 1);
    assert_eq!(tab_mgr.get_active_media_id(), Some(m2.id));

    let active_tab = tab_mgr.get_active_tab().unwrap();
    assert_eq!(active_tab.filename, "second.png");
    assert_eq!(active_tab.media_id, m2.id);

    let after_close = tab_mgr.close_tab(tab_id);
    assert_eq!(after_close, None);
    assert_eq!(tab_mgr.list_tabs().len(), 0);
    assert_eq!(tab_mgr.active_tab_id(), None);
}

#[test]
fn test_viewer_tab_manager_close_fallback_rules() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let p1 = tmp_media.path().join("a.png");
    let p2 = tmp_media.path().join("b.png");
    let p3 = tmp_media.path().join("c.png");
    image::RgbImage::new(10, 10).save(&p1).unwrap();
    image::RgbImage::new(10, 10).save(&p2).unwrap();
    image::RgbImage::new(10, 10).save(&p3).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 3);

    let tab_mgr = ViewerTabManager::new(lib);

    let (t1, _) = tab_mgr.open_or_focus_media(media[0].id, false).unwrap();
    let (t2, _) = tab_mgr.open_or_focus_media(media[1].id, false).unwrap();
    let (t3, _) = tab_mgr.open_or_focus_media(media[2].id, false).unwrap();
    assert_eq!(tab_mgr.active_tab_id(), Some(t3));

    let new_active = tab_mgr.close_tab(t3);
    assert_eq!(new_active, Some(t2));
    assert_eq!(tab_mgr.active_tab_id(), Some(t2));
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    let after_t1_close = tab_mgr.close_tab(t1);
    assert_eq!(after_t1_close, Some(t2));
    assert_eq!(tab_mgr.active_tab_id(), Some(t2));
    assert_eq!(tab_mgr.list_tabs().len(), 1);

    let after_last_close = tab_mgr.close_tab(t2);
    assert_eq!(after_last_close, None);
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert_eq!(tab_mgr.list_tabs().len(), 0);
}

#[test]
fn test_viewer_tab_manager_reactive_notifications() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img_path = tmp_media.path().join("original.png");
    image::RgbImage::new(100, 100).save(&img_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    let tab_mgr = ViewerTabManager::new(lib);
    let (tab_id, _) = tab_mgr.open_or_focus_media(media.id, false).unwrap();

    // 1. Rename notification must update filename and permanent title
    tab_mgr.notify_media_renamed(media.id, "renamed.png");
    let session_renamed = tab_mgr.get_tab(tab_id).unwrap();
    assert_eq!(session_renamed.filename, "renamed.png");
    assert_eq!(session_renamed.title, "renamed.png");
    assert!(session_renamed.file_path.ends_with("renamed.png"));
    assert_eq!(session_renamed.record.filename, "renamed.png");

    // 2. Test refresh notification
    let mut updated_record = media.clone();
    updated_record.rating = 5;
    updated_record.width = 1920;
    updated_record.height = 1080;
    tab_mgr.notify_media_refreshed(media.id, &updated_record);

    let session_refreshed = tab_mgr.get_tab(tab_id).unwrap();
    assert_eq!(session_refreshed.record.rating, 5);
    assert_eq!(session_refreshed.record.width, 1920);
    if let TabPayload::Image(ref img) = session_refreshed.payload {
        assert_eq!(img.orig_width, 1920);
        assert_eq!(img.orig_height, 1080);
    }

    // 3. Test delete notification
    tab_mgr.notify_media_deleted(media.id);
    let session_deleted = tab_mgr.get_tab(tab_id).unwrap();
    assert!(session_deleted.record.is_deleted);
}

#[test]
fn test_viewer_tab_refresh_updates_image_buffer_and_metadata() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img_path = tmp_media.path().join("modifiable_photo.png");
    image::RgbImage::new(100, 100).save(&img_path).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    let tab_mgr = ViewerTabManager::new(lib.clone());
    let (tab_id, _) = tab_mgr.open_or_focus_media(media.id, false).unwrap();

    let initial_res = HighResImageLoader::load_full_resolution(&img_path).unwrap();
    tab_mgr.update_tab_image_buffer(tab_id, initial_res.0, initial_res.1, initial_res.2);

    let session_init = tab_mgr.get_tab(tab_id).unwrap();
    if let TabPayload::Image(ref img) = session_init.payload {
        assert_eq!(img.orig_width, 100);
        assert_eq!(img.orig_height, 100);
        assert!(img.high_res_pixels.is_some());
    }

    let new_img = image::RgbImage::new(400, 300);
    new_img.save(&img_path).unwrap();

    let updated_res = HighResImageLoader::load_full_resolution(&img_path).unwrap();
    assert_eq!(updated_res.0, 400);
    assert_eq!(updated_res.1, 300);

    tab_mgr.update_tab_image_buffer(tab_id, updated_res.0, updated_res.1, updated_res.2);

    let session_refreshed = tab_mgr.get_tab(tab_id).unwrap();
    if let TabPayload::Image(ref img) = session_refreshed.payload {
        assert_eq!(img.orig_width, 400);
        assert_eq!(img.orig_height, 300);
        assert_eq!(img.high_res_pixels.as_ref().unwrap().width(), 400);
        assert_eq!(img.high_res_pixels.as_ref().unwrap().height(), 300);
    }
}

#[test]
fn test_high_res_image_loader_and_error_handling() {
    let tmp = tempdir().unwrap();

    let valid_path = tmp.path().join("valid.png");
    let img = image::RgbImage::new(640, 480);
    img.save(&valid_path).unwrap();

    let res = HighResImageLoader::load_full_resolution(&valid_path);
    assert!(res.is_ok());
    let (w, h, buf) = res.unwrap();
    assert_eq!(w, 640);
    assert_eq!(h, 480);
    assert_eq!(buf.width(), 640);
    assert_eq!(buf.height(), 480);

    let missing_path = tmp.path().join("does_not_exist.jpg");
    let err_missing = HighResImageLoader::load_full_resolution(&missing_path);
    assert!(err_missing.is_err());
    match err_missing.unwrap_err() {
        ViewerError::FileNotFound(p) => assert_eq!(p, missing_path),
        other => panic!("Expected ViewerError::FileNotFound, got {:?}", other),
    }

    let corrupt_path = tmp.path().join("corrupt.png");
    std::fs::write(&corrupt_path, b"not a real image payload").unwrap();
    let err_corrupt = HighResImageLoader::load_full_resolution(&corrupt_path);
    assert!(err_corrupt.is_err());
}

#[test]
fn test_viewer_tab_manager_high_res_buffer_eviction() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let count = MAX_CACHED_HIGH_RES_BUFFERS + 3;
    let mut media_ids = Vec::new();

    for i in 0..count {
        let p = tmp_media.path().join(format!("img_{}.png", i));
        image::RgbImage::new(10, 10).save(&p).unwrap();
    }

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    for m in &media {
        media_ids.push(m.id);
    }

    let tab_mgr = ViewerTabManager::new(lib);
    let mut tab_ids = Vec::new();

    for &mid in &media_ids {
        let (tid, _) = tab_mgr.open_or_focus_media(mid, false).unwrap();
        let pixel_buf = SharedPixelBuffer::<Rgb8Pixel>::new(10, 10);
        tab_mgr.update_tab_image_buffer(tid, 10, 10, pixel_buf);
        tab_ids.push(tid);
    }

    assert_eq!(tab_mgr.list_tabs().len(), count);

    let tabs = tab_mgr.list_tabs();
    let cached_count = tabs
        .iter()
        .filter(|t| matches!(&t.payload, TabPayload::Image(img) if img.high_res_pixels.is_some()))
        .count();

    assert_eq!(
        cached_count, MAX_CACHED_HIGH_RES_BUFFERS,
        "Inactive high-resolution buffers must be pruned when exceeding memory cap"
    );
}

#[test]
fn test_viewer_action_enum_variants() {
    let action_refresh = ViewerAction::RefreshActiveTab;
    assert_eq!(action_refresh, ViewerAction::RefreshActiveTab);

    let action_next = ViewerAction::NextItem;
    assert_eq!(action_next, ViewerAction::NextItem);

    let action_fs = ViewerAction::ToggleFullscreen;
    assert_eq!(action_fs, ViewerAction::ToggleFullscreen);
}

#[test]
fn test_x11_window_finder_smoke() {
    let pid = std::process::id();
    let _ = find_window_by_pid_and_title(pid, "NonExistentDummyTitle");
    let _ = find_window_by_title("NonExistentDummyTitle");
}
