use super::obscurer::LogObscurer;
use super::schema::AppSettings;

/// Granular, domain-specific delta representation between two `AppSettings` states.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsDiff {
    pub has_changes: bool,

    // General Subsystem
    pub root_folder_changed: bool,
    pub general_behavior_changed: bool,

    // Folders Subsystem
    pub folder_tree_presentation_changed: bool,
    pub folder_tree_behavior_changed: bool,
    pub folder_expansion_changed: bool,

    // Appearance & Grid Subsystem
    pub appearance_grid_changed: bool,
    pub tooltip_settings_changed: bool,
    pub type_ahead_changed: bool,
    pub hover_previews_changed: bool,

    // Performance & Pipeline Subsystem
    pub performance_cache_changed: bool,
    pub performance_pipeline_changed: bool,

    // View & Layout Subsystem
    pub view_settings_changed: bool,

    // Formats & Tools Subsystem
    pub formats_settings_changed: bool,
    /// The set of enabled formats changed (files and records of other formats appear or disappear).
    pub formats_policy_changed: bool,

    // Viewer Subsystem
    pub viewer_settings_changed: bool,

    // Fullscreen Subsystem
    pub fullscreen_settings_changed: bool,

    // Tags Subsystem
    pub tags_settings_changed: bool,
    pub tag_expansion_changed: bool,

    // API Subsystem
    pub api_settings_changed: bool,

    // Debug & Logging Subsystem
    pub debug_settings_changed: bool,

    // Window Geometry
    pub window_geometry_changed: bool,
}

impl SettingsDiff {
    /// Computes an exact, granular diff between old and new application settings.
    pub fn compute(old: &AppSettings, new: &AppSettings) -> Self {
        let root_folder_changed = old.general.root_folder != new.general.root_folder;

        let general_behavior_changed = old.general.remember_last_folder != new.general.remember_last_folder
            || old.general.confirm_on_file_delete != new.general.confirm_on_file_delete
            || old.general.confirm_open_file_count != new.general.confirm_open_file_count
            || old.general.deletion_mode != new.general.deletion_mode
            || old.general.ignore_nested_mounts != new.general.ignore_nested_mounts;

        let folder_tree_presentation_changed = old.folders.show_deleted_folders != new.folders.show_deleted_folders
            || old.folders.show_folder_metrics != new.folders.show_folder_metrics
            || old.folders.show_hidden_folders != new.folders.show_hidden_folders
            || old.folders.support_dot_hidden_files != new.folders.support_dot_hidden_files;

        let folder_tree_behavior_changed = old.folders.auto_reveal_remembered_folder != new.folders.auto_reveal_remembered_folder
            || old.folders.persist_folder_tree_state != new.folders.persist_folder_tree_state;

        let folder_expansion_changed = old.folders.expanded_folder_ids != new.folders.expanded_folder_ids;

        let appearance_grid_changed = old.appearance.default_thumbnail_size != new.appearance.default_thumbnail_size
            || old.appearance.thumbnail_aspect_ratio != new.appearance.thumbnail_aspect_ratio
            || old.appearance.show_filenames != new.appearance.show_filenames
            || old.appearance.show_ratings_overlay != new.appearance.show_ratings_overlay
            || old.appearance.show_tags_overlay != new.appearance.show_tags_overlay
            || old.appearance.show_media_type_badges != new.appearance.show_media_type_badges
            || old.appearance.show_animated_badge != new.appearance.show_animated_badge
            || old.appearance.show_duration_badge != new.appearance.show_duration_badge
            || old.appearance.thumbnail_badge_overflow != new.appearance.thumbnail_badge_overflow
            || old.appearance.dark_theme != new.appearance.dark_theme
            || old.appearance.grid_item_padding_horizontal != new.appearance.grid_item_padding_horizontal
            || old.appearance.grid_item_padding_vertical != new.appearance.grid_item_padding_vertical
            || old.appearance.grid_spacing != new.appearance.grid_spacing
            || old.appearance.grid_padding != new.appearance.grid_padding
            || old.appearance.grid_item_border_width != new.appearance.grid_item_border_width
            || old.appearance.grid_item_corner_radius != new.appearance.grid_item_corner_radius;

        let tooltip_settings_changed = old.appearance.enable_tooltips != new.appearance.enable_tooltips
            || old.appearance.tooltip_delay_ms != new.appearance.tooltip_delay_ms;

        let type_ahead_changed = old.appearance.type_ahead_enabled != new.appearance.type_ahead_enabled
            || old.appearance.type_ahead_timeout_ms != new.appearance.type_ahead_timeout_ms
            || old.appearance.type_ahead_new_search_gap_ms != new.appearance.type_ahead_new_search_gap_ms
            || old.appearance.type_ahead_include_folders != new.appearance.type_ahead_include_folders
            || old.appearance.type_ahead_cycle_repeated_keys != new.appearance.type_ahead_cycle_repeated_keys
            || old.appearance.type_ahead_center_target != new.appearance.type_ahead_center_target
            || old.appearance.type_ahead_show_no_match != new.appearance.type_ahead_show_no_match
            || old.appearance.type_ahead_multiview_delay_ms != new.appearance.type_ahead_multiview_delay_ms;

        let hover_previews_changed = old.appearance.enable_hover_previews != new.appearance.enable_hover_previews;

        let performance_cache_changed = old.performance.memory_budget_mb != new.performance.memory_budget_mb;

        let performance_pipeline_changed = old.performance.worker_threads != new.performance.worker_threads
            || old.performance.thumbnail_db_size != new.performance.thumbnail_db_size
            || old.performance.thumbnail_format != new.performance.thumbnail_format
            || old.performance.thumbnail_quality != new.performance.thumbnail_quality
            || old.performance.scaling_algorithm != new.performance.scaling_algorithm
            || old.performance.batch_commit_size != new.performance.batch_commit_size
            || old.performance.video_engine != new.performance.video_engine
            || old.performance.video_offset_percent != new.performance.video_offset_percent
            || old.performance.short_video_threshold_secs != new.performance.short_video_threshold_secs
            || old.performance.generate_video_previews != new.performance.generate_video_previews
            || old.performance.preview_frame_count != new.performance.preview_frame_count
            || old.performance.custom_preview_settings != new.performance.custom_preview_settings
            || old.performance.preview_format != new.performance.preview_format
            || old.performance.preview_quality != new.performance.preview_quality;

        let view_settings_changed = old.view != new.view;
        let formats_settings_changed = old.formats != new.formats;
        let formats_policy_changed = old.formats.format_mask() != new.formats.format_mask();
        let viewer_settings_changed = old.viewer != new.viewer;
        let fullscreen_settings_changed = old.fullscreen != new.fullscreen;
        let tags_settings_changed = old.tags.default_match_mode != new.tags.default_match_mode
            || old.tags.default_scope_mode != new.tags.default_scope_mode
            || old.tags.max_history_tags != new.tags.max_history_tags
            || old.tags.is_history_collapsed != new.tags.is_history_collapsed
            || old.tags.history_tag_ids != new.tags.history_tag_ids
            || (old.tags.history_height - new.tags.history_height).abs() > 0.05;
        let tag_expansion_changed = old.tags.expanded_tag_ids != new.tags.expanded_tag_ids;
        let api_settings_changed = old.api != new.api;
        let debug_settings_changed = old.debug != new.debug;
        let window_geometry_changed = old.window != new.window;

        let has_changes = root_folder_changed
            || general_behavior_changed
            || folder_tree_presentation_changed
            || folder_tree_behavior_changed
            || folder_expansion_changed
            || appearance_grid_changed
            || tooltip_settings_changed
            || type_ahead_changed
            || hover_previews_changed
            || performance_cache_changed
            || performance_pipeline_changed
            || view_settings_changed
            || formats_settings_changed
            || viewer_settings_changed
            || fullscreen_settings_changed
            || tags_settings_changed
            || tag_expansion_changed
            || api_settings_changed
            || debug_settings_changed
            || window_geometry_changed;

        Self {
            has_changes,
            root_folder_changed,
            general_behavior_changed,
            folder_tree_presentation_changed,
            folder_tree_behavior_changed,
            folder_expansion_changed,
            appearance_grid_changed,
            tooltip_settings_changed,
            type_ahead_changed,
            hover_previews_changed,
            performance_cache_changed,
            performance_pipeline_changed,
            view_settings_changed,
            formats_settings_changed,
            formats_policy_changed,
            viewer_settings_changed,
            fullscreen_settings_changed,
            tags_settings_changed,
            tag_expansion_changed,
            api_settings_changed,
            debug_settings_changed,
            window_geometry_changed,
        }
    }

    /// Generates human-readable descriptions of all modified settings for diagnostics and structured logging.
    pub fn detailed_summary(&self, old: &AppSettings, new: &AppSettings) -> Vec<String> {
        let mut changes = Vec::new();

        if self.root_folder_changed {
            changes.push(format!(
                "General: root_folder ({} -> {})",
                LogObscurer::path_opt(old.general.root_folder.as_ref()),
                LogObscurer::path_opt(new.general.root_folder.as_ref())
            ));
        }
        if old.general.remember_last_folder != new.general.remember_last_folder {
            changes.push(format!(
                "General: remember_last_folder ({} -> {})",
                old.general.remember_last_folder, new.general.remember_last_folder
            ));
        }
        if old.general.confirm_on_file_delete != new.general.confirm_on_file_delete {
            changes.push(format!(
                "General: confirm_on_file_delete ({} -> {})",
                old.general.confirm_on_file_delete, new.general.confirm_on_file_delete
            ));
        }
        if old.general.confirm_open_file_count != new.general.confirm_open_file_count {
            changes.push(format!(
                "General: confirm_open_file_count ({} -> {})",
                old.general.confirm_open_file_count, new.general.confirm_open_file_count
            ));
        }
        if old.general.ignore_nested_mounts != new.general.ignore_nested_mounts {
            changes.push(format!(
                "General: ignore_nested_mounts ({} -> {})",
                old.general.ignore_nested_mounts, new.general.ignore_nested_mounts
            ));
        }

        if old.folders.show_deleted_folders != new.folders.show_deleted_folders {
            changes.push(format!(
                "Folders: show_deleted_folders ({} -> {})",
                old.folders.show_deleted_folders, new.folders.show_deleted_folders
            ));
        }
        if old.folders.show_folder_metrics != new.folders.show_folder_metrics {
            changes.push(format!(
                "Folders: show_folder_metrics ({} -> {})",
                old.folders.show_folder_metrics, new.folders.show_folder_metrics
            ));
        }
        if old.folders.show_hidden_folders != new.folders.show_hidden_folders {
            changes.push(format!(
                "Folders: show_hidden_folders ({} -> {})",
                old.folders.show_hidden_folders, new.folders.show_hidden_folders
            ));
        }
        if old.folders.support_dot_hidden_files != new.folders.support_dot_hidden_files {
            changes.push(format!(
                "Folders: support_dot_hidden_files ({} -> {})",
                old.folders.support_dot_hidden_files, new.folders.support_dot_hidden_files
            ));
        }

        if old.appearance.default_thumbnail_size != new.appearance.default_thumbnail_size {
            changes.push(format!(
                "Appearance: default_thumbnail_size ({} -> {})",
                old.appearance.default_thumbnail_size, new.appearance.default_thumbnail_size
            ));
        }
        if old.appearance.thumbnail_aspect_ratio != new.appearance.thumbnail_aspect_ratio {
            changes.push(format!(
                "Appearance: thumbnail_aspect_ratio ({} -> {})",
                old.appearance.thumbnail_aspect_ratio, new.appearance.thumbnail_aspect_ratio
            ));
        }
        if old.appearance.show_filenames != new.appearance.show_filenames {
            changes.push(format!(
                "Appearance: show_filenames ({} -> {})",
                old.appearance.show_filenames, new.appearance.show_filenames
            ));
        }
        if old.appearance.show_ratings_overlay != new.appearance.show_ratings_overlay {
            changes.push(format!(
                "Appearance: show_ratings_overlay ({} -> {})",
                old.appearance.show_ratings_overlay, new.appearance.show_ratings_overlay
            ));
        }
        if old.appearance.show_tags_overlay != new.appearance.show_tags_overlay {
            changes.push(format!(
                "Appearance: show_tags_overlay ({} -> {})",
                old.appearance.show_tags_overlay, new.appearance.show_tags_overlay
            ));
        }
        if old.appearance.show_animated_badge != new.appearance.show_animated_badge {
            changes.push(format!(
                "Appearance: show_animated_badge ({} -> {})",
                old.appearance.show_animated_badge, new.appearance.show_animated_badge
            ));
        }

        if old.appearance.show_media_type_badges != new.appearance.show_media_type_badges {
            changes.push(format!(
                "Appearance: show_media_type_badges ({} -> {})",
                old.appearance.show_media_type_badges, new.appearance.show_media_type_badges
            ));
        }
        if old.appearance.show_duration_badge != new.appearance.show_duration_badge {
            changes.push(format!(
                "Appearance: show_duration_badge ({} -> {})",
                old.appearance.show_duration_badge, new.appearance.show_duration_badge
            ));
        }
        if old.appearance.thumbnail_badge_overflow != new.appearance.thumbnail_badge_overflow {
            changes.push(format!(
                "Appearance: thumbnail_badge_overflow ({} -> {})",
                old.appearance.thumbnail_badge_overflow, new.appearance.thumbnail_badge_overflow
            ));
        }
        if old.appearance.grid_item_padding_horizontal != new.appearance.grid_item_padding_horizontal {
            changes.push(format!(
                "Appearance: grid_item_padding_horizontal ({}px -> {}px)",
                old.appearance.grid_item_padding_horizontal, new.appearance.grid_item_padding_horizontal
            ));
        }
        if old.appearance.grid_item_padding_vertical != new.appearance.grid_item_padding_vertical {
            changes.push(format!(
                "Appearance: grid_item_padding_vertical ({}px -> {}px)",
                old.appearance.grid_item_padding_vertical, new.appearance.grid_item_padding_vertical
            ));
        }
        if old.appearance.grid_spacing != new.appearance.grid_spacing {
            changes.push(format!(
                "Appearance: grid_spacing ({}px -> {}px)",
                old.appearance.grid_spacing, new.appearance.grid_spacing
            ));
        }
        if old.appearance.grid_padding != new.appearance.grid_padding {
            changes.push(format!(
                "Appearance: grid_padding ({}px -> {}px)",
                old.appearance.grid_padding, new.appearance.grid_padding
            ));
        }
        if old.appearance.grid_item_border_width != new.appearance.grid_item_border_width {
            changes.push(format!(
                "Appearance: grid_item_border_width ({}px -> {}px)",
                old.appearance.grid_item_border_width, new.appearance.grid_item_border_width
            ));
        }
        if old.appearance.grid_item_corner_radius != new.appearance.grid_item_corner_radius {
            changes.push(format!(
                "Appearance: grid_item_corner_radius ({}px -> {}px)",
                old.appearance.grid_item_corner_radius, new.appearance.grid_item_corner_radius
            ));
        }
        if old.appearance.enable_hover_previews != new.appearance.enable_hover_previews {
            changes.push(format!(
                "Appearance: enable_hover_previews ({} -> {})",
                old.appearance.enable_hover_previews, new.appearance.enable_hover_previews
            ));
        }
        if old.appearance.enable_tooltips != new.appearance.enable_tooltips {
            changes.push(format!(
                "Appearance: enable_tooltips ({} -> {})",
                old.appearance.enable_tooltips, new.appearance.enable_tooltips
            ));
        }
        if old.appearance.tooltip_delay_ms != new.appearance.tooltip_delay_ms {
            changes.push(format!(
                "Appearance: tooltip_delay_ms ({}ms -> {}ms)",
                old.appearance.tooltip_delay_ms, new.appearance.tooltip_delay_ms
            ));
        }
        if self.type_ahead_changed {
            changes.push(format!(
                "Appearance: type_ahead (enabled {} -> {}, timeout {}ms -> {}ms, new_search_gap {}ms -> {}ms, folders {} -> {}, cycle {} -> {}, center {} -> {}, no_match_message {} -> {}, multiview_delay {}ms -> {}ms)",
                old.appearance.type_ahead_enabled, new.appearance.type_ahead_enabled,
                old.appearance.type_ahead_timeout_ms, new.appearance.type_ahead_timeout_ms,
                old.appearance.type_ahead_new_search_gap_ms, new.appearance.type_ahead_new_search_gap_ms,
                old.appearance.type_ahead_include_folders, new.appearance.type_ahead_include_folders,
                old.appearance.type_ahead_cycle_repeated_keys, new.appearance.type_ahead_cycle_repeated_keys,
                old.appearance.type_ahead_center_target, new.appearance.type_ahead_center_target,
                old.appearance.type_ahead_show_no_match, new.appearance.type_ahead_show_no_match,
                old.appearance.type_ahead_multiview_delay_ms, new.appearance.type_ahead_multiview_delay_ms
            ));
        }

        if old.performance.worker_threads != new.performance.worker_threads {
            changes.push(format!(
                "Performance: worker_threads ({} -> {})",
                old.performance.worker_threads, new.performance.worker_threads
            ));
        }
        if old.performance.memory_budget_mb != new.performance.memory_budget_mb {
            changes.push(format!(
                "Performance: memory_budget_mb ({} -> {}; 0 = automatic)",
                old.performance.memory_budget_mb, new.performance.memory_budget_mb
            ));
        }
        if old.performance.thumbnail_db_size != new.performance.thumbnail_db_size {
            changes.push(format!(
                "Performance: thumbnail_db_size ({} -> {})",
                old.performance.thumbnail_db_size, new.performance.thumbnail_db_size
            ));
        }
        if old.performance.thumbnail_format != new.performance.thumbnail_format {
            changes.push(format!(
                "Performance: thumbnail_format ({} -> {})",
                old.performance.thumbnail_format, new.performance.thumbnail_format
            ));
        }
        if old.performance.thumbnail_quality != new.performance.thumbnail_quality {
            changes.push(format!(
                "Performance: thumbnail_quality ({} -> {})",
                old.performance.thumbnail_quality, new.performance.thumbnail_quality
            ));
        }
        if old.performance.scaling_algorithm != new.performance.scaling_algorithm {
            changes.push(format!(
                "Performance: scaling_algorithm ({} -> {})",
                old.performance.scaling_algorithm, new.performance.scaling_algorithm
            ));
        }
        if old.performance.video_engine != new.performance.video_engine {
            changes.push(format!(
                "Performance: video_engine ({} -> {})",
                old.performance.video_engine, new.performance.video_engine
            ));
        }
        if old.performance.video_offset_percent != new.performance.video_offset_percent {
            changes.push(format!(
                "Performance: video_offset_percent ({}% -> {}%)",
                old.performance.video_offset_percent, new.performance.video_offset_percent
            ));
        }
        if old.performance.short_video_threshold_secs != new.performance.short_video_threshold_secs {
            changes.push(format!(
                "Performance: short_video_threshold_secs ({}s -> {}s)",
                old.performance.short_video_threshold_secs, new.performance.short_video_threshold_secs
            ));
        }
        if old.performance.generate_video_previews != new.performance.generate_video_previews {
            changes.push(format!(
                "Performance: generate_video_previews ({} -> {})",
                old.performance.generate_video_previews, new.performance.generate_video_previews
            ));
        }
        if old.performance.preview_frame_count != new.performance.preview_frame_count {
            changes.push(format!(
                "Performance: preview_frame_count ({} -> {})",
                old.performance.preview_frame_count, new.performance.preview_frame_count
            ));
        }
        if old.performance.custom_preview_settings != new.performance.custom_preview_settings {
            changes.push(format!(
                "Performance: custom_preview_settings ({} -> {})",
                old.performance.custom_preview_settings, new.performance.custom_preview_settings
            ));
        }
        if old.performance.preview_format != new.performance.preview_format {
            changes.push(format!(
                "Performance: preview_format ({} -> {})",
                old.performance.preview_format, new.performance.preview_format
            ));
        }
        if old.performance.preview_quality != new.performance.preview_quality {
            changes.push(format!(
                "Performance: preview_quality ({}% -> {}%)",
                old.performance.preview_quality, new.performance.preview_quality
            ));
        }

        if old.view.media_type_filter != new.view.media_type_filter {
            changes.push(format!(
                "View: media_type_filter ({} -> {})",
                old.view.media_type_filter, new.view.media_type_filter
            ));
        }

        if old.view.is_inspector_collapsed != new.view.is_inspector_collapsed {
            changes.push(format!(
                "View: is_inspector_collapsed ({} -> {})",
                old.view.is_inspector_collapsed, new.view.is_inspector_collapsed
            ));
        }

        if old.view.tab_grid_columns != new.view.tab_grid_columns {
            changes.push(format!(
                "View: tab_grid_columns ({} -> {})",
                old.view.tab_grid_columns, new.view.tab_grid_columns
            ));
        }

        if old.view.tab_grid_auto_columns != new.view.tab_grid_auto_columns {
            changes.push(format!(
                "View: tab_grid_auto_columns ({} -> {})",
                old.view.tab_grid_auto_columns, new.view.tab_grid_auto_columns
            ));
        }

        if old.view.col_path_width != new.view.col_path_width {
            changes.push(format!(
                "View: col_path_width ({}px -> {}px)",
                old.view.col_path_width, new.view.col_path_width
            ));
        }

        if old.view.col_flags_width != new.view.col_flags_width {
            changes.push(format!(
                "View: col_flags_width ({}px -> {}px)",
                old.view.col_flags_width, new.view.col_flags_width
            ));
        }

        if old.view.left_sidebar_panel_tabs != new.view.left_sidebar_panel_tabs {
            changes.push(format!(
                "View: left_sidebar_panel_tabs ({:?} -> {:?})",
                old.view.left_sidebar_panel_tabs, new.view.left_sidebar_panel_tabs
            ));
        }

        if old.view.left_sidebar_active_panel_tab != new.view.left_sidebar_active_panel_tab {
            changes.push(format!(
                "View: left_sidebar_active_panel_tab ({} -> {})",
                old.view.left_sidebar_active_panel_tab, new.view.left_sidebar_active_panel_tab
            ));
        }

        if old.formats.auto_detect_extensionless_media != new.formats.auto_detect_extensionless_media {
            changes.push(format!(
                "Formats: auto_detect_extensionless_media ({} -> {})",
                old.formats.auto_detect_extensionless_media, new.formats.auto_detect_extensionless_media
            ));
        }

        if self.formats_policy_changed {
            let changes_to = old.formats.format_mask().changes_to(new.formats.format_mask());
            let named = |on: bool| {
                changes_to
                    .iter()
                    .filter(|(_, enabled)| *enabled == on)
                    .map(|(f, _)| f.spec().id)
                    .collect::<Vec<_>>()
            };
            changes.push(format!(
                "Formats: enabled {:?}, disabled {:?}",
                named(true),
                named(false)
            ));
        }

        if old.viewer.images.open_in_tab != new.viewer.images.open_in_tab {
            changes.push(format!(
                "Viewer: images.open_in_tab ({} -> {})",
                old.viewer.images.open_in_tab, new.viewer.images.open_in_tab
            ));
        }

        if old.viewer.images.allow_multiple_tabs != new.viewer.images.allow_multiple_tabs {
            changes.push(format!(
                "Viewer: images.allow_multiple_tabs ({} -> {})",
                old.viewer.images.allow_multiple_tabs, new.viewer.images.allow_multiple_tabs
            ));
        }

        if old.viewer.videos.open_in_tab != new.viewer.videos.open_in_tab {
            changes.push(format!(
                "Viewer: videos.open_in_tab ({} -> {})",
                old.viewer.videos.open_in_tab, new.viewer.videos.open_in_tab
            ));
        }

        if old.viewer.videos.allow_multiple_tabs != new.viewer.videos.allow_multiple_tabs {
            changes.push(format!(
                "Viewer: videos.allow_multiple_tabs ({} -> {})",
                old.viewer.videos.allow_multiple_tabs, new.viewer.videos.allow_multiple_tabs
            ));
        }

        if old.viewer.tabs.show_rating_badge != new.viewer.tabs.show_rating_badge {
            changes.push(format!(
                "Viewer: tabs.show_rating_badge ({} -> {})",
                old.viewer.tabs.show_rating_badge, new.viewer.tabs.show_rating_badge
            ));
        }

        if old.viewer.background_mode != new.viewer.background_mode {
            changes.push(format!(
                "Viewer: background_mode ({} -> {})",
                old.viewer.background_mode, new.viewer.background_mode
            ));
        }

        if self.fullscreen_settings_changed {
            changes.push(format!(
                "Fullscreen: specific_enabled: {}, specific_monitor: '{}', fallback_enabled: {}, fallback_target: '{}'",
                new.fullscreen.enable_specific_monitor,
                new.fullscreen.specific_monitor,
                new.fullscreen.enable_fallback_rule,
                new.fullscreen.fallback_target
            ));
            let bar = &new.fullscreen.media_bar;
            changes.push(format!(
                "Fullscreen media action bar: enabled: {}, mute: {}, sticky: {}, duplicate: {}, open_as_tab: {}, open_with: {}, zoom: {}, delete: {}, remove: {}, reveal_distance_px: {}",
                bar.enabled, bar.show_mute, bar.show_sticky, bar.show_duplicate, bar.show_open_as_tab, bar.show_open_with, bar.show_zoom, bar.show_delete,
                bar.show_remove, bar.reveal_distance_px
            ));
            let primary = &new.fullscreen.primary_bar;
            changes.push(format!(
                "Fullscreen primary action bar: enabled: {}, send_to_tab: {}, close: {}, mute_all: {}, play_pause_all: {}, reset_layout: {}, hide_cursor: {}, pin: {}, reveal_distance_px: {}, hide_idle_cursor: {}, idle_cursor_ms: {}",
                primary.enabled, primary.show_send_to_tab, primary.show_close, primary.show_mute_all, primary.show_play_pause_all,
                primary.show_reset_layout, primary.show_hide_cursor, primary.show_pin, primary.reveal_distance_px,
                primary.hide_idle_cursor, primary.idle_cursor_ms
            ));
            let zoom = &new.fullscreen.zoom;
            changes.push(format!(
                "Fullscreen image zoom: step_percent: {}, max_percent: {}, out_percent: {}, smooth: {}, minimap_position: '{}', minimap_always: {}, minimap_secs: {}, resample: '{}'",
                zoom.step_percent, zoom.max_percent, zoom.out_percent, zoom.smooth, zoom.minimap_position, zoom.minimap_always, zoom.minimap_secs,
                zoom.resample
            ));
            let sound = &new.fullscreen.sound;
            changes.push(format!(
                "Fullscreen sound: auto_control: {}, follows: '{}', hand_over: {}",
                sound.auto_control, sound.follows, sound.hand_over
            ));
            changes.push(format!(
                "Fullscreen background: source: '{}', color: '{}'",
                new.fullscreen.background.source, new.fullscreen.background.color
            ));
        }

        if old.tags.max_history_tags != new.tags.max_history_tags {
            changes.push(format!(
                "Tags: max_history_tags ({} -> {})",
                old.tags.max_history_tags, new.tags.max_history_tags
            ));
        }

        if old.tags.is_history_collapsed != new.tags.is_history_collapsed {
            changes.push(format!(
                "Tags: is_history_collapsed ({} -> {})",
                old.tags.is_history_collapsed, new.tags.is_history_collapsed
            ));
        }

        if (old.tags.history_height - new.tags.history_height).abs() > 0.05 {
            changes.push(format!(
                "Tags: history_height ({:.1}px -> {:.1}px)",
                old.tags.history_height, new.tags.history_height
            ));
        }

        if old.api.auto_complete_external_stubs != new.api.auto_complete_external_stubs {
            changes.push(format!(
                "API: auto_complete_external_stubs ({} -> {})",
                old.api.auto_complete_external_stubs, new.api.auto_complete_external_stubs
            ));
        }

        if old.debug.obfuscate_logs != new.debug.obfuscate_logs {
            changes.push(format!(
                "Debug: obfuscate_logs ({} -> {})",
                old.debug.obfuscate_logs, new.debug.obfuscate_logs
            ));
        }

        if self.window_geometry_changed {
            changes.push(format!(
                "Window: geometry (maximized: {} -> {}, size: {}x{} -> {}x{}, maximized size: {}x{} -> {}x{}, pos: {:?},{:?} -> {:?},{:?})",
                old.window.is_maximized, new.window.is_maximized,
                old.window.width, old.window.height,
                new.window.width, new.window.height,
                old.window.maximized_width, old.window.maximized_height,
                new.window.maximized_width, new.window.maximized_height,
                old.window.x, old.window.y,
                new.window.x, new.window.y
            ));
        }

        changes
    }
}
