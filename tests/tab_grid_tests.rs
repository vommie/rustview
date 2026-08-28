use rustview::library::Library;
use rustview::settings::SettingsManager;
use rustview::ui::bridge_image_fullscreen::ImageFullscreenDialogController;
use rustview::ui::tab_grid::{
    ResponsiveBreakpoints, TabGridMasonryEngine, TabGridTile,
};
use rustview::viewer::{
    SubwindowGeometry, ViewportBounds, ViewerTabManager, X11VideoSubwindow,
};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_responsive_breakpoints_column_calculation() {
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(450.0), 1);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(619.0), 1);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(700.0), 2);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(950.0), 2);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(1100.0), 3);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(1440.0), 4);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(1920.0), 5);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(2560.0), 6);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(3000.0), 7);
    assert_eq!(ResponsiveBreakpoints::compute_auto_columns(3840.0), 8);
}

#[test]
fn test_tab_grid_tile_aspect_ratio_clamping() {
    // Standard 16:9
    let ar_16_9 = TabGridTile::compute_aspect_ratio(1920, 1080, true);
    assert!((ar_16_9 - (16.0 / 9.0)).abs() < 0.01);

    // Standard 4:3
    let ar_4_3 = TabGridTile::compute_aspect_ratio(800, 600, false);
    assert!((ar_4_3 - (4.0 / 3.0)).abs() < 0.01);

    // 0x0 fallback for video -> 16:9
    let ar_video_fallback = TabGridTile::compute_aspect_ratio(0, 0, true);
    assert!((ar_video_fallback - (16.0 / 9.0)).abs() < 0.01);

    // 0x0 fallback for image -> 4:3
    let ar_image_fallback = TabGridTile::compute_aspect_ratio(0, 0, false);
    assert!((ar_image_fallback - (4.0 / 3.0)).abs() < 0.01);

    // Extreme tall portrait -> clamped to 0.28
    let ar_tall = TabGridTile::compute_aspect_ratio(10, 1000, false);
    assert_eq!(ar_tall, 0.28);

    // Extreme wide panorama -> clamped to 3.5
    let ar_wide = TabGridTile::compute_aspect_ratio(10000, 500, false);
    assert_eq!(ar_wide, 3.5);
}

#[test]
fn test_tab_grid_masonry_layout_greedy_shortest_column_assignment() {
    // 4 items:
    // Item 0: 1.0 (Square)
    // Item 1: 2.0 (Wide)
    // Item 2: 0.5 (Tall)
    // Item 3: 1.0 (Square)
    let aspect_ratios = vec![1.0, 2.0, 0.5, 1.0];

    let viewport_w = 900.0;
    let num_cols = 2;
    let grid_padding = 10.0;
    let grid_spacing = 10.0;
    let item_padding_v = 6.0;
    let header_height = 26.0;

    let (rects, total_content_height) = TabGridMasonryEngine::compute_layout(
        &aspect_ratios,
        viewport_w,
        num_cols,
        grid_padding,
        grid_spacing,
        item_padding_v,
        header_height,
    );

    assert_eq!(rects.len(), 4);

    // Item 0 -> Placed in Column 0 at y = 10.0
    assert_eq!(rects[0].x, 10.0);
    assert_eq!(rects[0].y, 10.0);
    assert_eq!(rects[0].width, 435.0);
    assert_eq!(rects[0].height, 473.0);

    // Item 1 -> Placed in Column 1 at y = 10.0
    assert_eq!(rects[1].x, 10.0 + 435.0 + 10.0);
    assert_eq!(rects[1].y, 10.0);
    assert_eq!(rects[1].width, 435.0);
    assert_eq!(rects[1].height, 256.0);

    // Item 2 -> Greedily placed in Column 1 (shorter) at y = 10 + 266 = 276.0
    assert_eq!(rects[2].x, 455.0);
    assert_eq!(rects[2].y, 276.0);
    assert_eq!(rects[2].width, 435.0);
    assert_eq!(rects[2].height, 908.0);

    // Item 3 -> Greedily placed in Column 0 (now shorter) at y = 10 + 483 = 493.0
    assert_eq!(rects[3].x, 10.0);
    assert_eq!(rects[3].y, 493.0);
    assert_eq!(rects[3].width, 435.0);
    assert_eq!(rects[3].height, 473.0);

    assert!(total_content_height > 1184.0);
}

#[test]
fn test_tab_grid_empty_aspect_ratios_returns_zero_height() {
    let (rects, total_h) = TabGridMasonryEngine::compute_layout(
        &[],
        1000.0,
        3,
        12.0,
        12.0,
        6.0,
        26.0,
    );
    assert!(rects.is_empty());
    assert_eq!(total_h, 0.0);
}

#[test]
fn test_tab_manager_set_all_videos_view_active_and_hide_subwindows() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let vid_p = tmp_media.path().join("clip.mp4");
    std::fs::write(&vid_p, b"video data").unwrap();
    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap()[0].clone();

    let tab_mgr = ViewerTabManager::new(lib);
    let (t1, _) = tab_mgr.open_or_focus_media(media.id, true).unwrap();
    let (t2, _) = tab_mgr.open_or_focus_media(media.id, true).unwrap();

    let player1 = tab_mgr.get_tab(t1).unwrap().payload;
    let player2 = tab_mgr.get_tab(t2).unwrap().payload;

    if let (
        rustview::viewer::TabPayload::Video(ref v1),
        rustview::viewer::TabPayload::Video(ref v2),
    ) = (player1, player2)
    {
        // Deactivate all videos (as done when leaving TabGrid mode or entering browse mode)
        tab_mgr.set_all_videos_view_active(false);
        assert!(!v1.player.is_view_active());
        assert!(!v2.player.is_view_active());

        // Reactivate
        tab_mgr.set_all_videos_view_active(true);
        assert!(v1.player.is_view_active());
        assert!(v2.player.is_view_active());

        // Test hide_all_video_subwindows
        tab_mgr.hide_all_video_subwindows();
        assert!(!v1.player.is_view_active());
        assert!(!v2.player.is_view_active());

        // Test hide_video_subwindows_except
        tab_mgr.hide_video_subwindows_except(Some(t1));
        assert!(!v2.player.is_view_active());
    } else {
        panic!("Expected video payloads");
    }
}

#[test]
fn test_fullscreen_controller_show_for_specific_tab() {
    let tmp_db = tempdir().unwrap();
    let tmp_media = tempdir().unwrap();
    let tmp_config = tempdir().unwrap();

    let lib = Arc::new(Library::open(tmp_db.path()).unwrap());
    let folder = lib.folders().add_folder(tmp_media.path()).unwrap();

    let img1_p = tmp_media.path().join("img1.png");
    let img2_p = tmp_media.path().join("img2.png");
    image::RgbImage::new(100, 100).save(&img1_p).unwrap();
    image::RgbImage::new(200, 200).save(&img2_p).unwrap();

    let _ = lib.refresher().refresh_folder(folder.id).unwrap();
    let media = lib.media().get_by_folder(folder.id).unwrap();

    let tab_mgr = Arc::new(ViewerTabManager::new(lib));
    let _settings = SettingsManager::new(tmp_config.path());

    let (_t1, _) = tab_mgr.open_or_focus_media(media[0].id, false).unwrap();
    let (t2, _) = tab_mgr.open_or_focus_media(media[1].id, false).unwrap();

    assert_eq!(tab_mgr.active_tab_id(), Some(t2));

    let fs_ctrl = ImageFullscreenDialogController::new();

    // Closing when not active is a safe no-op
    fs_ctrl.close();
    assert!(!fs_ctrl.is_open());

    assert_eq!(tab_mgr.active_tab_id(), Some(t2));
}

#[test]
fn test_subwindow_viewport_clipping_and_geometry_updates() {
    let root_window_res = rustview::viewer::find_window_by_title("RustView");
    let target_parent = root_window_res.unwrap_or(0);

    if target_parent > 0
        && let Ok(mut sub) = X11VideoSubwindow::new(target_parent, 100, 100, 400, 300)
    {
        let viewport = ViewportBounds {
            x: 0,
            y: 64,
            width: 1200,
            height: 700,
        };

        // 1. Fully inside viewport (view = 0, 64, 1200, 700)
        let inside = sub.update_geometry_and_clip(
            SubwindowGeometry {
                x: 100,
                y: 100,
                width: 400,
                height: 300,
            },
            viewport,
        );
        assert!(inside);
        assert!(sub.is_visible());

        // 2. Partially above top boundary (phys_y = 20, view_top = 64, height = 300) -> clamped to y=64, height=256
        let partial_top = sub.update_geometry_and_clip(
            SubwindowGeometry {
                x: 100,
                y: 20,
                width: 400,
                height: 300,
            },
            viewport,
        );
        assert!(partial_top);
        assert!(sub.is_visible());

        // 3. Completely above top boundary (phys_y = -350, height = 300, view_top = 64) -> hidden
        let outside_top = sub.update_geometry_and_clip(
            SubwindowGeometry {
                x: 100,
                y: -350,
                width: 400,
                height: 300,
            },
            viewport,
        );
        assert!(!outside_top);
        assert!(!sub.is_visible());

        // 4. Completely below bottom boundary (phys_y = 900, view_bottom = 764) -> hidden
        let outside_bottom = sub.update_geometry_and_clip(
            SubwindowGeometry {
                x: 100,
                y: 900,
                width: 400,
                height: 300,
            },
            viewport,
        );
        assert!(!outside_bottom);
        assert!(!sub.is_visible());
    }
}
