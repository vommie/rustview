//! Callbacks of the main window that belong to no single feature: modal dismissal, tooltips, keyboard navigation, type-ahead, sidebar resizers.

use super::UiBridge;
use crate::ui::context_menu::{BridgeContextMenu, ContextMenuRegistration};
use crate::ui::favorites::BridgeFavorites;
use crate::ui::file_dialogs::FileRenameDialogController;
use crate::ui::folder_tree::BridgeFolders;
use crate::ui::media::BridgeMedia;
use crate::ui::rating_hotkey::{RatingHotkeyParams, rate as rate_by_key};
use crate::ui::selection::BridgeFileSelection;
use crate::ui::selection::keyboard::{KeyboardNavigateParams, handle_keyboard_navigate};
use crate::ui::state::LastActivePanel;
use crate::ui::tabs::BridgeViewerTabs;
use crate::ui::tabs::grid_bridge::BridgeTabGrid;
use crate::ui::tags::BridgeTags;
use crate::ui::tags::history_bridge::BridgeTagHistory;
use crate::ui::type_ahead::{TypeAheadParams, TypeAheadState, handle_type_ahead_key};
use parking_lot::Mutex;
use rustview_core::util::cast::{Az, SaturatingAs, WrappingAs};

use slint::ComponentHandle;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

impl UiBridge {
    pub(super) fn register_subsystem_callbacks(&self) {
        BridgeViewerTabs::register_callbacks(
            &self.window,
            self.tab_manager.clone(),
            &self.library.clone(),
            self.thumbnails.clone(),
            self.settings.clone(),
            &self.state.clone(),
            self.fullscreen.clone(),
        );

        BridgeFolders::register_callbacks(
            &self.window,
            self.library.clone(),
            &self.thumbnails.clone(),
            &self.settings.clone(),
            &self.state.clone(),
            self.settings_dialog.clone(),
        );

        {
            // The sidebars are the dock (`ui::dock`); a change of it changes the room of the view in the center.
            let window = self.window.clone_strong();
            let tab_manager = self.tab_manager.clone();
            let fullscreen = self.fullscreen.clone();
            let settings = self.settings.clone();
            let state = self.state.clone();
            crate::ui::dock::install(&self.window, &self.settings, move || {
                if window.get_active_view_mode() == 1 {
                    BridgeViewerTabs::sync_video_embedding(&window, &tab_manager, &fullscreen, &settings);
                } else if window.get_active_view_mode() == 2 {
                    BridgeTabGrid::sync_tab_grid_ui(&window, &tab_manager, &settings, &state);
                }
            });
        }

        BridgeFileSelection::register_callbacks(
            &self.window,
            self.library.clone(),
            self.settings.clone(),
            &self.tab_manager.clone(),
            self.state.clone(),
            self.last_click.clone(),
            self.fullscreen.clone(),
        );

        BridgeContextMenu::register_callbacks(
            &self.window,
            ContextMenuRegistration {
                library: self.library.clone(),
                thumbnails: self.thumbnails.clone(),
                tab_manager: self.tab_manager.clone(),
                favorites: self.favorites.clone(),
                state: self.state.clone(),
                dispatcher: self.ctx_menu_dispatcher.clone(),
                rename_dialog: self.rename_dialog.clone(),
                favorite_rename_dialog: self.favorite_rename_dialog.clone(),
                missing_file_dialog: self.missing_file_dialog.clone(),
                delete_file_dialog: self.delete_file_dialog.clone(),
                remove_orphaned_dialog: self.remove_orphaned_dialog.clone(),
                conflict_dialog: self.conflict_dialog.clone(),
                tag_dialogs: self.tag_dialogs.clone(),
                folder_dialogs: self.folder_dialogs.clone(),
                fullscreen: self.fullscreen.clone(),
                settings: self.settings.clone(),
            },
        );

        BridgeTags::register_callbacks(
            &self.window,
            self.library.clone(),
            &self.thumbnails.clone(),
            &self.tab_manager.clone(),
            &self.settings.clone(),
            self.state.clone(),
        );

        let win_th_click = self.window.clone_strong();
        let lib_th_click = self.library.clone();
        let thumb_th_click = self.thumbnails.clone();
        let tm_th_click = self.tab_manager.clone();
        let set_th_click = self.settings.clone();
        let st_th_click = self.state.clone();
        self.window.on_tag_history_tag_clicked(move |tag_id| {
            BridgeTagHistory::handle_tag_history_clicked(
                tag_id.wrapping_as::<u32>(),
                &win_th_click,
                &lib_th_click,
                &thumb_th_click,
                &tm_th_click,
                &set_th_click,
                &st_th_click,
            );
        });

        let win_th_geom = self.window.clone_strong();
        let lib_th_geom = self.library.clone();
        let st_th_geom = self.state.clone();
        self.window.on_tag_history_geometry_changed(move |avail_w| {
            BridgeTagHistory::handle_geometry_changed(avail_w, &win_th_geom, &lib_th_geom, &st_th_geom);
        });

        BridgeMedia::register_query_callbacks(
            &self.window,
            &self.library.clone(),
            self.thumbnails.clone(),
            &self.tab_manager.clone(),
            self.settings.clone(),
            self.state.clone(),
        );

        let win_fav = self.window.clone_strong();
        let lib_fav = self.library.clone();
        let thumb_fav = self.thumbnails.clone();
        let set_fav = self.settings.clone();
        let st_fav = self.state.clone();
        self.window.on_favorite_selected(move |path_str| {
            st_fav.write().last_active_panel = LastActivePanel::Favorite;
            BridgeFavorites::handle_favorite_clicked(
                path_str.as_str(),
                &win_fav,
                &lib_fav,
                &thumb_fav,
                &set_fav,
                &st_fav,
            );
        });

        self.register_rename_callbacks();
        self.register_delete_callbacks();
        self.register_browse_folder_callbacks();
        self.register_keyboard_navigation_callbacks();
        self.register_type_ahead_callbacks();
        self.register_rating_hotkey_callbacks();
        self.register_tab_grid_callbacks();
        self.register_dnd_callbacks();
        self.register_rubberband_callbacks();
        self.register_sidebar_resize_callbacks();
        self.register_modal_dismiss_callback();
    }

    pub(super) fn register_modal_dismiss_callback(&self) {
        let fs_ctrl = self.fullscreen.clone();
        let ren_dlg = self.rename_dialog.clone();
        let fav_ren_dlg = self.favorite_rename_dialog.clone();
        let set_dlg = self.settings_dialog.clone();
        let miss_dlg = self.missing_file_dialog.clone();
        let del_dlg = self.delete_file_dialog.clone();
        let orph_dlg = self.remove_orphaned_dialog.clone();
        let conf_dlg = self.conflict_dialog.clone();
        let fld_dlg = self.folder_dialogs.clone();
        let tag_dlg = self.tag_dialogs.clone();
        let win = self.window.clone_strong();
        let st = self.state.clone();

        self.window.on_modal_dismiss_requested(move || {
            tracing::info!(
                "[UiBridge: ModalDismiss] Main UI modal dismiss requested (backdrop clicked or Escape pressed during modal lock)"
            );
            // An open dialog belongs to its parent window (X11): the click on the locked parent brings it to the front instead
            // of closing it. Without a window manager parent (Wayland) the dialogs are closed as before.
            if crate::ui::dialog_parent::raise_open() {
                return;
            }
            if fs_ctrl.is_isolated_open() {
                fs_ctrl.request_close();
            }
            FileRenameDialogController::close(&ren_dlg, &st, &win, &fs_ctrl);
            fav_ren_dlg.close(&win);
            set_dlg.borrow().close();
            miss_dlg.close(&win);
            del_dlg.close(&win);
            orph_dlg.close(&win);
            conf_dlg.close(&win);
            fld_dlg.close_rename(&win);
            fld_dlg.close_delete(&win);
            fld_dlg.close_missing_delete(&win);
            fld_dlg.close_move(&win);
            fld_dlg.close_remove_orphaned(&win);
            fld_dlg.close_reborn_warn(&win);
            tag_dlg.close_add(&win);
            tag_dlg.close_rename(&win);
            tag_dlg.close_delete(&win);
            win.set_is_modal_locked(false);
        });
    }

    pub(super) fn register_keyboard_navigation_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let fs = self.fullscreen.clone();

        self.window
            .on_media_navigate(move |direction_code, is_shift, page_rows| {
                handle_keyboard_navigate(&KeyboardNavigateParams {
                    direction_code,
                    is_shift,
                    page_rows,
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    settings: &set,
                    state: &st,
                    fullscreen: &fs,
                });
            });
    }

    pub(super) fn register_type_ahead_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let search: TypeAheadState = TypeAheadState::default();

        self.window.on_type_ahead_key(move |text, command| {
            handle_type_ahead_key(&TypeAheadParams {
                text: text.as_str(),
                command,
                search: &search,
                window: &win,
                library: &lib,
                thumbnails: &thumb,
                settings: &set,
                state: &st,
            })
        });
    }

    pub(super) fn register_rating_hotkey_callbacks(&self) {
        let win = self.window.clone_strong();
        let lib = self.library.clone();
        let thumb = self.thumbnails.clone();
        let tm = self.tab_manager.clone();
        let set = self.settings.clone();
        let st = self.state.clone();
        let fullscreen = self.fullscreen.clone();

        self.window.on_rate_key(move |rating| {
            rate_by_key(
                &RatingHotkeyParams {
                    window: &win,
                    library: &lib,
                    thumbnails: &thumb,
                    tab_manager: &tm,
                    settings: &set,
                    state: &st,
                    fullscreen: &fullscreen,
                },
                rating.clamp(0, 5).saturating_as::<u8>(),
            );
        });
    }

    pub(super) fn register_sidebar_resize_callbacks(&self) {
        let last_left_resize = Arc::new(Mutex::new(Instant::now()));
        let left_events_count = Arc::new(AtomicUsize::new(0));

        let last_left_clone = last_left_resize;
        let left_count_clone = left_events_count;

        self.window.on_sidebar_left_resized(move |new_width| {
            let t0 = Instant::now();
            let count = left_count_clone.fetch_add(1, Ordering::Relaxed) + 1;

            let mut last_log = last_left_clone.lock();
            let elapsed = last_log.elapsed();
            if elapsed.as_millis() >= 100 {
                let hz = if elapsed.as_secs_f64() > 0.0 {
                    count.az::<f64>() / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                let elapsed_us = t0.elapsed().as_micros();
                tracing::debug!(
                    sidebar = "left",
                    width = new_width,
                    event_rate_hz = format!("{:.1}", hz),
                    total_events = count,
                    processing_us = elapsed_us,
                    "[Benchmark: Sidebar Drag] Smooth high-FPS left sidebar resize active"
                );
                *last_log = Instant::now();
                left_count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_left = self.settings.clone();
        self.window.on_sidebar_left_resize_finished(move |final_width| {
            let width_f32 = final_width;
            let mut app_settings = set_left.get();
            app_settings.view.sidebar_left_width = width_f32;
            set_left.save_quiet(&app_settings);
        });

        let last_right_resize = Arc::new(Mutex::new(Instant::now()));
        let right_events_count = Arc::new(AtomicUsize::new(0));

        let last_right_clone = last_right_resize;
        let right_count_clone = right_events_count;

        let tm_right = self.tab_manager.clone();
        let fs_right = self.fullscreen.clone();
        let set_right = self.settings.clone();
        let win_right = self.window.clone_strong();
        let st_right = self.state.clone();

        self.window.on_sidebar_right_resized(move |new_width| {
            let t0 = Instant::now();
            let count = right_count_clone.fetch_add(1, Ordering::Relaxed) + 1;

            if win_right.get_active_view_mode() == 1 {
                BridgeViewerTabs::sync_video_embedding(&win_right, &tm_right, &fs_right, &set_right);
            } else if win_right.get_active_view_mode() == 2 {
                BridgeTabGrid::sync_tab_grid_ui(&win_right, &tm_right, &set_right, &st_right);
            }

            let mut last_log = last_right_clone.lock();
            let elapsed = last_log.elapsed();
            if elapsed.as_millis() >= 100 {
                let hz = if elapsed.as_secs_f64() > 0.0 {
                    count.az::<f64>() / elapsed.as_secs_f64()
                } else {
                    0.0
                };
                let elapsed_us = t0.elapsed().as_micros();
                tracing::debug!(
                    sidebar = "right",
                    width = new_width,
                    event_rate_hz = format!("{:.1}", hz),
                    total_events = count,
                    processing_us = elapsed_us,
                    "[Benchmark: Sidebar Drag] Smooth high-FPS right sidebar resize active"
                );
                *last_log = Instant::now();
                right_count_clone.store(0, Ordering::Relaxed);
            }
        });

        let set_right_fin = self.settings.clone();
        let tm_right_fin = self.tab_manager.clone();
        let fs_right_fin = self.fullscreen.clone();
        let win_right_fin = self.window.clone_strong();
        let st_right_fin = self.state.clone();

        self.window.on_sidebar_right_resize_finished(move |final_width| {
            let width_f32 = final_width;
            let mut app_settings = set_right_fin.get();
            app_settings.view.sidebar_right_width = width_f32;
            set_right_fin.save_quiet(&app_settings);

            if win_right_fin.get_active_view_mode() == 1 {
                BridgeViewerTabs::sync_video_embedding(&win_right_fin, &tm_right_fin, &fs_right_fin, &set_right_fin);
            } else if win_right_fin.get_active_view_mode() == 2 {
                BridgeTabGrid::sync_tab_grid_ui(&win_right_fin, &tm_right_fin, &set_right_fin, &st_right_fin);
            }
        });
    }
}
