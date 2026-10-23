mod formats_page;

pub use formats_page::FormatCounter;
use formats_page::FormatsPage;
use rustview_core::settings::memory_budget::{BYTES_PER_MB, MemoryPlan, SystemMemory};
use rustview_core::settings::registry::SettingsManager;
use rustview_core::settings::schema::{
    AppSettings, FullscreenSettings, MAX_CONFIRM_OPEN_FILE_COUNT, MIN_CONFIRM_OPEN_FILE_COUNT, MAX_MEMORY_BUDGET_MB, MIN_MEMORY_BUDGET_MB, GridAspectRatio, ThumbnailBadgeOverflow, DEFAULT_FULLSCREEN_BACKGROUND_COLOR,
    MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT, DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS, DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT, DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT, MAX_FULLSCREEN_ZOOM_MAX_PERCENT,
    MAX_FULLSCREEN_ZOOM_MINIMAP_SECS, MAX_FULLSCREEN_ZOOM_OUT_PERCENT, MAX_FULLSCREEN_ZOOM_STEP_PERCENT, MIN_FULLSCREEN_ZOOM_MAX_PERCENT,
    MIN_FULLSCREEN_ZOOM_MINIMAP_SECS, MIN_FULLSCREEN_ZOOM_OUT_PERCENT, MIN_FULLSCREEN_ZOOM_STEP_PERCENT,
    MAX_FULLSCREEN_IDLE_CURSOR_MS, MIN_FULLSCREEN_IDLE_CURSOR_MS, MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, MIN_MAX_HISTORY_TAGS, MAX_MAX_HISTORY_TAGS,
    MAX_TYPE_AHEAD_MULTIVIEW_DELAY_MS, MAX_TYPE_AHEAD_NEW_SEARCH_GAP_MS, MAX_TYPE_AHEAD_TIMEOUT_MS, MIN_TYPE_AHEAD_TIMEOUT_MS,
};
use rustview_core::util::hex_color;
use crate::system::{MonitorInfo, SystemDisplayService};
use crate::ui::color_picker::PickerRequest;
use crate::ui::{color_picker, MainWindow, SettingsWindow, TooltipSettings};
use slint::{CloseRequestResponse, Color, ComponentHandle, ModelRc, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

pub struct SettingsDialogController {
    settings_window: Option<SettingsWindow>,
    manager: Arc<SettingsManager>,
    main_window_weak: Option<slint::Weak<MainWindow>>,
    detected_monitors: Rc<RefCell<Vec<MonitorInfo>>>,
    formats_page: Arc<FormatsPage>,
}

impl SettingsDialogController {
    pub fn new(manager: Arc<SettingsManager>) -> Self {
        Self {
            settings_window: None,
            manager,
            main_window_weak: None,
            detected_monitors: Rc::new(RefCell::new(Vec::new())),
            formats_page: Arc::new(FormatsPage::default()),
        }
    }

    /// Lets the File Formats page show how many indexed files each format owns (call before the first `show`).
    pub fn set_format_counter(&self, counter: FormatCounter) {
        self.formats_page.set_counter(counter);
    }

    pub fn set_main_window(&mut self, weak: slint::Weak<MainWindow>) {
        self.main_window_weak = Some(weak);
    }

    /// Checks if the settings window is currently open and mapped.
    pub fn is_open(&self) -> bool {
        self.settings_window
            .as_ref()
            .map(|w| w.window().is_visible())
            .unwrap_or(false)
    }

    /// Programmatically closes and hides the settings window, releasing the main window modal lock.
    pub fn close(&self) {
        if let Some(ref w) = self.settings_window {
            color_picker::close();
                let _ = w.hide();
        }
        if let Some(ref mw_weak) = self.main_window_weak
            && let Some(mw) = mw_weak.upgrade()
        {
            mw.set_is_modal_locked(false);
        }
    }

    pub fn show(&mut self) {
        if self.settings_window.is_none() {
            let window = SettingsWindow::new().expect("Failed to create SettingsWindow");
            color_picker::install(&window);
            self.formats_page.install(&window);
            self.register_callbacks(&window);
            self.settings_window = Some(window);
        }

        if let Some(ref window) = self.settings_window {
            self.populate_ui(window, &self.manager.get());

            if let Some(ref w_weak) = self.main_window_weak
                && let Some(main) = w_weak.upgrade()
            {
                main.set_is_modal_locked(true);
            }

            let _ = window.show();
        }
    }

    fn populate_ui(&self, window: &SettingsWindow, settings: &AppSettings) {
        window.set_app_version(env!("CARGO_PKG_VERSION").into());
        window.set_root_folder_path(
            settings
                .general
                .root_folder
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
                .into(),
        );
        window.set_ignore_nested_mounts(settings.general.ignore_nested_mounts);
        window.set_remember_last_folder(settings.general.remember_last_folder);
        window.set_confirm_delete(settings.general.confirm_on_file_delete);
        window.set_confirm_open_file_count(settings.general.confirm_open_file_count as i32);

        window.set_auto_reveal_remembered_folder(settings.folders.auto_reveal_remembered_folder);
        window.set_persist_folder_tree_state(settings.folders.persist_folder_tree_state);
        window.set_show_deleted_folders(settings.folders.show_deleted_folders);
        window.set_show_folder_metrics(settings.folders.show_folder_metrics);
        window.set_show_hidden_folders(settings.folders.show_hidden_folders);
        window.set_support_dot_hidden_files(settings.folders.support_dot_hidden_files);

        // Appearance properties
        window.set_default_thumb_size(settings.appearance.default_thumbnail_size as i32);
        let ratio = GridAspectRatio::from_str_name(&settings.appearance.thumbnail_aspect_ratio);
        window.set_thumb_aspect_idx(ratio.to_index());
        window.set_show_filenames(settings.appearance.show_filenames);
        window.set_show_rating_badge(settings.appearance.show_ratings_overlay);
        window.set_show_tag_badge(settings.appearance.show_tags_overlay);
        window.set_show_media_type_badges(settings.appearance.show_media_type_badges);
        window.set_show_duration_badge(settings.appearance.show_duration_badge);
        window.set_show_animated_badge(settings.appearance.show_animated_badge);
        window.set_thumbnail_badge_overflow_idx(
            ThumbnailBadgeOverflow::from_str_name(&settings.appearance.thumbnail_badge_overflow)
                .to_index(),
        );
        window.set_enable_hover_previews(settings.appearance.enable_hover_previews);
        window.set_enable_tooltips(settings.appearance.enable_tooltips);
        window.set_tooltip_delay_ms(settings.appearance.tooltip_delay_ms as i32);
        // The window reacts to later changes of these two; the values it opens with are set here.
        let tooltips = window.global::<TooltipSettings>();
        tooltips.set_enabled(settings.appearance.enable_tooltips);
        tooltips.set_default_delay(i64::from(settings.appearance.tooltip_delay_ms));
        window.set_type_ahead_enabled(settings.appearance.type_ahead_enabled);
        window.set_type_ahead_timeout_ms(settings.appearance.type_ahead_timeout_ms as i32);
        window.set_type_ahead_new_search_gap_ms(settings.appearance.type_ahead_new_search_gap_ms as i32);
        window.set_type_ahead_include_folders(settings.appearance.type_ahead_include_folders);
        window.set_type_ahead_cycle_repeated_keys(settings.appearance.type_ahead_cycle_repeated_keys);
        window.set_type_ahead_center_target(settings.appearance.type_ahead_center_target);
        window.set_type_ahead_show_no_match(settings.appearance.type_ahead_show_no_match);
        window.set_type_ahead_multiview_delay_ms(settings.appearance.type_ahead_multiview_delay_ms as i32);

        // Grid Items Layout properties
        window.set_grid_item_padding_h(settings.appearance.grid_item_padding_horizontal as i32);
        window.set_grid_item_padding_v(settings.appearance.grid_item_padding_vertical as i32);
        window.set_grid_spacing(settings.appearance.grid_spacing as i32);
        window.set_grid_padding(settings.appearance.grid_padding as i32);
        window.set_grid_item_border_width(settings.appearance.grid_item_border_width as i32);
        window.set_grid_item_corner_radius(settings.appearance.grid_item_corner_radius as i32);

        // Performance properties
        window.set_worker_threads(settings.performance.worker_threads as i32);
        window.set_batch_commit_size(settings.performance.batch_commit_size as i32);
        let system = SystemMemory::read();
        window.set_memory_budget_auto(settings.performance.memory_budget_mb == 0);
        window.set_memory_budget_mb(
            if settings.performance.memory_budget_mb == 0 {
                MemoryPlan::resolve(0, system).budget_bytes / BYTES_PER_MB
            } else {
                u64::from(settings.performance.memory_budget_mb)
            } as i32,
        );
        window.set_memory_budget_max_mb((system.usable_total_bytes() / BYTES_PER_MB).max(1024) as i32);
        Self::show_memory_plan(window, system);
        window.set_thumb_db_size(settings.performance.thumbnail_db_size as i32);
        window.set_thumb_format_idx(
            if settings
                .performance
                .thumbnail_format
                .eq_ignore_ascii_case("jpeg")
            {
                1
            } else {
                0
            },
        );
        window.set_thumb_quality(settings.performance.thumbnail_quality as i32);
        window.set_scaling_algo_idx(
            match settings
                .performance
                .scaling_algorithm
                .to_lowercase()
                .as_str()
            {
                "bilinear" | "triangle" => 1,
                "lanczos" | "lanczos3" => 2,
                "nearest" => 3,
                _ => 0, // CatmullRom
            },
        );
        window.set_video_engine_idx(
            if settings
                .performance
                .video_engine
                .eq_ignore_ascii_case("mpv")
            {
                1
            } else {
                0
            },
        );
        window.set_video_offset_percent(settings.performance.video_offset_percent as i32);
        window.set_short_video_threshold_secs(
            settings.performance.short_video_threshold_secs as i32,
        );
        window.set_generate_video_previews(settings.performance.generate_video_previews);
        window.set_preview_frame_count(settings.performance.preview_frame_count as i32);
        window.set_custom_preview_settings(settings.performance.custom_preview_settings);
        window.set_preview_format_idx(
            if settings
                .performance
                .preview_format
                .eq_ignore_ascii_case("jpeg")
            {
                1
            } else {
                0
            },
        );
        window.set_preview_quality(settings.performance.preview_quality as i32);

        // Formats properties
        self.formats_page.populate(window, settings.formats.format_mask());
        window.set_auto_detect_extensionless(settings.formats.auto_detect_extensionless_media);

        // Viewer properties
        window.set_open_images_in_tab(settings.viewer.images.open_in_tab);
        window.set_allow_multiple_image_tabs(settings.viewer.images.allow_multiple_tabs);
        window.set_open_videos_in_tab(settings.viewer.videos.open_in_tab);
        window.set_allow_multiple_video_tabs(settings.viewer.videos.allow_multiple_tabs);
        window.set_viewer_show_tab_rating_badge(settings.viewer.tabs.show_rating_badge);
        window.set_viewer_background_mode_idx(
            match settings.viewer.background_mode.to_lowercase().as_str() {
                "deep black" | "black" => 1,
                "transparent checkerboard" | "checkerboard" => 2,
                _ => 0,
            },
        );

        // Fullscreen & Target Display properties
        let monitors = SystemDisplayService::detect_monitors();
        let mut options = Vec::with_capacity(monitors.len());
        for m in &monitors {
            options.push(slint::SharedString::from(m.display_name()));
        }

        if options.is_empty() {
            options.push(slint::SharedString::from("No displays detected"));
        }

        let target_saved = &settings.fullscreen.specific_monitor;
        let mut selected_idx = 0usize;
        if !target_saved.is_empty() {
            for (idx, m) in monitors.iter().enumerate() {
                if m.connector.eq_ignore_ascii_case(target_saved)
                    || m.model.eq_ignore_ascii_case(target_saved)
                {
                    selected_idx = idx;
                    break;
                }
            }
        }

        *self.detected_monitors.borrow_mut() = monitors;
        window.set_display_options(ModelRc::new(VecModel::from(options)));
        window.set_selected_display_idx(selected_idx as i32);

        window.set_fullscreen_enable_specific(settings.fullscreen.enable_specific_monitor);
        window.set_fullscreen_enable_fallback(settings.fullscreen.enable_fallback_rule);
        let fallback_idx = if settings.fullscreen.fallback_target.eq_ignore_ascii_case("different") {
            1
        } else {
            0
        };
        window.set_fullscreen_fallback_target_idx(fallback_idx);
        Self::populate_fullscreen_extras(window, &settings.fullscreen);

        // Tags & Ratings properties
        window.set_max_history_tags(settings.tags.max_history_tags as i32);

        // API settings
        window.set_auto_complete_external_stubs(settings.api.auto_complete_external_stubs);

        // Debug properties
        window.set_obfuscate_logs(settings.debug.obfuscate_logs);
    }

    /// Action bar, sound and background of the fullscreen.
    fn populate_fullscreen_extras(window: &SettingsWindow, fullscreen: &FullscreenSettings) {
        let bar = &fullscreen.media_bar;
        window.set_fullscreen_media_bar_enabled(bar.enabled);
        window.set_fullscreen_media_bar_show_mute(bar.show_mute);
        window.set_fullscreen_media_bar_show_sticky(bar.show_sticky);
        window.set_fullscreen_media_bar_show_duplicate(bar.show_duplicate);
        window.set_fullscreen_media_bar_show_open_as_tab(bar.show_open_as_tab);
        window.set_fullscreen_media_bar_show_open_with(bar.show_open_with);
        window.set_fullscreen_media_bar_show_zoom(bar.show_zoom);
        window.set_fullscreen_media_bar_show_delete(bar.show_delete);
        window.set_fullscreen_media_bar_show_remove(bar.show_remove);
        window.set_fullscreen_media_bar_reveal_distance_px(bar.reveal_distance_px as i32);

        let primary = &fullscreen.primary_bar;
        window.set_fullscreen_primary_bar_enabled(primary.enabled);
        window.set_fullscreen_primary_bar_show_send_to_tab(primary.show_send_to_tab);
        window.set_fullscreen_primary_bar_show_close(primary.show_close);
        window.set_fullscreen_primary_bar_show_mute_all(primary.show_mute_all);
        window.set_fullscreen_primary_bar_show_play_pause_all(primary.show_play_pause_all);
        window.set_fullscreen_primary_bar_show_reset_layout(primary.show_reset_layout);
        window.set_fullscreen_primary_bar_show_hide_cursor(primary.show_hide_cursor);
        window.set_fullscreen_primary_bar_show_pin(primary.show_pin);
        window.set_fullscreen_primary_bar_reveal_distance_px(primary.reveal_distance_px as i32);
        window.set_fullscreen_hide_idle_cursor(primary.hide_idle_cursor);
        window.set_fullscreen_idle_cursor_secs((primary.idle_cursor_ms as f32 / 1000.0).round() as i32);

        let zoom = &fullscreen.zoom;
        window.set_fullscreen_zoom_step_percent(zoom.step_percent as i32);
        window.set_fullscreen_zoom_max_percent(zoom.max_percent as i32);
        window.set_fullscreen_zoom_out_percent(zoom.out_percent as i32);
        window.set_fullscreen_zoom_minimap_secs(zoom.minimap_secs as i32);
        window.set_fullscreen_zoom_smooth(zoom.smooth);
        window.set_fullscreen_zoom_minimap_idx(i32::from(zoom.minimap_position == "bottom_right"));
        window.set_fullscreen_zoom_minimap_always(zoom.minimap_always);
        window.set_fullscreen_zoom_resample_idx(match zoom.resample.as_str() {
            "catmull_rom" => 1,
            "bilinear" => 2,
            "nearest" => 3,
            _ => 0,
        });

        let sound = &fullscreen.sound;
        window.set_fullscreen_sound_auto_control(sound.auto_control);
        window.set_fullscreen_sound_follows_idx(match sound.follows.as_str() {
            "newest" => 1,
            "nobody" => 2,
            _ => 0,
        });
        window.set_fullscreen_sound_hand_over(sound.hand_over);

        window.set_fullscreen_background_source_idx(match fullscreen.background.source.as_str() {
            "viewer" => 1,
            "custom" => 2,
            _ => 0,
        });
        let [r, g, b] = hex_color::parse_rgb(&fullscreen.background.color)
            .or_else(|| hex_color::parse_rgb(DEFAULT_FULLSCREEN_BACKGROUND_COLOR))
            .unwrap_or([0x11, 0x11, 0x1b]);
        window.set_fullscreen_background_color(Color::from_rgb_u8(r, g, b));
    }

    fn save_fullscreen_extras(window: &SettingsWindow, fullscreen: &mut FullscreenSettings) {
        let bar = &mut fullscreen.media_bar;
        bar.enabled = window.get_fullscreen_media_bar_enabled();
        bar.show_mute = window.get_fullscreen_media_bar_show_mute();
        bar.show_sticky = window.get_fullscreen_media_bar_show_sticky();
        bar.show_duplicate = window.get_fullscreen_media_bar_show_duplicate();
        bar.show_open_as_tab = window.get_fullscreen_media_bar_show_open_as_tab();
        bar.show_open_with = window.get_fullscreen_media_bar_show_open_with();
        bar.show_zoom = window.get_fullscreen_media_bar_show_zoom();
        bar.show_delete = window.get_fullscreen_media_bar_show_delete();
        bar.show_remove = window.get_fullscreen_media_bar_show_remove();
        bar.reveal_distance_px = u32::try_from(window.get_fullscreen_media_bar_reveal_distance_px())
            .unwrap_or(0)
            .clamp(MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX);

        let primary = &mut fullscreen.primary_bar;
        primary.enabled = window.get_fullscreen_primary_bar_enabled();
        primary.show_send_to_tab = window.get_fullscreen_primary_bar_show_send_to_tab();
        primary.show_close = window.get_fullscreen_primary_bar_show_close();
        primary.show_mute_all = window.get_fullscreen_primary_bar_show_mute_all();
        primary.show_play_pause_all = window.get_fullscreen_primary_bar_show_play_pause_all();
        primary.show_reset_layout = window.get_fullscreen_primary_bar_show_reset_layout();
        primary.show_hide_cursor = window.get_fullscreen_primary_bar_show_hide_cursor();
        primary.show_pin = window.get_fullscreen_primary_bar_show_pin();
        primary.reveal_distance_px = u32::try_from(window.get_fullscreen_primary_bar_reveal_distance_px())
            .unwrap_or(0)
            .clamp(MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX);
        primary.hide_idle_cursor = window.get_fullscreen_hide_idle_cursor();
        let secs = u32::try_from(window.get_fullscreen_idle_cursor_secs()).unwrap_or(1);
        primary.idle_cursor_ms = (secs * 1000).clamp(MIN_FULLSCREEN_IDLE_CURSOR_MS, MAX_FULLSCREEN_IDLE_CURSOR_MS);

        let zoom = &mut fullscreen.zoom;
        zoom.step_percent = u32::try_from(window.get_fullscreen_zoom_step_percent())
            .unwrap_or(DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT)
            .clamp(MIN_FULLSCREEN_ZOOM_STEP_PERCENT, MAX_FULLSCREEN_ZOOM_STEP_PERCENT);
        zoom.max_percent = u32::try_from(window.get_fullscreen_zoom_max_percent())
            .unwrap_or(DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT)
            .clamp(MIN_FULLSCREEN_ZOOM_MAX_PERCENT, MAX_FULLSCREEN_ZOOM_MAX_PERCENT);
        zoom.out_percent = u32::try_from(window.get_fullscreen_zoom_out_percent())
            .unwrap_or(DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT)
            .clamp(MIN_FULLSCREEN_ZOOM_OUT_PERCENT, MAX_FULLSCREEN_ZOOM_OUT_PERCENT);
        zoom.minimap_secs = u32::try_from(window.get_fullscreen_zoom_minimap_secs())
            .unwrap_or(DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS)
            .clamp(MIN_FULLSCREEN_ZOOM_MINIMAP_SECS, MAX_FULLSCREEN_ZOOM_MINIMAP_SECS);
        zoom.smooth = window.get_fullscreen_zoom_smooth();
        zoom.minimap_position = if window.get_fullscreen_zoom_minimap_idx() == 1 { "bottom_right" } else { "bottom_left" }.to_string();
        zoom.minimap_always = window.get_fullscreen_zoom_minimap_always();
        zoom.resample = match window.get_fullscreen_zoom_resample_idx() {
            1 => "catmull_rom",
            2 => "bilinear",
            3 => "nearest",
            _ => "lanczos3",
        }
        .to_string();

        let sound = &mut fullscreen.sound;
        sound.auto_control = window.get_fullscreen_sound_auto_control();
        sound.follows = match window.get_fullscreen_sound_follows_idx() {
            1 => "newest",
            2 => "nobody",
            _ => "first",
        }
        .to_string();
        sound.hand_over = window.get_fullscreen_sound_hand_over();

        fullscreen.background.source = match window.get_fullscreen_background_source_idx() {
            1 => "viewer",
            2 => "custom",
            _ => "theme",
        }
        .to_string();
        let color = window.get_fullscreen_background_color();
        fullscreen.background.color = hex_color::format_rgb([color.red(), color.green(), color.blue()]);
    }

    /// Shows the summary and the warnings of the budget the dialog currently describes.
    fn show_memory_plan(window: &SettingsWindow, system: SystemMemory) {
        let setting_mb = if window.get_memory_budget_auto() { 0 } else { window.get_memory_budget_mb().max(0) as u32 };
        let plan = MemoryPlan::resolve(setting_mb, system);
        window.set_memory_summary(plan.summary().into());
        window.set_memory_warning(plan.warning_text().into());
    }

    fn register_callbacks(&self, window: &SettingsWindow) {
        let w_weak = window.as_weak();

        // The summary and the warnings follow the budget controls while the dialog is open (the machine is read once).
        let system = SystemMemory::read();
        let w_weak_memory = w_weak.clone();
        window.on_memory_budget_edited(move || {
            if let Some(w) = w_weak_memory.upgrade() {
                Self::show_memory_plan(&w, system);
            }
        });

        let manager = self.manager.clone();
        let main_weak = self.main_window_weak.clone();
        let formats_page_save = self.formats_page.clone();

        let w_weak_browse = w_weak.clone();
        window.on_browse_root_folder(move || {
            if let Some(folder) = rfd::FileDialog::new()
                .set_title("Select Root Directory for RustView")
                .pick_folder()
                && let Some(w) = w_weak_browse.upgrade()
            {
                w.set_root_folder_path(folder.to_string_lossy().to_string().into());
            }
        });

        // Interactive Display Refreshment
        let w_weak_refresh = w_weak.clone();
        let detected_monitors_refresh = self.detected_monitors.clone();
        window.on_refresh_displays(move || {
            tracing::info!(
                "[SettingsDialog: Displays] User triggered display re-scan from Settings dialog"
            );
            let monitors = SystemDisplayService::detect_monitors();
            let mut options = Vec::with_capacity(monitors.len());
            for m in &monitors {
                options.push(slint::SharedString::from(m.display_name()));
            }

            if options.is_empty() {
                options.push(slint::SharedString::from("No displays detected"));
            }

            if let Some(w) = w_weak_refresh.upgrade() {
                let cur_idx = w.get_selected_display_idx() as usize;
                let prev_monitors = detected_monitors_refresh.borrow();
                let cur_connector = prev_monitors.get(cur_idx).map(|m| m.connector.clone());
                drop(prev_monitors);

                let mut new_idx = 0usize;
                if let Some(ref conn) = cur_connector {
                    for (i, m) in monitors.iter().enumerate() {
                        if m.connector.eq_ignore_ascii_case(conn) {
                            new_idx = i;
                            break;
                        }
                    }
                }

                *detected_monitors_refresh.borrow_mut() = monitors;
                w.set_display_options(ModelRc::new(VecModel::from(options)));
                w.set_selected_display_idx(new_idx as i32);
            }
        });

        let w_weak_save = w_weak.clone();
        let mgr_save = manager;
        let main_weak_save = main_weak.clone();
        let detected_monitors_save = self.detected_monitors.clone();

        window.on_save_and_apply(move || {
            if let Some(w) = w_weak_save.upgrade() {
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
                    .clamp(MIN_CONFIRM_OPEN_FILE_COUNT as i32, MAX_CONFIRM_OPEN_FILE_COUNT as i32)
                    as u32;
                current.general.ignore_nested_mounts = w.get_ignore_nested_mounts();

                current.folders.auto_reveal_remembered_folder =
                    w.get_auto_reveal_remembered_folder();
                current.folders.persist_folder_tree_state = w.get_persist_folder_tree_state();
                current.folders.show_deleted_folders = w.get_show_deleted_folders();
                current.folders.show_folder_metrics = w.get_show_folder_metrics();
                current.folders.show_hidden_folders = w.get_show_hidden_folders();
                current.folders.support_dot_hidden_files = w.get_support_dot_hidden_files();

                current.appearance.default_thumbnail_size = w.get_default_thumb_size() as u32;
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
                current.appearance.tooltip_delay_ms =
                    w.get_tooltip_delay_ms().clamp(50, 2000) as u32;
                current.appearance.type_ahead_enabled = w.get_type_ahead_enabled();
                current.appearance.type_ahead_timeout_ms = w
                    .get_type_ahead_timeout_ms()
                    .clamp(MIN_TYPE_AHEAD_TIMEOUT_MS as i32, MAX_TYPE_AHEAD_TIMEOUT_MS as i32)
                    as u32;
                current.appearance.type_ahead_new_search_gap_ms = w
                    .get_type_ahead_new_search_gap_ms()
                    .clamp(0, MAX_TYPE_AHEAD_NEW_SEARCH_GAP_MS as i32)
                    as u32;
                current.appearance.type_ahead_include_folders = w.get_type_ahead_include_folders();
                current.appearance.type_ahead_cycle_repeated_keys = w.get_type_ahead_cycle_repeated_keys();
                current.appearance.type_ahead_center_target = w.get_type_ahead_center_target();
                current.appearance.type_ahead_show_no_match = w.get_type_ahead_show_no_match();
                current.appearance.type_ahead_multiview_delay_ms = w
                    .get_type_ahead_multiview_delay_ms()
                    .clamp(0, MAX_TYPE_AHEAD_MULTIVIEW_DELAY_MS as i32)
                    as u32;

                // Save Grid Items Layout Properties
                current.appearance.grid_item_padding_horizontal =
                    w.get_grid_item_padding_h().clamp(0, 32) as u32;
                current.appearance.grid_item_padding_vertical =
                    w.get_grid_item_padding_v().clamp(0, 32) as u32;
                current.appearance.grid_spacing = w.get_grid_spacing().clamp(0, 48) as u32;
                current.appearance.grid_padding = w.get_grid_padding().clamp(0, 48) as u32;
                current.appearance.grid_item_border_width =
                    w.get_grid_item_border_width().clamp(0, 6) as u32;
                current.appearance.grid_item_corner_radius =
                    w.get_grid_item_corner_radius().clamp(0, 24) as u32;

                // Save Performance Parameters
                current.performance.worker_threads = w.get_worker_threads().clamp(1, 32) as usize;
                current.performance.batch_commit_size =
                    w.get_batch_commit_size().clamp(4, 256) as usize;
                current.performance.memory_budget_mb = if w.get_memory_budget_auto() {
                    0
                } else {
                    w.get_memory_budget_mb().clamp(MIN_MEMORY_BUDGET_MB as i32, MAX_MEMORY_BUDGET_MB as i32) as u32
                };
                current.performance.thumbnail_db_size =
                    w.get_thumb_db_size().clamp(64, 1024) as u32;
                current.performance.thumbnail_format = if w.get_thumb_format_idx() == 1 {
                    "JPEG".to_string()
                } else {
                    "WebP".to_string()
                };
                current.performance.thumbnail_quality = w.get_thumb_quality().clamp(10, 100) as u8;
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
                current.performance.video_offset_percent =
                    w.get_video_offset_percent().clamp(1, 90) as u8;
                current.performance.short_video_threshold_secs =
                    w.get_short_video_threshold_secs().clamp(1, 60) as u32;
                current.performance.generate_video_previews = w.get_generate_video_previews();
                current.performance.preview_frame_count =
                    w.get_preview_frame_count().clamp(2, 100) as u32;
                current.performance.custom_preview_settings = w.get_custom_preview_settings();
                current.performance.preview_format = if w.get_preview_format_idx() == 1 {
                    "JPEG".to_string()
                } else {
                    "WebP".to_string()
                };
                current.performance.preview_quality = w.get_preview_quality().clamp(10, 100) as u8;

                // Save Formats Parameters
                current.formats.auto_detect_extensionless_media = w.get_auto_detect_extensionless();
                current.formats.disabled_formats = formats_page_save.disabled_ids();

                // Save Viewer Parameters
                current.viewer.images.open_in_tab = w.get_open_images_in_tab();
                current.viewer.images.allow_multiple_tabs = w.get_allow_multiple_image_tabs();
                current.viewer.videos.open_in_tab = w.get_open_videos_in_tab();
                current.viewer.videos.allow_multiple_tabs = w.get_allow_multiple_video_tabs();
                current.viewer.tabs.show_rating_badge = w.get_viewer_show_tab_rating_badge();
                current.viewer.background_mode = match w.get_viewer_background_mode_idx() {
                    1 => "Deep Black".to_string(),
                    2 => "Transparent Checkerboard".to_string(),
                    _ => "Dark".to_string(),
                };

                // Save Fullscreen & Target Display Parameters
                current.fullscreen.enable_specific_monitor = w.get_fullscreen_enable_specific();
                let sel_disp_idx = w.get_selected_display_idx() as usize;
                let monitors_guard = detected_monitors_save.borrow();
                if let Some(m) = monitors_guard.get(sel_disp_idx) {
                    current.fullscreen.specific_monitor = m.connector.clone();
                }
                drop(monitors_guard);

                current.fullscreen.enable_fallback_rule = w.get_fullscreen_enable_fallback();
                current.fullscreen.fallback_target = if w.get_fullscreen_fallback_target_idx() == 1 {
                    "different".to_string()
                } else {
                    "same".to_string()
                };
                Self::save_fullscreen_extras(&w, &mut current.fullscreen);

                // Save Tag History Configuration
                let new_max_history = (w.get_max_history_tags() as usize).clamp(MIN_MAX_HISTORY_TAGS, MAX_MAX_HISTORY_TAGS);
                current.tags.max_history_tags = new_max_history;
                current.tags.history_tag_ids.truncate(new_max_history);

                // Save API Parameters
                current.api.auto_complete_external_stubs = w.get_auto_complete_external_stubs();

                // Save Debug Parameters
                current.debug.obfuscate_logs = w.get_obfuscate_logs();

                mgr_save.update(current);

                if let Some(ref mw) = main_weak_save
                    && let Some(main) = mw.upgrade()
                {
                    main.set_is_modal_locked(false);
                }

                color_picker::close();
                let _ = w.hide();
            }
        });

        let w_weak_cancel = w_weak;
        let main_weak_cancel = main_weak;
        let w_weak_pick = w_weak_cancel.clone();
        window.on_pick_fullscreen_background(move || {
            let Some(w) = w_weak_pick.upgrade() else {
                return;
            };
            let [r, g, b] = hex_color::parse_rgb(DEFAULT_FULLSCREEN_BACKGROUND_COLOR).unwrap_or([0x11, 0x11, 0x1b]);
            let target = w.as_weak();
            color_picker::open(
                &w,
                PickerRequest {
                    title: "Fullscreen background color",
                    initial: w.get_fullscreen_background_color(),
                    default: Some(Color::from_rgb_u8(r, g, b)),
                    show_alpha: false,
                },
                move |color| {
                    if let Some(w) = target.upgrade() {
                        w.set_fullscreen_background_color(color);
                    }
                },
            );
        });

        window.on_cancel_clicked(move || {
            tracing::info!(
                "[SettingsDialog: Cancel] Settings window cancelled/dismissed via Escape key or Cancel button"
            );
            if let Some(w) = w_weak_cancel.upgrade() {
                if let Some(ref mw) = main_weak_cancel
                    && let Some(mw) = mw.upgrade()
                {
                    mw.set_is_modal_locked(false);
                }
                color_picker::close();
                let _ = w.hide();
            }
        });

        let main_weak_close = self.main_window_weak.clone();
        window.window().on_close_requested(move || {
            tracing::info!(
                "[SettingsDialog: CloseRequest] Settings window closed via titlebar close button"
            );
            if let Some(ref mw) = main_weak_close
                && let Some(main) = mw.upgrade()
            {
                main.set_is_modal_locked(false);
            }
            color_picker::close();
            CloseRequestResponse::HideWindow
        });
    }
}
