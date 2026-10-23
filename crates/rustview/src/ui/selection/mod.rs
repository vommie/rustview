pub mod delete;
pub mod keyboard;
pub mod model;
pub mod rubberband;
pub mod rubberband_bridge;

use crate::ui::MainWindow;
use crate::ui::multiview::MultiviewHandle;
use crate::ui::state::{LastActivePanel, UiState};
use crate::ui::browse_folders::BrowseFolders;
use rustview_core::library::Library;
use rustview_core::library::models::MediaId;
use rustview_core::settings::SettingsManager;
use crate::viewer::ViewerTabManager;
use parking_lot::{Mutex, RwLock};
use slint::{ComponentHandle};
use std::sync::Arc;
use std::time::Instant;

mod clicks;
mod operations;

/// Parameters representing a media click event with modifier keys.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileClickEvent {
    pub media_id: MediaId,
    pub is_ctrl: bool,
    pub is_shift: bool,
}

/// Context parameter bundle for handling media file click events.
pub struct FileClickContext<'a> {
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub settings: &'a Arc<SettingsManager>,
    pub tab_manager: &'a Arc<ViewerTabManager>,
    pub state: &'a Arc<RwLock<UiState>>,
    pub last_click: &'a Arc<Mutex<Option<LastClickRecord>>>,
    pub fullscreen: &'a MultiviewHandle,
}

/// Discriminant representing the detected double click type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoubleClickKind {
    None,
    Normal,
    Ctrl,
}

/// Record capturing timestamp and modifier state of the previous click for double-click evaluation.
#[derive(Debug, Clone, Copy)]
pub struct LastClickRecord {
    pub media_id: MediaId,
    pub instant: Instant,
    pub is_ctrl: bool,
}

/// Subsystem managing media file selection calculations, fast in-place model mutations, clicks, and the triggers that drive the fullscreen Multiview.
pub struct BridgeFileSelection;

impl BridgeFileSelection {
    /// Registers media click, middle click, background click, and global selection hotkey callbacks onto MainWindow.
    pub fn register_callbacks(
        window: &MainWindow,
        library: Arc<Library>,
        settings: Arc<SettingsManager>,
        tab_manager: Arc<ViewerTabManager>,
        state: Arc<RwLock<UiState>>,
        last_click: Arc<Mutex<Option<LastClickRecord>>>,
        fullscreen: MultiviewHandle,
    ) {
        let win = window.clone_strong();
        let lib = library.clone();
        let set = settings.clone();
        let tm = tab_manager.clone();
        let st = state.clone();
        let lc = last_click;
        let fs = fullscreen.clone();
        window.on_media_clicked(move |id, is_ctrl, is_shift| {
            Self::handle_click(
                FileClickEvent {
                    media_id: id as u64,
                    is_ctrl,
                    is_shift,
                },
                FileClickContext {
                    window: &win,
                    library: &lib,
                    settings: &set,
                    tab_manager: &tm,
                    state: &st,
                    last_click: &lc,
                    fullscreen: &fs,
                },
            );
        });

        let (win_enter, lib_enter, set_enter, st_enter, tm_enter, fs_enter) =
            (window.clone_strong(), library.clone(), settings.clone(), state.clone(), tab_manager.clone(), fullscreen.clone());
        window.on_browse_enter_pressed(move || {
            if BrowseFolders::open_selected_row(&win_enter, &lib_enter, &set_enter, &st_enter) {
                return;
            }
            let single = {
                let st = st_enter.read();
                (st.file_selection.count() == 1 && !st.folder_rows.has_selection())
                    .then(|| st.file_selection.selected_ids().iter().next().copied())
                    .flatten()
            };
            if let Some(media_id) = single {
                Self::handle_double_click(media_id, &st_enter, &lib_enter, &set_enter, &tm_enter, &win_enter, &fs_enter);
            }
        });

        let win_mid = window.clone_strong();
        let lib_mid = library.clone();
        let set_mid = settings;
        let st_mid = state.clone();
        let fs_mid = fullscreen;
        window.on_media_middle_clicked(move |id| {
            Self::handle_middle_click(id as u64, &win_mid, &lib_mid, &set_mid, &st_mid, &fs_mid);
        });

        let win_bg = window.clone_strong();
        let lib_bg = library.clone();
        let st_bg = state.clone();
        window.on_background_clicked(move || {
            tracing::info!(
                "[FileSelection: BackgroundClick] Background free space clicked, clearing active file selection"
            );
            st_bg.write().last_active_panel = LastActivePanel::Media;
            Self::clear_selection(&win_bg, &lib_bg, &st_bg);
        });

        // Global Select All (Ctrl+A) Callback in Browse View
        let win_sel_all = window.clone_strong();
        let lib_sel_all = library.clone();
        let st_sel_all = state.clone();
        window.on_select_all_media(move || {
            tracing::info!("[FileSelection: Shortcut] Ctrl+A shortcut received in Browse View: selecting all media");
            Self::select_all(&win_sel_all, &lib_sel_all, &st_sel_all);
        });

        // Global Clear Selection (Ctrl+Shift+A / Esc) Callback in Browse View
        let win_clr_sel = window.clone_strong();
        let lib_clr_sel = library;
        let st_clr_sel = state;
        window.on_clear_media_selection(move || {
            tracing::info!("[FileSelection: Shortcut] Ctrl+Shift+A / Esc shortcut received in Browse View: clearing selection");
            Self::clear_selection(&win_clr_sel, &lib_clr_sel, &st_clr_sel);
        });
    }

}
