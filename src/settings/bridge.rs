use super::registry::SettingsManager;
use super::schema::{AppSettings, GridAspectRatio};
use crate::ui::{MainWindow, SettingsWindow};
use slint::{CloseRequestResponse, ComponentHandle};
use std::path::PathBuf;
use std::sync::Arc;

pub struct SettingsDialogController {
    settings_window: Option<SettingsWindow>,
    manager: Arc<SettingsManager>,
    main_window_weak: Option<slint::Weak<MainWindow>>,
}

impl SettingsDialogController {
    pub fn new(manager: Arc<SettingsManager>) -> Self {
        Self {
            settings_window: None,
            manager,
            main_window_weak: None,
        }
    }

    pub fn set_main_window(&mut self, weak: slint::Weak<MainWindow>) {
        self.main_window_weak = Some(weak);
    }

    pub fn show(&mut self) {
        if self.settings_window.is_none() {
            let window = SettingsWindow::new().expect("Failed to create SettingsWindow");
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
        window.set_root_folder_path(
            settings
                .general
                .root_folder
                .as_ref()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default()
                .into(),
        );
        window.set_remember_last_folder(settings.general.remember_last_folder);
        window.set_confirm_delete(settings.general.confirm_on_file_delete);

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
        window.set_enable_hover_previews(settings.appearance.enable_hover_previews);
        window.set_enable_tooltips(settings.appearance.enable_tooltips);
        window.set_tooltip_delay_ms(settings.appearance.tooltip_delay_ms as i32);

        // Performance properties
        window.set_worker_threads(settings.performance.worker_threads as i32);
        window.set_batch_commit_size(settings.performance.batch_commit_size as i32);
        window.set_l1_cache_slots(settings.performance.l1_cache_slots as i32);
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

        window.set_autoplay_videos(settings.player.autoplay_videos);
        window.set_default_volume(settings.player.default_volume);

        // Debug properties
        window.set_obfuscate_logs(settings.debug.obfuscate_logs);
    }

    fn register_callbacks(&self, window: &SettingsWindow) {
        let w_weak = window.as_weak();
        let manager = self.manager.clone();
        let main_weak = self.main_window_weak.clone();

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

        let w_weak_save = w_weak.clone();
        let mgr_save = manager;
        let main_weak_save = main_weak.clone();
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
                current.appearance.enable_hover_previews = w.get_enable_hover_previews();
                current.appearance.enable_tooltips = w.get_enable_tooltips();
                current.appearance.tooltip_delay_ms =
                    w.get_tooltip_delay_ms().clamp(50, 2000) as u32;

                // Save Performance Parameters
                current.performance.worker_threads = w.get_worker_threads().clamp(1, 32) as usize;
                current.performance.batch_commit_size =
                    w.get_batch_commit_size().clamp(4, 256) as usize;
                current.performance.l1_cache_slots =
                    w.get_l1_cache_slots().clamp(100, 8000) as usize;
                current.performance.thumbnail_db_size =
                    w.get_thumb_db_size().clamp(64, 1024) as u32;
                current.performance.thumbnail_format = if w.get_thumb_format_idx() == 1 {
                    "JPEG".to_string()
                } else {
                    "WebP".to_string()
                };
                current.performance.thumbnail_quality =
                    w.get_thumb_quality().clamp(10, 100) as u8;
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
                current.performance.generate_video_previews = w.get_generate_video_previews();
                current.performance.preview_frame_count =
                    w.get_preview_frame_count().clamp(2, 100) as u32;
                current.performance.custom_preview_settings = w.get_custom_preview_settings();
                current.performance.preview_format = if w.get_preview_format_idx() == 1 {
                    "JPEG".to_string()
                } else {
                    "WebP".to_string()
                };
                current.performance.preview_quality =
                    w.get_preview_quality().clamp(10, 100) as u8;

                current.player.autoplay_videos = w.get_autoplay_videos();
                current.player.default_volume = w.get_default_volume();

                // Save Debug Parameters
                current.debug.obfuscate_logs = w.get_obfuscate_logs();

                mgr_save.update(current);

                if let Some(ref mw) = main_weak_save
                    && let Some(main) = mw.upgrade()
                {
                    main.set_is_modal_locked(false);
                }

                let _ = w.hide();
            }
        });

        let w_weak_cancel = w_weak;
        let main_weak_cancel = main_weak;
        window.on_cancel_clicked(move || {
            if let Some(w) = w_weak_cancel.upgrade() {
                if let Some(ref mw) = main_weak_cancel
                    && let Some(main) = mw.upgrade()
                {
                    main.set_is_modal_locked(false);
                }
                let _ = w.hide();
            }
        });

        let main_weak_close = self.main_window_weak.clone();
        window.window().on_close_requested(move || {
            if let Some(ref mw) = main_weak_close
                && let Some(main) = mw.upgrade()
            {
                main.set_is_modal_locked(false);
            }
            CloseRequestResponse::HideWindow
        });
    }
}
