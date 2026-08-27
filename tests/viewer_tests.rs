use rustview::library::Library;
use rustview::viewer::{
    find_window_by_pid_and_title, find_window_by_title, HighResImageLoader, MpvPlayer, TabPayload,
    ViewerAction, ViewerError, ViewerTabManager, MAX_CACHED_HIGH_RES_BUFFERS,
};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::sync::Arc;
use tempfile::tempdir;

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

    // Test Volume API
    let _ = player.set_volume(85.0);
    assert_eq!(player.get_volume().round() as i32, 85);

    // Test Pause / Play API
    assert!(player.pause().is_ok());
    assert!(player.play().is_ok());

    // Test Seek API (No crash when idle)
    assert!(player.seek_relative(5.0).is_ok());
    assert!(player.seek_absolute(10.0).is_ok());

    player.stop();
    let after_stop = player.get_playback_state();
    assert!(!after_stop.is_playing);
}

#[test]
fn test_mpv_player_user_pause_vs_auto_pause_semantics() {
    let player = MpvPlayer::new().unwrap();

    // 1. Initial state: active view, not user paused
    assert!(player.is_view_active());
    assert!(!player.is_user_paused());

    // 2. User explicitly pauses the active video
    player.pause().unwrap();
    assert!(player.is_user_paused());
    assert!(player.is_paused());

    // 3. Deactivate view (simulate user switching away to Browse mode)
    player.set_view_active(false);
    assert!(!player.is_view_active());
    assert!(player.is_user_paused());
    assert!(player.is_paused());

    // 4. Reactivate view (simulate user returning to this tab) -> MUST STAY PAUSED because user paused it!
    player.set_view_active(true);
    assert!(player.is_view_active());
    assert!(player.is_user_paused());
    assert!(player.is_paused());

    // 5. User explicitly resumes (plays) the video
    player.play().unwrap();
    assert!(!player.is_user_paused());

    // 6. Deactivate view while playing -> auto-pause takes effect
    player.set_view_active(false);
    assert!(!player.is_view_active());
    assert!(!player.is_user_paused(), "Auto-pause must not flag is_user_paused");
    assert!(player.is_paused(), "Background video must be auto-paused");

    // 7. Reactivate view -> MUST AUTO-RESUME because user had not paused it!
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

    // Initially in Browse mode
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert_eq!(tab_mgr.list_tabs().len(), 0);

    // 1. Open first media tab (allow_multiple = false)
    let (tab1_id, is_new1) = tab_mgr.open_or_focus_media(m1.id, false).unwrap();
    assert!(is_new1);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));
    assert_eq!(tab_mgr.list_tabs().len(), 1);

    // 2. Open second media tab (allow_multiple = false)
    let (tab2_id, is_new2) = tab_mgr.open_or_focus_media(m2.id, false).unwrap();
    assert!(is_new2);
    assert_ne!(tab1_id, tab2_id);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab2_id));
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    // 3. Re-open first media with allow_multiple = false -> MUST focus existing tab without creating duplicate
    let (focused_id, is_new_reopen) = tab_mgr.open_or_focus_media(m1.id, false).unwrap();
    assert!(!is_new_reopen);
    assert_eq!(focused_id, tab1_id);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    // 4. Test active tab queries
    let active_tab = tab_mgr.get_active_tab().unwrap();
    assert_eq!(active_tab.id, tab1_id);
    assert_eq!(active_tab.media_id, m1.id);
    assert_eq!(active_tab.filename, "photo1.png");
    assert_eq!(tab_mgr.get_active_media_id(), Some(m1.id));

    // 5. Switch to Browse view
    tab_mgr.activate_browse();
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    // 6. Switch directly to Tab 2
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

    // 1. Open first video tab with allow_multiple = true -> Tab 1 created
    let (tab1_id, is_new1) = tab_mgr.open_or_focus_media(vid_media.id, true).unwrap();
    assert!(is_new1);
    assert_eq!(tab_mgr.list_tabs().len(), 1);

    // 2. Open second video tab with allow_multiple = true -> Tab 2 created for SAME media_id!
    let (tab2_id, is_new2) = tab_mgr.open_or_focus_media(vid_media.id, true).unwrap();
    assert!(is_new2);
    assert_ne!(tab1_id, tab2_id);
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    // 3. Open third video tab with allow_multiple = true -> Tab 3 created
    let (tab3_id, is_new3) = tab_mgr.open_or_focus_media(vid_media.id, true).unwrap();
    assert!(is_new3);
    assert_ne!(tab2_id, tab3_id);
    assert_eq!(tab_mgr.list_tabs().len(), 3);

    // Verify Title Computation with Compact Iteration Suffixes
    let tabs = tab_mgr.list_tabs();
    let title1 = ViewerTabManager::compute_tab_title(&tabs, tab1_id);
    let title2 = ViewerTabManager::compute_tab_title(&tabs, tab2_id);
    let title3 = ViewerTabManager::compute_tab_title(&tabs, tab3_id);

    assert_eq!(title1, "clip_sample.mp4");
    assert_eq!(title2, "clip_sample.mp4 (1)");
    assert_eq!(title3, "clip_sample.mp4 (2)");

    // 4. Open Image tab with allow_multiple = false
    let (img_tab1_id, img_is_new1) = tab_mgr.open_or_focus_media(img_media.id, false).unwrap();
    assert!(img_is_new1);

    // Re-opening image with allow_multiple = false must reuse the tab without suffix
    let (img_tab_reuse_id, img_is_new2) = tab_mgr.open_or_focus_media(img_media.id, false).unwrap();
    assert!(!img_is_new2);
    assert_eq!(img_tab_reuse_id, img_tab1_id);

    let tabs_after_img = tab_mgr.list_tabs();
    let img_title = ViewerTabManager::compute_tab_title(&tabs_after_img, img_tab1_id);
    assert_eq!(img_title, "single_image.png");

    // 5. Close Tab 2 and verify remaining tabs update their iteration suffixes dynamically
    tab_mgr.close_tab(tab2_id);
    let tabs_remaining = tab_mgr.list_tabs();
    assert_eq!(tabs_remaining.len(), 3); // 2 video tabs + 1 image tab

    let title1_rem = ViewerTabManager::compute_tab_title(&tabs_remaining, tab1_id);
    let title3_rem = ViewerTabManager::compute_tab_title(&tabs_remaining, tab3_id);

    assert_eq!(title1_rem, "clip_sample.mp4");
    assert_eq!(title3_rem, "clip_sample.mp4 (1)");
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

    // 1. Open Video Tab 1
    let (tab1_id, is_new1) = tab_mgr.open_or_focus_media(media[0].id, false).unwrap();
    assert!(is_new1);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));

    let tab1_player = tab_mgr.get_active_player().expect("Tab 1 must have active MpvPlayer");

    // Set custom playback state in Tab 1
    let _ = tab1_player.set_volume(42.0);
    assert_eq!(tab1_player.get_volume().round() as i32, 42);

    // 2. Open Video Tab 2 (Tab 1 is auto-paused, Tab 2 gets its own isolated player)
    let (tab2_id, is_new2) = tab_mgr.open_or_focus_media(media[1].id, false).unwrap();
    assert!(is_new2);
    assert_eq!(tab_mgr.active_tab_id(), Some(tab2_id));

    let tab2_player = tab_mgr.get_active_player().expect("Tab 2 must have active MpvPlayer");
    let _ = tab2_player.set_volume(90.0);
    assert_eq!(tab2_player.get_volume().round() as i32, 90);

    // Verify Tab 1's state is preserved independently
    assert_eq!(tab1_player.get_volume().round() as i32, 42);

    // 3. Switch to Browse view (all tabs preserved in RAM)
    tab_mgr.activate_browse();
    assert_eq!(tab_mgr.active_tab_id(), None);
    assert_eq!(tab_mgr.list_tabs().len(), 2);
    assert_eq!(tab1_player.get_volume().round() as i32, 42);
    assert_eq!(tab2_player.get_volume().round() as i32, 90);

    // 4. Switch back to Tab 1
    assert!(tab_mgr.set_active_tab(tab1_id));
    assert_eq!(tab_mgr.active_tab_id(), Some(tab1_id));
    assert_eq!(tab_mgr.get_active_player().unwrap().get_volume().round() as i32, 42);

    // 5. Close Tab 1 (only Tab 1 is destroyed; Tab 2 becomes active and stays in RAM)
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

    // 1. Open tab for image 1 -> tab count = 1
    let (tab_id, _) = tab_mgr.open_or_focus_media(m1.id, false).unwrap();
    assert_eq!(tab_mgr.list_tabs().len(), 1);
    assert_eq!(tab_mgr.get_active_media_id(), Some(m1.id));

    // 2. Navigate in-place to image 2 -> tab count MUST REMAIN 1, same tab_id
    let nav_tab_id = tab_mgr.navigate_active_tab_to_media(m2.id).unwrap();
    assert_eq!(nav_tab_id, tab_id);
    assert_eq!(tab_mgr.list_tabs().len(), 1);
    assert_eq!(tab_mgr.get_active_media_id(), Some(m2.id));

    let active_tab = tab_mgr.get_active_tab().unwrap();
    assert_eq!(active_tab.filename, "second.png");
    assert_eq!(active_tab.media_id, m2.id);

    // 3. Close the tab -> closes in 1 single step and returns to Browse mode (None)
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

    // Close active tab 3 -> Should fall back to adjacent tab 2
    let new_active = tab_mgr.close_tab(t3);
    assert_eq!(new_active, Some(t2));
    assert_eq!(tab_mgr.active_tab_id(), Some(t2));
    assert_eq!(tab_mgr.list_tabs().len(), 2);

    // Close background tab 1 while tab 2 is active -> Active tab remains 2
    let after_t1_close = tab_mgr.close_tab(t1);
    assert_eq!(after_t1_close, Some(t2));
    assert_eq!(tab_mgr.active_tab_id(), Some(t2));
    assert_eq!(tab_mgr.list_tabs().len(), 1);

    // Close last remaining tab 2 -> Active tab becomes None (Browse mode)
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

    // 1. Test rename notification
    tab_mgr.notify_media_renamed(media.id, "renamed.png");
    let session_renamed = tab_mgr.get_tab(tab_id).unwrap();
    assert_eq!(session_renamed.filename, "renamed.png");
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

    // Initial load of 100x100 buffer
    let initial_res = HighResImageLoader::load_full_resolution(&img_path).unwrap();
    tab_mgr.update_tab_image_buffer(tab_id, initial_res.0, initial_res.1, initial_res.2);

    let session_init = tab_mgr.get_tab(tab_id).unwrap();
    if let TabPayload::Image(ref img) = session_init.payload {
        assert_eq!(img.orig_width, 100);
        assert_eq!(img.orig_height, 100);
        assert!(img.high_res_pixels.is_some());
    }

    // Overwrite file on disk
    let new_img = image::RgbImage::new(400, 300);
    new_img.save(&img_path).unwrap();

    // Re-load full resolution image buffer directly into the tab
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

    // 1. Decode valid PNG
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

    // 2. Non-existent file returns ViewerError::FileNotFound
    let missing_path = tmp.path().join("does_not_exist.jpg");
    let err_missing = HighResImageLoader::load_full_resolution(&missing_path);
    assert!(err_missing.is_err());
    match err_missing.unwrap_err() {
        ViewerError::FileNotFound(p) => assert_eq!(p, missing_path),
        other => panic!("Expected ViewerError::FileNotFound, got {:?}", other),
    }

    // 3. Corrupted file returns DecodeFailed
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
