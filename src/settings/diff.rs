use super::obscurer::LogObscurer;
use super::schema::AppSettings;

/// Granular, domain-specific delta representation between two `AppSettings` states.
/// Enables targeted subsystem updates without triggering unnecessary re-indexing or view reloads.
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
    pub hover_previews_changed: bool,

    // Performance & Pipeline Subsystem
    pub performance_cache_changed: bool,
    pub performance_pipeline_changed: bool,

    // View & Layout Subsystem
    pub view_settings_changed: bool,

    // Video Player Subsystem
    pub player_settings_changed: bool,

    // Tags Subsystem
    pub tags_settings_changed: bool,
    pub tag_expansion_changed: bool,

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
            || old.general.deletion_mode != new.general.deletion_mode;

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
            || old.appearance.dark_theme != new.appearance.dark_theme;

        let tooltip_settings_changed = old.appearance.enable_tooltips != new.appearance.enable_tooltips
            || old.appearance.tooltip_delay_ms != new.appearance.tooltip_delay_ms;

        let hover_previews_changed = old.appearance.enable_hover_previews != new.appearance.enable_hover_previews;

        let performance_cache_changed = old.performance.l1_cache_slots != new.performance.l1_cache_slots;

        let performance_pipeline_changed = old.performance.worker_threads != new.performance.worker_threads
            || old.performance.thumbnail_db_size != new.performance.thumbnail_db_size
            || old.performance.thumbnail_format != new.performance.thumbnail_format
            || old.performance.thumbnail_quality != new.performance.thumbnail_quality
            || old.performance.scaling_algorithm != new.performance.scaling_algorithm
            || old.performance.batch_commit_size != new.performance.batch_commit_size
            || old.performance.video_engine != new.performance.video_engine
            || old.performance.video_offset_percent != new.performance.video_offset_percent
            || old.performance.generate_video_previews != new.performance.generate_video_previews
            || old.performance.preview_frame_count != new.performance.preview_frame_count
            || old.performance.custom_preview_settings != new.performance.custom_preview_settings
            || old.performance.preview_format != new.performance.preview_format
            || old.performance.preview_quality != new.performance.preview_quality;

        let view_settings_changed = old.view != new.view;
        let player_settings_changed = old.player != new.player;
        let tags_settings_changed = old.tags.default_match_mode != new.tags.default_match_mode
            || old.tags.default_scope_mode != new.tags.default_scope_mode;
        let tag_expansion_changed = old.tags.expanded_tag_ids != new.tags.expanded_tag_ids;
        let debug_settings_changed = old.debug != new.debug;
        let window_geometry_changed = old.window != new.window;

        let has_changes = root_folder_changed
            || general_behavior_changed
            || folder_tree_presentation_changed
            || folder_tree_behavior_changed
            || folder_expansion_changed
            || appearance_grid_changed
            || tooltip_settings_changed
            || hover_previews_changed
            || performance_cache_changed
            || performance_pipeline_changed
            || view_settings_changed
            || player_settings_changed
            || tags_settings_changed
            || tag_expansion_changed
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
            hover_previews_changed,
            performance_cache_changed,
            performance_pipeline_changed,
            view_settings_changed,
            player_settings_changed,
            tags_settings_changed,
            tag_expansion_changed,
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

        if old.performance.worker_threads != new.performance.worker_threads {
            changes.push(format!(
                "Performance: worker_threads ({} -> {})",
                old.performance.worker_threads, new.performance.worker_threads
            ));
        }
        if old.performance.l1_cache_slots != new.performance.l1_cache_slots {
            changes.push(format!(
                "Performance: l1_cache_slots ({} -> {})",
                old.performance.l1_cache_slots, new.performance.l1_cache_slots
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

        if old.debug.obfuscate_logs != new.debug.obfuscate_logs {
            changes.push(format!(
                "Debug: obfuscate_logs ({} -> {})",
                old.debug.obfuscate_logs, new.debug.obfuscate_logs
            ));
        }

        changes
    }
}
