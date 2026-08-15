use super::registry::SettingsManager;
use super::schema::AppSettings;
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

    /// Displays the SettingsWindow and locks the MainWindow interaction.
    pub fn show(&mut self) {
        if self.settings_window.is_none() {
            let window = SettingsWindow::new().expect("Failed to create SettingsWindow");
            self.register_callbacks(&window);
            self.settings_window = Some(window);
        }

        if let Some(ref window) = self.settings_window {
            self.populate_ui(window, &self.manager.get());

            // Lock main window
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

        window.set_default_thumb_size(settings.appearance.default_thumbnail_size as i32);
        window.set_show_filenames(settings.appearance.show_filenames);
        window.set_show_rating_badge(settings.appearance.show_ratings_overlay);
        window.set_show_tag_badge(settings.appearance.show_tags_overlay);

        window.set_l1_cache_slots(settings.performance.l1_cache_slots as i32);
        window.set_worker_threads(settings.performance.worker_threads as i32);

        window.set_autoplay_videos(settings.player.autoplay_videos);
        window.set_default_volume(settings.player.default_volume);
    }

    fn register_callbacks(&self, window: &SettingsWindow) {
        let w_weak = window.as_weak();
        let manager = self.manager.clone();
        let main_weak = self.main_window_weak.clone();

        // BROWSE ROOT DIRECTORY
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

        // SAVE AND APPLY
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

                current.appearance.default_thumbnail_size = w.get_default_thumb_size() as u32;
                current.appearance.show_filenames = w.get_show_filenames();
                current.appearance.show_ratings_overlay = w.get_show_rating_badge();
                current.appearance.show_tags_overlay = w.get_show_tag_badge();

                current.performance.l1_cache_slots = w.get_l1_cache_slots() as usize;
                current.performance.worker_threads = w.get_worker_threads() as usize;

                current.player.autoplay_videos = w.get_autoplay_videos();
                current.player.default_volume = w.get_default_volume();

                mgr_save.update(current);

                // Unlock main window
                if let Some(ref mw) = main_weak_save
                    && let Some(main) = mw.upgrade()
                {
                    main.set_is_modal_locked(false);
                }

                let _ = w.hide();
            }
        });

        // CANCEL
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

        // WINDOW CLOSE REQUEST (X on Titlebar)
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
