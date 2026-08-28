use rustview::library::models::MediaRecord;
use rustview::library::Library;
use rustview::media::{MediaFormat, MediaType};
use rustview::ui::bridge_ctx_menu_tab::TabContextMenu;
use rustview::ui::bridge_state::{
    CTX_TAB_CLOSE, CTX_TAB_CLOSE_OTHERS, CTX_TAB_CLOSE_TO_LEFT, CTX_TAB_CLOSE_TO_RIGHT,
    CTX_TAB_DUPLICATE, CTX_TAB_MOVE_LEFT, CTX_TAB_MOVE_RIGHT, CTX_TAB_MOVE_TO_END,
    CTX_TAB_MOVE_TO_START,
};
use rustview::ui::tab_dnd::{TabDndAutoScroller, TabDndGeometry, TabDndSession};
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
fn test_tab_dnd_geometry_slot_computation_and_target_detection() {
    let tab_ids = vec![10, 20, 30, 40];
    let widths = vec![100.0, 150.0, 100.0, 200.0];
    let spacing = 4.0;
    let padding_start = 2.0;

    let slots = TabDndGeometry::compute_tab_slots(&tab_ids, &widths, spacing, padding_start);
    assert_eq!(slots.len(), 4);

    // Slot 0: [2.0 .. 102.0], Center = 52.0
    assert_eq!(slots[0].start_x, 2.0);
    assert_eq!(slots[0].width, 100.0);
    assert_eq!(slots[0].center_x, 52.0);

    // Slot 1: [106.0 .. 256.0], Center = 181.0
    assert_eq!(slots[1].start_x, 106.0);
    assert_eq!(slots[1].width, 150.0);
    assert_eq!(slots[1].center_x, 181.0);

    // Slot 2: [260.0 .. 360.0], Center = 310.0
    assert_eq!(slots[2].start_x, 260.0);
    assert_eq!(slots[2].width, 100.0);
    assert_eq!(slots[2].center_x, 310.0);

    // Slot 3: [364.0 .. 564.0], Center = 464.0
    assert_eq!(slots[3].start_x, 364.0);
    assert_eq!(slots[3].width, 200.0);
    assert_eq!(slots[3].center_x, 464.0);

    // 1. Drag Tab 0 (Source index 0) rightwards past center of Slot 1 (Center = 181.0)
    let target_r1 = TabDndGeometry::calculate_target_index(&slots, 0, 190.0, 0);
    assert_eq!(target_r1, 1);

    // 2. Drag Tab 0 further past center of Slot 2 (Center = 310.0)
    let target_r2 = TabDndGeometry::calculate_target_index(&slots, 0, 320.0, 1);
    assert_eq!(target_r2, 2);

    // 3. Drag Tab 3 (Source index 3) leftwards past center of Slot 1 (Center = 181.0)
    let target_l1 = TabDndGeometry::calculate_target_index(&slots, 3, 170.0, 3);
    assert_eq!(target_l1, 1);
}

#[test]
fn test_tab_dnd_geometry_displacement_push_away_shifts() {
    let tab_ids = vec![1, 2, 3, 4];
    let widths = vec![100.0, 100.0, 100.0, 100.0];
    let spacing = 4.0;
    let slots = TabDndGeometry::compute_tab_slots(&tab_ids, &widths, spacing, 0.0);

    // Scenario A: Drag Tab 0 -> Slot 2 (Moving Right across slots 1 and 2)
    // Dragged item total footprint = 100.0 + 4.0 = 104.0
    // Tabs 1 and 2 must slide LEFT (-104.0px), Tab 0 stays 0.0, Tab 3 stays 0.0
    let shifts_right = TabDndGeometry::compute_shift_offsets(&slots, 0, 2, spacing);
    assert_eq!(shifts_right[0], 0.0);
    assert_eq!(shifts_right[1], -104.0);
    assert_eq!(shifts_right[2], -104.0);
    assert_eq!(shifts_right[3], 0.0);

    // Scenario B: Drag Tab 3 -> Slot 1 (Moving Left across slots 2 and 1)
    // Tabs 1 and 2 must slide RIGHT (+104.0px), Tab 0 stays 0.0, Tab 3 stays 0.0
    let shifts_left = TabDndGeometry::compute_shift_offsets(&slots, 3, 1, spacing);
    assert_eq!(shifts_left[0], 0.0);
    assert_eq!(shifts_left[1], 104.0);
    assert_eq!(shifts_left[2], 104.0);
    assert_eq!(shifts_left[3], 0.0);

    // Scenario C: Drag Tab 1 -> Slot 1 (Same slot) -> All shifts must be 0.0
    let shifts_same = TabDndGeometry::compute_shift_offsets(&slots, 1, 1, spacing);
    assert_eq!(shifts_same, vec![0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn test_tab_dnd_session_lifecycle() {
    let tab_ids = vec![100, 200, 300];
    let widths = vec![120.0, 140.0, 110.0];
    let slots = TabDndGeometry::compute_tab_slots(&tab_ids, &widths, 4.0, 2.0);

    let mut session = TabDndSession::new(200, 1, 50.0, 176.0, slots);
    assert!(session.is_active);
    assert_eq!(session.dragged_tab_id, 200);
    assert_eq!(session.source_index, 1);
    assert_eq!(session.target_index, 1);
    assert_eq!(session.ghost_left_x(), 126.0);
    assert_eq!(session.dragged_center_x(), 196.0);

    session.current_cursor_x = 300.0;
    assert_eq!(session.ghost_left_x(), 250.0);
}

#[test]
fn test_tab_dnd_autoscroller_lifecycle() {
    let scroller = TabDndAutoScroller::new();
    assert!(!scroller.is_active());

    // Cursor inside neutral zone
    scroller.update_cursor(150.0, 500.0, |_dx| {});
    assert!(!scroller.is_active());

    // Cursor inside left edge zone (< 36px)
    scroller.update_cursor(10.0, 500.0, |_dx| {});
    assert!(scroller.is_active());

    scroller.stop();
    assert!(!scroller.is_active());
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
fn test_tab_slot_geometry_lookup_and_width_estimation() {
    let t1 = create_test_session(1, 100, "short.png", "short.png");
    let t2 = create_test_session(2, 200, "a_much_longer_tab_title_name.png", "a_much_longer_tab_title_name.png");
    let t3 = create_test_session(3, 300, "photo.jpg", "photo.jpg");

    let tabs = vec![t1, t2, t3];

    // Verify slot geometry computation for first, middle, and last tab
    let slot0 = TabDndGeometry::find_tab_slot_geometry(&tabs, 1, false).expect("Slot 0 found");
    assert_eq!(slot0.0, 2.0); // starts at 2px
    assert!(slot0.1 >= 70.0);

    let slot1 = TabDndGeometry::find_tab_slot_geometry(&tabs, 2, false).expect("Slot 1 found");
    assert_eq!(slot1.0, 2.0 + slot0.1 + 4.0); // starts after slot0 + 4px spacing
    assert!(slot1.1 > slot0.1); // longer title is wider

    let slot2 = TabDndGeometry::find_tab_slot_geometry(&tabs, 3, false).expect("Slot 2 found");
    assert_eq!(slot2.0, slot1.0 + slot1.1 + 4.0);

    // Non-existent tab ID returns None
    assert!(TabDndGeometry::find_tab_slot_geometry(&tabs, 999, false).is_none());
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
fn test_tab_collection_batch_close_methods() {
    let mut collection = TabCollection::new();

    let t1 = create_test_session(1, 10, "1.png", "1.png");
    let t2 = create_test_session(2, 20, "2.png", "2.png");
    let t3 = create_test_session(3, 30, "3.png", "3.png");
    let t4 = create_test_session(4, 40, "4.png", "4.png");
    let t5 = create_test_session(5, 50, "5.png", "5.png");

    collection.push(t1);
    collection.push(t2);
    collection.push(t3);
    collection.push(t4);
    collection.push(t5);
    assert_eq!(collection.len(), 5);

    // 1. Remove to left of Tab 3 -> Should remove Tab 1 and Tab 2
    let left_evicted = collection.remove_to_left(3);
    assert_eq!(left_evicted.len(), 2);
    assert_eq!(left_evicted[0].id, 1);
    assert_eq!(left_evicted[1].id, 2);
    assert_eq!(collection.len(), 3); // [3, 4, 5]
    assert_eq!(collection.get_at(0).unwrap().id, 3);
    assert_eq!(collection.get_at(1).unwrap().id, 4);
    assert_eq!(collection.get_at(2).unwrap().id, 5);

    // Remove to left of first tab -> Should return empty
    let empty_left = collection.remove_to_left(3);
    assert!(empty_left.is_empty());
    assert_eq!(collection.len(), 3);

    // 2. Remove to right of Tab 4 -> Should remove Tab 5
    let right_evicted = collection.remove_to_right(4);
    assert_eq!(right_evicted.len(), 1);
    assert_eq!(right_evicted[0].id, 5);
    assert_eq!(collection.len(), 2); // [3, 4]
    assert_eq!(collection.get_at(0).unwrap().id, 3);
    assert_eq!(collection.get_at(1).unwrap().id, 4);

    // Remove to right of last tab -> Should return empty
    let empty_right = collection.remove_to_right(4);
    assert!(empty_right.is_empty());
    assert_eq!(collection.len(), 2);

    // 3. Remove other tabs keeping Tab 4 -> Should remove Tab 3
    let other_evicted = collection.retain_only(4);
    assert_eq!(other_evicted.len(), 1);
    assert_eq!(other_evicted[0].id, 3);
    assert_eq!(collection.len(), 1);
    assert_eq!(collection.get_at(0).unwrap().id, 4);
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
fn test_viewer_tab_manager_batch_close_methods() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let mut media_ids = Vec::new();
    for i in 1..=5 {
        let p = tmp_media.path().join(format!("test_img_{}.png", i));
        image::RgbImage::new(10, 10).save(&p).unwrap();
    }

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    for m in &media {
        media_ids.push(m.id);
    }
    assert_eq!(media_ids.len(), 5);

    let tab_mgr = ViewerTabManager::new(lib.clone());
    let mut tab_ids = Vec::new();

    for &mid in &media_ids {
        let (tid, _) = tab_mgr.open_or_focus_media(mid, true).unwrap();
        tab_ids.push(tid);
    }
    assert_eq!(tab_mgr.tab_count(), 5);

    // 1. Close tabs to the left of Tab 3 (index 2) -> Should close Tab 1 & Tab 2
    let (closed_left, active_after_left) = tab_mgr.close_tabs_to_left(tab_ids[2]);
    assert_eq!(closed_left, 2);
    assert_eq!(tab_mgr.tab_count(), 3);
    assert_eq!(active_after_left, Some(tab_ids[4])); // Active tab was Tab 5 (not closed), so Tab 5 remains active
    assert!(tab_mgr.get_tab(tab_ids[0]).is_none());
    assert!(tab_mgr.get_tab(tab_ids[1]).is_none());

    // 2. Close tabs to the right of Tab 3 -> Should close Tab 4 and Tab 5
    let (closed_right, active_after_right) = tab_mgr.close_tabs_to_right(tab_ids[2]);
    assert_eq!(closed_right, 2);
    assert_eq!(tab_mgr.tab_count(), 1);
    // Active tab was Tab 5 (which was closed) -> Target Tab 3 becomes the new active tab!
    assert_eq!(active_after_right, Some(tab_ids[2]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[2]));
    assert!(tab_mgr.get_tab(tab_ids[3]).is_none());
    assert!(tab_mgr.get_tab(tab_ids[4]).is_none());

    // Reopen additional tabs
    let (t_new1, _) = tab_mgr.open_or_focus_media(media_ids[0], true).unwrap();
    let (t_new2, _) = tab_mgr.open_or_focus_media(media_ids[1], true).unwrap();
    assert_eq!(tab_mgr.tab_count(), 3); // [Tab 3, t_new1, t_new2]

    // 3. Close other tabs keeping t_new1 -> Should close Tab 3 and t_new2
    let (closed_others, active_others) = tab_mgr.close_other_tabs(t_new1);
    assert_eq!(closed_others, 2);
    assert_eq!(tab_mgr.tab_count(), 1);
    assert_eq!(active_others, Some(t_new1));
    assert_eq!(tab_mgr.active_tab_id(), Some(t_new1));
    assert!(tab_mgr.get_tab(tab_ids[2]).is_none());
    assert!(tab_mgr.get_tab(t_new2).is_none());
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
fn test_viewer_tab_cycling_with_mouse_wheel_and_wrapping() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    for i in 1..=4 {
        let p = tmp_media.path().join(format!("item_{}.png", i));
        image::RgbImage::new(10, 10).save(&p).unwrap();
    }

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();
    assert_eq!(media.len(), 4);

    let tab_mgr = ViewerTabManager::new(lib);
    let mut tab_ids = Vec::new();

    for m in &media {
        let (tid, _) = tab_mgr.open_or_focus_media(m.id, true).unwrap();
        tab_ids.push(tid);
    }
    assert_eq!(tab_mgr.tab_count(), 4);

    // Initial state: Tab 4 (index 3) is active
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[3]));

    // 1. Wheel DOWN on Tab 4 (last tab, index 3) -> Wrap around to Tab 1 (index 0)
    let cycled_next_wrap = tab_mgr.cycle_tab(true);
    assert_eq!(cycled_next_wrap, Some(tab_ids[0]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[0]));

    // 2. Wheel DOWN on Tab 1 (index 0) -> Tab 2 (index 1)
    let cycled_next_1 = tab_mgr.cycle_tab(true);
    assert_eq!(cycled_next_1, Some(tab_ids[1]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[1]));

    // 3. Wheel DOWN on Tab 2 (index 1) -> Tab 3 (index 2)
    let cycled_next_2 = tab_mgr.cycle_tab(true);
    assert_eq!(cycled_next_2, Some(tab_ids[2]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[2]));

    // 4. Wheel UP on Tab 3 (index 2) -> Tab 2 (index 1)
    let cycled_prev_1 = tab_mgr.cycle_tab(false);
    assert_eq!(cycled_prev_1, Some(tab_ids[1]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[1]));

    // 5. Wheel UP on Tab 2 (index 1) -> Tab 1 (index 0)
    let cycled_prev_2 = tab_mgr.cycle_tab(false);
    assert_eq!(cycled_prev_2, Some(tab_ids[0]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[0]));

    // 6. Wheel UP on Tab 1 (first tab, index 0) -> Wrap around to Tab 4 (last tab, index 3)
    let cycled_prev_wrap = tab_mgr.cycle_tab(false);
    assert_eq!(cycled_prev_wrap, Some(tab_ids[3]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[3]));

    // 7. Test Browse Mode (no tab active):
    tab_mgr.activate_browse();
    assert_eq!(tab_mgr.active_tab_id(), None);

    // Wheel DOWN in Browse Mode -> Selects first tab (index 0)
    let browse_next = tab_mgr.cycle_tab(true);
    assert_eq!(browse_next, Some(tab_ids[0]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[0]));

    tab_mgr.activate_browse();
    // Wheel UP in Browse Mode -> Selects last tab (index 3)
    let browse_prev = tab_mgr.cycle_tab(false);
    assert_eq!(browse_prev, Some(tab_ids[3]));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab_ids[3]));

    // 8. Test Single Tab: Feature is inactive and returns None
    tab_mgr.close_other_tabs(tab_ids[0]);
    assert_eq!(tab_mgr.tab_count(), 1);

    let single_next = tab_mgr.cycle_tab(true);
    assert_eq!(single_next, None);

    let single_prev = tab_mgr.cycle_tab(false);
    assert_eq!(single_prev, None);
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

    assert!(tab_mgr.move_to_end(t2));
    assert_eq!(tab_mgr.get_tab_index(t2), Some(3));
    assert!(!tab_mgr.move_to_end(t2));
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

    // 1. Middle Tab (index 1 of 3): Move Left/Right enabled; Close Left/Right/Others all enabled
    let menu_middle = TabContextMenu::build_menu(&tabs[1], 1, 3, &tabs);

    assert_eq!(menu_middle.items[0].text, "Tab: photo_test.png (1)");
    assert_eq!(menu_middle.items[1].id, CTX_TAB_DUPLICATE);
    assert_eq!(menu_middle.items[1].text, "Duplicate Tab");
    assert!(menu_middle.items[1].is_enabled);
    assert!(menu_middle.items[1].icon_name.is_none());

    // Move Tab Submenu
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

    // Close Tabs Submenu (Positioned directly above Close Tab, under existing separator)
    let close_tabs_sub = &menu_middle.items[5];
    assert_eq!(close_tabs_sub.text, "Close Tabs...");
    assert!(close_tabs_sub.is_enabled);
    assert_eq!(close_tabs_sub.children.len(), 3);

    assert_eq!(close_tabs_sub.children[0].id, CTX_TAB_CLOSE_TO_LEFT);
    assert_eq!(close_tabs_sub.children[0].text, "Left of selected tab");
    assert!(close_tabs_sub.children[0].is_enabled);

    assert_eq!(close_tabs_sub.children[1].id, CTX_TAB_CLOSE_TO_RIGHT);
    assert_eq!(close_tabs_sub.children[1].text, "Right of selected tab");
    assert!(close_tabs_sub.children[1].is_enabled);

    assert_eq!(close_tabs_sub.children[2].id, CTX_TAB_CLOSE_OTHERS);
    assert_eq!(close_tabs_sub.children[2].text, "All other tabs");
    assert!(close_tabs_sub.children[2].is_enabled);

    // Close Tab Action
    assert_eq!(menu_middle.items[6].id, CTX_TAB_CLOSE);
    assert_eq!(menu_middle.items[6].text, "Close Tab");
    assert_eq!(menu_middle.items[6].shortcut, Some("Ctrl+W".to_string()));
    assert!(menu_middle.items[6].is_enabled);

    // 2. First Tab (index 0 of 3): Close Left disabled, Close Right enabled
    let menu_first = TabContextMenu::build_menu(&tabs[0], 0, 3, &tabs);
    let close_sub_first = &menu_first.items[5];
    assert!(!close_sub_first.children[0].is_enabled); // Left disabled
    assert!(close_sub_first.children[1].is_enabled);  // Right enabled
    assert!(close_sub_first.children[2].is_enabled);  // Others enabled

    // 3. Last Tab (index 2 of 3): Close Left enabled, Close Right disabled
    let menu_last = TabContextMenu::build_menu(&tabs[2], 2, 3, &tabs);
    let close_sub_last = &menu_last.items[5];
    assert!(close_sub_last.children[0].is_enabled);  // Left enabled
    assert!(!close_sub_last.children[1].is_enabled); // Right disabled
    assert!(close_sub_last.children[2].is_enabled);  // Others enabled

    // 4. Single Tab (index 0 of 1): Entire "Close Tabs..." submenu must be DISABLED
    let menu_single = TabContextMenu::build_menu(&tabs[0], 0, 1, &tabs[0..1]);
    let close_sub_single = &menu_single.items[5];
    assert!(!close_sub_single.is_enabled);
    assert!(!close_sub_single.children[0].is_enabled);
    assert!(!close_sub_single.children[1].is_enabled);
    assert!(!close_sub_single.children[2].is_enabled);
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
