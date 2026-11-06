mod ab_repeat_card;
mod apply;
mod confirmations;
mod debug_page;
mod formats_page;
mod hotkeys_page;
mod image_edit_page;
mod player_bar;
mod selected_medium;

/// The index of the page Hotkeys in the sidebar of the settings window (`ui/settings_sidebar.slint`).
pub const HOTKEYS_PAGE: i32 = 9;

use crate::system::{MonitorInfo, SystemDisplayService};
use crate::ui::color_picker::PickerRequest;
use crate::ui::{MainWindow, SettingsWindow, TooltipSettings, color_picker};
use debug_page::DebugPageController;
pub use debug_page::{DEBUG_PAGE, DebugContext};
pub use formats_page::FormatCounter;
use formats_page::FormatsPage;
use hotkeys_page::HotkeysPageController;
use rustview_core::settings::memory_budget::{BYTES_PER_MB, MemoryPlan, SystemMemory};
use rustview_core::settings::registry::SettingsManager;
use rustview_core::settings::schema::{
    AppSettings, DEFAULT_FULLSCREEN_BACKGROUND_COLOR, DEFAULT_FULLSCREEN_ZOOM_MAX_PERCENT,
    DEFAULT_FULLSCREEN_ZOOM_MINIMAP_SECS, DEFAULT_FULLSCREEN_ZOOM_OUT_PERCENT, DEFAULT_FULLSCREEN_ZOOM_STEP_PERCENT,
    FullscreenSettings, GridAspectRatio, MAX_CONFIRM_OPEN_FILE_COUNT, MAX_FULLSCREEN_IDLE_CURSOR_MS,
    MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, MAX_FULLSCREEN_ZOOM_MAX_PERCENT, MAX_FULLSCREEN_ZOOM_MINIMAP_SECS,
    MAX_FULLSCREEN_ZOOM_OUT_PERCENT, MAX_FULLSCREEN_ZOOM_STEP_PERCENT, MAX_MAX_HISTORY_TAGS, MAX_MEMORY_BUDGET_MB,
    MAX_TYPE_AHEAD_MULTIVIEW_DELAY_MS, MAX_TYPE_AHEAD_NEW_SEARCH_GAP_MS, MAX_TYPE_AHEAD_TIMEOUT_MS,
    MAX_VIEWER_VIDEO_JUMP_SECS, MIN_CONFIRM_OPEN_FILE_COUNT, MIN_FULLSCREEN_IDLE_CURSOR_MS,
    MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX, MIN_FULLSCREEN_ZOOM_MAX_PERCENT, MIN_FULLSCREEN_ZOOM_MINIMAP_SECS,
    MIN_FULLSCREEN_ZOOM_OUT_PERCENT, MIN_FULLSCREEN_ZOOM_STEP_PERCENT, MIN_MAX_HISTORY_TAGS, MIN_MEMORY_BUDGET_MB,
    MIN_TYPE_AHEAD_TIMEOUT_MS, MIN_VIEWER_VIDEO_JUMP_SECS, ThumbnailBadgeOverflow, VIEWER_TAB_CLOSE_BUTTON_MODES,
};
use rustview_core::util::cast::{Az, FloatToInt, SaturatingAs};

use rustview_core::util::hex_color;
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
    hotkeys_page: Rc<HotkeysPageController>,
    debug_page: Rc<DebugPageController>,
}

impl SettingsDialogController {
    pub fn new(manager: Arc<SettingsManager>) -> Self {
        let hotkeys_page = HotkeysPageController::new(manager.clone());
        let debug_page = DebugPageController::new(manager.clone());
        Self {
            settings_window: None,
            manager,
            main_window_weak: None,
            detected_monitors: Rc::new(RefCell::new(Vec::new())),
            formats_page: Arc::new(FormatsPage::default()),
            hotkeys_page,
            debug_page,
        }
    }

    /// Lets the File Formats page show how many indexed files each format owns (call before the first `show`).
    pub fn set_format_counter(&self, counter: FormatCounter) {
        self.formats_page.set_counter(counter);
    }

    /// Gives Settings > Debug what it needs from the rest of the app (call before the first `show`).
    pub fn set_debug_context(&self, context: DebugContext) {
        self.debug_page.set_context(context);
    }

    pub fn set_main_window(&mut self, weak: slint::Weak<MainWindow>) {
        self.main_window_weak = Some(weak);
    }

    /// Checks if the settings window is currently open and mapped.
    pub fn is_open(&self) -> bool {
        self.settings_window.as_ref().is_some_and(|w| w.window().is_visible())
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

    /// Opens the window on the page `category` (the index of the sidebar entry, e.g. [`HOTKEYS_PAGE`]).
    pub fn show_page(&mut self, category: i32) {
        Self::show(self);
        if let Some(ref window) = self.settings_window {
            window.set_active_category(category);
        }
    }

    pub fn show(&mut self) {
        if self.settings_window.is_none() {
            let window = SettingsWindow::new().expect("Failed to create SettingsWindow");
            crate::ui::tooltip::install(&window);
            crate::ui::dialog_parent::install_window(&window, crate::ui::dialog_parent::Role::Settings);
            color_picker::install(&window);
            self.formats_page.install(&window);
            self.hotkeys_page.install(&window);
            self.debug_page.install(&window);
            let debug_page = self.debug_page.clone();
            window.on_debug_page_shown(move |shown| debug_page.page_shown(shown));
            self.register_callbacks(&window);
            self.settings_window = Some(window);
        }

        if let Some(ref window) = self.settings_window {
            Self::fill(&self.apply_context(), window, &self.manager.get());

            if let Some(ref w_weak) = self.main_window_weak
                && let Some(main) = w_weak.upgrade()
            {
                main.set_is_modal_locked(true);
            }

            let _ = crate::ui::dialog_parent::show(window);
            // The window keeps its page between openings: no change event tells the Debug page it is shown again.
            if window.get_active_category() == DEBUG_PAGE {
                self.debug_page.page_shown(true);
            }
        }
    }

    /// What applying and filling need besides the window.
    fn apply_context(&self) -> apply::ApplyContext {
        apply::ApplyContext {
            mgr: Arc::clone(&self.manager),
            formats: Arc::clone(&self.formats_page),
            hotkeys: Rc::clone(&self.hotkeys_page),
            monitors: Rc::clone(&self.detected_monitors),
            debug: Rc::clone(&self.debug_page),
        }
    }

    /// Fills the window from `settings` without applying anything: edits apply again once it is filled.
    fn fill(ctx: &apply::ApplyContext, window: &SettingsWindow, settings: &AppSettings) {
        window.set_armed(false);
        Self::populate_ui(ctx, window, settings);
        let weak = window.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(400), move || {
            if let Some(w) = weak.upgrade() {
                w.set_armed(true);
            }
        });
    }

    fn populate_ui(ctx: &apply::ApplyContext, window: &SettingsWindow, settings: &AppSettings) {
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
        window.set_confirm_open_file_count(settings.general.confirm_open_file_count.saturating_as::<i32>());
        confirmations::populate(window, &settings.general);

        window.set_auto_reveal_remembered_folder(settings.folders.auto_reveal_remembered_folder);
        window.set_persist_folder_tree_state(settings.folders.persist_folder_tree_state);
        window.set_show_deleted_folders(settings.folders.show_deleted_folders);
        window.set_show_folder_metrics(settings.folders.show_folder_metrics);
        window.set_show_hidden_folders(settings.folders.show_hidden_folders);
        window.set_support_dot_hidden_files(settings.folders.support_dot_hidden_files);

        // Appearance properties
        window.set_default_thumb_size(settings.appearance.default_thumbnail_size.saturating_as::<i32>());
        let ratio = GridAspectRatio::from_str_name(&settings.appearance.thumbnail_aspect_ratio);
        window.set_thumb_aspect_idx(ratio.to_index());
        window.set_show_filenames(settings.appearance.show_filenames);
        window.set_show_rating_badge(settings.appearance.show_ratings_overlay);
        window.set_show_tag_badge(settings.appearance.show_tags_overlay);
        window.set_show_media_type_badges(settings.appearance.show_media_type_badges);
        window.set_show_duration_badge(settings.appearance.show_duration_badge);
        window.set_show_animated_badge(settings.appearance.show_animated_badge);
        window.set_thumbnail_badge_overflow_idx(
            ThumbnailBadgeOverflow::from_str_name(&settings.appearance.thumbnail_badge_overflow).to_index(),
        );
        window.set_enable_hover_previews(settings.appearance.enable_hover_previews);
        window.set_enable_tooltips(settings.appearance.enable_tooltips);
        window.set_tooltip_delay_ms(settings.appearance.tooltip_delay_ms.saturating_as::<i32>());
        // The window reacts to later changes of these two; the values it opens with are set here.
        let tooltips = window.global::<TooltipSettings>();
        tooltips.set_enabled(settings.appearance.enable_tooltips);
        tooltips.set_default_delay(i64::from(settings.appearance.tooltip_delay_ms));
        window.set_type_ahead_enabled(settings.appearance.type_ahead_enabled);
        window.set_type_ahead_timeout_ms(settings.appearance.type_ahead_timeout_ms.saturating_as::<i32>());
        window
            .set_type_ahead_new_search_gap_ms(settings.appearance.type_ahead_new_search_gap_ms.saturating_as::<i32>());
        window.set_type_ahead_include_folders(settings.appearance.type_ahead_include_folders);
        window.set_type_ahead_cycle_repeated_keys(settings.appearance.type_ahead_cycle_repeated_keys);
        window.set_type_ahead_center_target(settings.appearance.type_ahead_center_target);
        window.set_type_ahead_show_no_match(settings.appearance.type_ahead_show_no_match);
        window.set_type_ahead_multiview_delay_ms(
            settings.appearance.type_ahead_multiview_delay_ms.saturating_as::<i32>(),
        );

        // Grid Items Layout properties
        window.set_grid_item_padding_h(settings.appearance.grid_item_padding_horizontal.saturating_as::<i32>());
        window.set_grid_item_padding_v(settings.appearance.grid_item_padding_vertical.saturating_as::<i32>());
        window.set_grid_spacing(settings.appearance.grid_spacing.saturating_as::<i32>());
        window.set_grid_padding(settings.appearance.grid_padding.saturating_as::<i32>());
        window.set_grid_item_border_width(settings.appearance.grid_item_border_width.saturating_as::<i32>());
        window.set_grid_item_corner_radius(settings.appearance.grid_item_corner_radius.saturating_as::<i32>());

        // Performance properties
        window.set_worker_threads(settings.performance.worker_threads.saturating_as::<i32>());
        window.set_batch_commit_size(settings.performance.batch_commit_size.saturating_as::<i32>());
        let system = SystemMemory::read();
        window.set_memory_budget_auto(settings.performance.memory_budget_mb == 0);
        window.set_memory_budget_mb(
            (if settings.performance.memory_budget_mb == 0 {
                MemoryPlan::resolve(0, system).budget_bytes / BYTES_PER_MB
            } else {
                u64::from(settings.performance.memory_budget_mb)
            })
            .saturating_as::<i32>(),
        );
        window.set_memory_budget_max_mb(
            (system.usable_total_bytes() / BYTES_PER_MB)
                .max(1024)
                .saturating_as::<i32>(),
        );
        Self::show_memory_plan(window, system);
        window.set_thumb_db_size(settings.performance.thumbnail_db_size.saturating_as::<i32>());
        window.set_thumb_format_idx(i32::from(
            settings.performance.thumbnail_format.eq_ignore_ascii_case("jpeg"),
        ));
        window.set_thumb_quality(i32::from(settings.performance.thumbnail_quality));
        window.set_scaling_algo_idx(match settings.performance.scaling_algorithm.to_lowercase().as_str() {
            "bilinear" | "triangle" => 1,
            "lanczos" | "lanczos3" => 2,
            "nearest" => 3,
            _ => 0, // CatmullRom
        });
        window.set_video_engine_idx(i32::from(settings.performance.video_engine.eq_ignore_ascii_case("mpv")));
        window.set_video_offset_percent(i32::from(settings.performance.video_offset_percent));
        window.set_short_video_threshold_secs(settings.performance.short_video_threshold_secs.saturating_as::<i32>());
        window.set_generate_video_previews(settings.performance.generate_video_previews);
        window.set_preview_frame_count(settings.performance.preview_frame_count.saturating_as::<i32>());
        window.set_custom_preview_settings(settings.performance.custom_preview_settings);
        window.set_preview_format_idx(i32::from(
            settings.performance.preview_format.eq_ignore_ascii_case("jpeg"),
        ));
        window.set_preview_quality(i32::from(settings.performance.preview_quality));

        // Formats properties
        ctx.formats.populate(window, settings.formats.format_mask());
        ctx.hotkeys.populate(settings);
        window.set_auto_detect_extensionless(settings.formats.auto_detect_extensionless_media);

        // Viewer properties
        window.set_open_images_in_tab(settings.viewer.images.open_in_tab);
        window.set_allow_multiple_image_tabs(settings.viewer.images.allow_multiple_tabs);
        window.set_open_videos_in_tab(settings.viewer.videos.open_in_tab);
        window.set_allow_multiple_video_tabs(settings.viewer.videos.allow_multiple_tabs);
        window.set_video_jump_secs(settings.viewer.videos.jump_secs.saturating_as::<i32>());
        window.set_video_jump_long_secs(settings.viewer.videos.jump_long_secs.saturating_as::<i32>());
        ab_repeat_card::populate(window, &settings.viewer.videos.ab_repeat);
        window.set_viewer_show_tab_rating_badge(settings.viewer.tabs.show_rating_badge);
        let close_mode = VIEWER_TAB_CLOSE_BUTTON_MODES
            .iter()
            .position(|m| *m == settings.viewer.tabs.close_buttons)
            .unwrap_or(0);
        window.set_viewer_tab_close_buttons_idx(close_mode.saturating_as::<i32>());
        window.set_viewer_background_mode_idx(match settings.viewer.background_mode.to_lowercase().as_str() {
            "deep black" | "black" => 1,
            "transparent checkerboard" | "checkerboard" => 2,
            _ => 0,
        });

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
                if m.connector.eq_ignore_ascii_case(target_saved) || m.model.eq_ignore_ascii_case(target_saved) {
                    selected_idx = idx;
                    break;
                }
            }
        }

        *ctx.monitors.borrow_mut() = monitors;
        window.set_display_options(ModelRc::new(VecModel::from(options)));
        window.set_selected_display_idx(selected_idx.saturating_as::<i32>());

        window.set_fullscreen_enable_specific(settings.fullscreen.enable_specific_monitor);
        window.set_fullscreen_enable_fallback(settings.fullscreen.enable_fallback_rule);
        let fallback_idx = i32::from(settings.fullscreen.fallback_target.eq_ignore_ascii_case("different"));
        window.set_fullscreen_fallback_target_idx(fallback_idx);
        Self::populate_fullscreen_extras(window, &settings.fullscreen);
        image_edit_page::populate(window, &settings.image_edit);

        // Tags & Ratings properties
        window.set_max_history_tags(settings.tags.max_history_tags.saturating_as::<i32>());
        window.set_tag_tree_compact(settings.tags.compact_tree);

        // API settings
        window.set_auto_complete_external_stubs(settings.api.auto_complete_external_stubs);

        // Debug properties
        window.set_obfuscate_logs(settings.debug.obfuscate_logs);
        ctx.debug.populate(window, settings);
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
        window.set_fullscreen_media_bar_show_crop(bar.show_crop);
        window.set_fullscreen_media_bar_show_delete(bar.show_delete);
        window.set_fullscreen_media_bar_show_remove(bar.show_remove);
        window.set_fullscreen_media_bar_reveal_distance_px(bar.reveal_distance_px.saturating_as::<i32>());

        let primary = &fullscreen.primary_bar;
        window.set_fullscreen_primary_bar_enabled(primary.enabled);
        window.set_fullscreen_primary_bar_show_send_to_tab(primary.show_send_to_tab);
        window.set_fullscreen_primary_bar_show_close(primary.show_close);
        window.set_fullscreen_primary_bar_show_mute_all(primary.show_mute_all);
        window.set_fullscreen_primary_bar_show_play_pause_all(primary.show_play_pause_all);
        window.set_fullscreen_primary_bar_show_reset_layout(primary.show_reset_layout);
        window.set_fullscreen_primary_bar_show_hide_cursor(primary.show_hide_cursor);
        window.set_fullscreen_primary_bar_show_pin(primary.show_pin);
        window.set_fullscreen_primary_bar_reveal_distance_px(primary.reveal_distance_px.saturating_as::<i32>());
        window.set_fullscreen_hide_idle_cursor(primary.hide_idle_cursor);
        window.set_fullscreen_idle_cursor_secs((primary.idle_cursor_ms.az::<f32>() / 1000.0).round().to_int::<i32>());

        let zoom = &fullscreen.zoom;
        window.set_fullscreen_zoom_step_percent(zoom.step_percent.saturating_as::<i32>());
        window.set_fullscreen_zoom_max_percent(zoom.max_percent.saturating_as::<i32>());
        window.set_fullscreen_zoom_out_percent(zoom.out_percent.saturating_as::<i32>());
        window.set_fullscreen_zoom_minimap_secs(zoom.minimap_secs.saturating_as::<i32>());
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
            "selected" => 2,
            "nobody" => 3,
            _ => 0,
        });
        window.set_fullscreen_sound_hand_over(sound.hand_over);
        window.set_fullscreen_rating_show_feedback(fullscreen.rating.show_feedback);
        window.set_fullscreen_rating_keys(crate::ui::hotkeys::rating_keys_text().into());
        selected_medium::populate(window, fullscreen);
        player_bar::populate(window, &fullscreen.player);

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
        bar.show_crop = window.get_fullscreen_media_bar_show_crop();
        bar.show_delete = window.get_fullscreen_media_bar_show_delete();
        bar.show_remove = window.get_fullscreen_media_bar_show_remove();
        bar.reveal_distance_px = u32::try_from(window.get_fullscreen_media_bar_reveal_distance_px())
            .unwrap_or(0)
            .clamp(
                MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
                MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
            );

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
            .clamp(
                MIN_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
                MAX_FULLSCREEN_MEDIA_BAR_REVEAL_DISTANCE_PX,
            );
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
        zoom.minimap_position = if window.get_fullscreen_zoom_minimap_idx() == 1 {
            "bottom_right"
        } else {
            "bottom_left"
        }
        .to_string();
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
            2 => "selected",
            3 => "nobody",
            _ => "first",
        }
        .to_string();
        sound.hand_over = window.get_fullscreen_sound_hand_over();
        fullscreen.rating.show_feedback = window.get_fullscreen_rating_show_feedback();
        selected_medium::save(window, fullscreen);
        player_bar::save(window, &mut fullscreen.player);

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
        let setting_mb = if window.get_memory_budget_auto() {
            0
        } else {
            window.get_memory_budget_mb().max(0).saturating_as::<u32>()
        };
        let plan = MemoryPlan::resolve(setting_mb, system);
        window.set_memory_summary(plan.summary().into());
        window.set_memory_warning(plan.warning_text().into());
    }

    fn register_callbacks(&self, window: &SettingsWindow) {
        let w_weak = window.as_weak();
        image_edit_page::register_callbacks(window);
        ab_repeat_card::install(window);

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
        let hotkeys_page_save = self.hotkeys_page.clone();

        let w_weak_browse = w_weak.clone();
        window.on_browse_root_folder(move || {
            let Some(w) = w_weak_browse.upgrade() else { return };
            if let Some(folder) = crate::ui::dialog_parent::file_dialog(w.window())
                .set_title("Select Root Directory for RustView")
                .pick_folder()
            {
                w.set_root_folder_path(folder.to_string_lossy().to_string().into());
            }
        });

        // Interactive Display Refreshment
        let w_weak_refresh = w_weak.clone();
        let detected_monitors_refresh = self.detected_monitors.clone();
        window.on_refresh_displays(move || {
            tracing::info!("[SettingsDialog: Displays] User triggered display re-scan from Settings dialog");
            let monitors = SystemDisplayService::detect_monitors();
            let mut options = Vec::with_capacity(monitors.len());
            for m in &monitors {
                options.push(slint::SharedString::from(m.display_name()));
            }

            if options.is_empty() {
                options.push(slint::SharedString::from("No displays detected"));
            }

            if let Some(w) = w_weak_refresh.upgrade() {
                let cur_idx = w.get_selected_display_idx().saturating_as::<usize>();
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
                w.set_selected_display_idx(new_idx.saturating_as::<i32>());
            }
        });

        let w_weak_save = w_weak.clone();
        let mgr_save = manager;
        let main_weak_save = main_weak.clone();
        let detected_monitors_save = self.detected_monitors.clone();

        // Every change applies a moment after the edit (the window debounces it), and once more when it closes.
        let apply_ctx = apply::ApplyContext {
            mgr: Arc::clone(&mgr_save),
            formats: Arc::clone(&formats_page_save),
            hotkeys: Rc::clone(&hotkeys_page_save),
            monitors: Rc::clone(&detected_monitors_save),
            debug: Rc::clone(&self.debug_page),
        };
        let close_ctx = apply_ctx.clone();
        let reset_ctx = apply_ctx.clone();
        let w_weak_reset = w_weak.clone();
        window.on_reset_page(move |page| {
            let Some(w) = w_weak_reset.upgrade() else {
                return;
            };
            let Ok(page) = usize::try_from(page) else {
                return;
            };
            // What the window shows is saved first, so only this page changes.
            Self::apply_from_window(&w, &reset_ctx);
            let mut settings = reset_ctx.mgr.get();
            if rustview_core::settings::page_reset::reset_page(&mut settings, page) {
                tracing::info!(page, "[SettingsDialog: Reset] Page reset to its defaults");
                reset_ctx.mgr.update(&settings);
                Self::fill(&reset_ctx, &w, &reset_ctx.mgr.get());
            }
        });
        let w_weak_apply = w_weak.clone();
        window.on_apply_now(move || {
            if let Some(w) = w_weak_apply.upgrade() {
                tracing::debug!("[SettingsDialog: Apply] Changes applied at once");
                Self::apply_from_window(&w, &apply_ctx);
            }
        });
        let w_weak_close_apply = w_weak.clone();
        let save_ctx = close_ctx.clone();

        window.on_save_and_apply(move || {
            if let Some(w) = w_weak_save.upgrade() {
                Self::apply_from_window(&w, &save_ctx);

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
                &PickerRequest {
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
            tracing::info!("[SettingsDialog: CloseRequest] Settings window closed via titlebar close button");
            if let Some(w) = w_weak_close_apply.upgrade() {
                Self::apply_from_window(&w, &close_ctx);
            }
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
