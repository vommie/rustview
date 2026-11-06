//! Writes what the Settings window shows into the settings. Since 0.6.14 every change applies at once (`RustCut`'s
//! "Changes are saved at once."): the window calls this a moment after an edit and when it closes.

#[allow(
    clippy::wildcard_imports,
    reason = "the page helpers and constants of the settings window module"
)]
use super::*;

/// What applying needs besides the window: the settings and the pages that keep state of their own.
#[derive(Clone)]
pub(super) struct ApplyContext {
    pub mgr: Arc<SettingsManager>,
    pub formats: Arc<FormatsPage>,
    pub hotkeys: Rc<HotkeysPageController>,
    pub monitors: Rc<RefCell<Vec<MonitorInfo>>>,
    pub debug: Rc<DebugPageController>,
}

impl SettingsDialogController {
    /// Reads every page of `w` into the settings and saves them (the settings subscribers apply the changes).
    pub(super) fn apply_from_window(w: &SettingsWindow, ctx: &ApplyContext) {
        let mgr_save = &ctx.mgr;
        let formats_page_save = &ctx.formats;
        let hotkeys_page_save = &ctx.hotkeys;
        let detected_monitors_save = &ctx.monitors;
        let mut current = mgr_save.get();

        let root_str = w.get_root_folder_path();
        current.general.root_folder = if root_str.is_empty() {
            None
        } else {
            Some(PathBuf::from(root_str.as_str()))
        };

        current.general.remember_last_folder = w.get_remember_last_folder();
        current.general.confirm_on_file_delete = w.get_confirm_delete();
        current.general.confirm_open_file_count = w
            .get_confirm_open_file_count()
            .clamp(
                MIN_CONFIRM_OPEN_FILE_COUNT.saturating_as::<i32>(),
                MAX_CONFIRM_OPEN_FILE_COUNT.saturating_as::<i32>(),
            )
            .saturating_as::<u32>();
        current.general.ignore_nested_mounts = w.get_ignore_nested_mounts();
        confirmations::save(w, &mut current.general);

        current.folders.auto_reveal_remembered_folder = w.get_auto_reveal_remembered_folder();
        current.folders.persist_folder_tree_state = w.get_persist_folder_tree_state();
        current.folders.show_deleted_folders = w.get_show_deleted_folders();
        current.folders.show_folder_metrics = w.get_show_folder_metrics();
        current.folders.show_hidden_folders = w.get_show_hidden_folders();
        current.folders.support_dot_hidden_files = w.get_support_dot_hidden_files();

        current.appearance.default_thumbnail_size = w.get_default_thumb_size().saturating_as::<u32>();
        let ratio = GridAspectRatio::from_index(w.get_thumb_aspect_idx());
        current.appearance.thumbnail_aspect_ratio = ratio.as_str().to_string();
        current.appearance.show_filenames = w.get_show_filenames();
        current.appearance.show_ratings_overlay = w.get_show_rating_badge();
        current.appearance.show_tags_overlay = w.get_show_tag_badge();
        current.appearance.show_media_type_badges = w.get_show_media_type_badges();
        current.appearance.show_duration_badge = w.get_show_duration_badge();
        current.appearance.show_animated_badge = w.get_show_animated_badge();
        current.appearance.thumbnail_badge_overflow =
            ThumbnailBadgeOverflow::from_index(w.get_thumbnail_badge_overflow_idx())
                .as_str()
                .to_string();
        current.appearance.enable_hover_previews = w.get_enable_hover_previews();
        current.appearance.enable_tooltips = w.get_enable_tooltips();
        let theme = rustview_core::settings::theme::ThemeMode::from_index(
            usize::try_from(w.get_appearance_theme_idx()).unwrap_or(0),
        );
        theme.name().clone_into(&mut current.appearance.theme);
        current.appearance.tooltip_delay_ms = w.get_tooltip_delay_ms().clamp(50, 2000).saturating_as::<u32>();
        current.appearance.type_ahead_enabled = w.get_type_ahead_enabled();
        current.appearance.type_ahead_timeout_ms = w
            .get_type_ahead_timeout_ms()
            .clamp(
                MIN_TYPE_AHEAD_TIMEOUT_MS.saturating_as::<i32>(),
                MAX_TYPE_AHEAD_TIMEOUT_MS.saturating_as::<i32>(),
            )
            .saturating_as::<u32>();
        current.appearance.type_ahead_new_search_gap_ms = (w
            .get_type_ahead_new_search_gap_ms()
            .clamp(0, MAX_TYPE_AHEAD_NEW_SEARCH_GAP_MS.saturating_as::<i32>()))
        .saturating_as::<u32>();
        current.appearance.type_ahead_include_folders = w.get_type_ahead_include_folders();
        current.appearance.type_ahead_cycle_repeated_keys = w.get_type_ahead_cycle_repeated_keys();
        current.appearance.type_ahead_center_target = w.get_type_ahead_center_target();
        current.appearance.type_ahead_show_no_match = w.get_type_ahead_show_no_match();
        current.appearance.type_ahead_multiview_delay_ms = (w
            .get_type_ahead_multiview_delay_ms()
            .clamp(0, MAX_TYPE_AHEAD_MULTIVIEW_DELAY_MS.saturating_as::<i32>()))
        .saturating_as::<u32>();

        // Save Grid Items Layout Properties
        current.appearance.grid_item_padding_horizontal =
            w.get_grid_item_padding_h().clamp(0, 32).saturating_as::<u32>();
        current.appearance.grid_item_padding_vertical = w.get_grid_item_padding_v().clamp(0, 32).saturating_as::<u32>();
        current.appearance.grid_spacing = w.get_grid_spacing().clamp(0, 48).saturating_as::<u32>();
        current.appearance.grid_padding = w.get_grid_padding().clamp(0, 48).saturating_as::<u32>();
        current.appearance.grid_item_border_width = w.get_grid_item_border_width().clamp(0, 6).saturating_as::<u32>();
        current.appearance.grid_item_corner_radius =
            w.get_grid_item_corner_radius().clamp(0, 24).saturating_as::<u32>();

        // Save Performance Parameters
        current.performance.worker_threads = w.get_worker_threads().clamp(1, 32).saturating_as::<usize>();
        current.performance.batch_commit_size = w.get_batch_commit_size().clamp(4, 256).saturating_as::<usize>();
        current.performance.memory_budget_mb = if w.get_memory_budget_auto() {
            0
        } else {
            w.get_memory_budget_mb()
                .clamp(
                    MIN_MEMORY_BUDGET_MB.saturating_as::<i32>(),
                    MAX_MEMORY_BUDGET_MB.saturating_as::<i32>(),
                )
                .saturating_as::<u32>()
        };
        current.performance.thumbnail_db_size = w.get_thumb_db_size().clamp(64, 1024).saturating_as::<u32>();
        current.performance.thumbnail_format = if w.get_thumb_format_idx() == 1 {
            "JPEG".to_string()
        } else {
            "WebP".to_string()
        };
        current.performance.thumbnail_quality = w.get_thumb_quality().clamp(10, 100).saturating_as::<u8>();
        current.performance.scaling_algorithm = match w.get_scaling_algo_idx() {
            1 => "Bilinear".to_string(),
            2 => "Lanczos3".to_string(),
            3 => "Nearest".to_string(),
            _ => "CatmullRom".to_string(),
        };
        current.performance.video_engine = if w.get_video_engine_idx() == 1 {
            "MPV".to_string()
        } else {
            "FFmpeg".to_string()
        };
        current.performance.video_offset_percent = w.get_video_offset_percent().clamp(1, 90).saturating_as::<u8>();
        current.performance.short_video_threshold_secs =
            w.get_short_video_threshold_secs().clamp(1, 60).saturating_as::<u32>();
        current.performance.generate_video_previews = w.get_generate_video_previews();
        current.performance.preview_frame_count = w.get_preview_frame_count().clamp(2, 100).saturating_as::<u32>();
        current.performance.custom_preview_settings = w.get_custom_preview_settings();
        current.performance.preview_format = if w.get_preview_format_idx() == 1 {
            "JPEG".to_string()
        } else {
            "WebP".to_string()
        };
        current.performance.preview_quality = w.get_preview_quality().clamp(10, 100).saturating_as::<u8>();

        // Save Formats Parameters
        current.formats.auto_detect_extensionless_media = w.get_auto_detect_extensionless();
        current.formats.disabled_formats = formats_page_save.disabled_ids();
        hotkeys_page_save.save(&mut current);

        // Save Viewer Parameters
        current.viewer.images.open_in_tab = w.get_open_images_in_tab();
        current.viewer.images.allow_multiple_tabs = w.get_allow_multiple_image_tabs();
        current.viewer.videos.open_in_tab = w.get_open_videos_in_tab();
        current.viewer.videos.allow_multiple_tabs = w.get_allow_multiple_video_tabs();
        let jump = |secs: i32| {
            secs.clamp(
                MIN_VIEWER_VIDEO_JUMP_SECS.saturating_as::<i32>(),
                MAX_VIEWER_VIDEO_JUMP_SECS.saturating_as::<i32>(),
            )
            .saturating_as::<u32>()
        };
        current.viewer.videos.jump_secs = jump(w.get_video_jump_secs());
        current.viewer.videos.jump_long_secs = jump(w.get_video_jump_long_secs());
        ab_repeat_card::save(w, &mut current.viewer.videos.ab_repeat);
        current.viewer.tabs.show_rating_badge = w.get_viewer_show_tab_rating_badge();
        let close_mode = usize::try_from(w.get_viewer_tab_close_buttons_idx()).unwrap_or(0);
        if let Some(mode) = VIEWER_TAB_CLOSE_BUTTON_MODES.get(close_mode) {
            (*mode).clone_into(&mut current.viewer.tabs.close_buttons);
        }
        current.viewer.background_mode = match w.get_viewer_background_mode_idx() {
            1 => "Deep Black".to_string(),
            2 => "Transparent Checkerboard".to_string(),
            _ => "Dark".to_string(),
        };

        // Save Fullscreen & Target Display Parameters
        current.fullscreen.enable_specific_monitor = w.get_fullscreen_enable_specific();
        let sel_disp_idx = w.get_selected_display_idx().saturating_as::<usize>();
        let monitors_guard = detected_monitors_save.borrow();
        if let Some(m) = monitors_guard.get(sel_disp_idx) {
            current.fullscreen.specific_monitor.clone_from(&m.connector);
        }
        drop(monitors_guard);

        current.fullscreen.enable_fallback_rule = w.get_fullscreen_enable_fallback();
        current.fullscreen.fallback_target = if w.get_fullscreen_fallback_target_idx() == 1 {
            "different".to_string()
        } else {
            "same".to_string()
        };
        Self::save_fullscreen_extras(w, &mut current.fullscreen);
        image_edit_page::save(w, &mut current.image_edit);

        // Save Tag History Configuration
        let new_max_history = w
            .get_max_history_tags()
            .saturating_as::<usize>()
            .clamp(MIN_MAX_HISTORY_TAGS, MAX_MAX_HISTORY_TAGS);
        current.tags.max_history_tags = new_max_history;
        current.tags.history_tag_ids.truncate(new_max_history);
        current.tags.compact_tree = w.get_tag_tree_compact();

        // Save API Parameters
        current.api.auto_complete_external_stubs = w.get_auto_complete_external_stubs();

        // Save Debug Parameters
        current.debug.obfuscate_logs = w.get_obfuscate_logs();
        DebugPageController::save(w, &mut current);

        mgr_save.update(&current);
    }
}
