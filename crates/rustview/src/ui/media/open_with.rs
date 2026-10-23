//! "Open With" for media: the applications that can open a file, launching one of them and the native application chooser.
//! One implementation for every place that offers it (the context menu of a medium, the action bar of the Multiview).

use crate::ctx_menu::{CtxMenuActionId, CtxMenuBuilder, CtxMenuItem};
use crate::icons::Icon;
use crate::system::{DesktopApp, DesktopIntegration};
use crate::ui::MainWindow;
use crate::ui::state::UiState;
use crate::ui::state::{CTX_ACTION_OPEN_WITH_APP_BASE, CTX_ACTION_OPEN_WITH_OTHER};
use crate::ui::status_bar::StatusBarBridge;
use parking_lot::RwLock;
use rustview_core::library::Library;
use rustview_core::library::models::MediaId;
use rustview_core::settings::LogObscurer;
use std::path::{Path, PathBuf};

/// What a chosen item of the "Open With" menu stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenWithChoice {
    /// The application at this index of the list `discover_apps` stored.
    App(usize),
    /// "Other Application...": the native chooser.
    Other,
}

/// The most applications one menu can offer (the item ids of the applications are `CTX_ACTION_OPEN_WITH_APP_BASE` + index).
const MAX_APPS: CtxMenuActionId = 500;

pub struct MediaOpenWith;

impl MediaOpenWith {
    /// The files of `ids`, resolved from the loaded folder first and the library after that; the drive is not touched.
    pub fn paths_for(state: &RwLock<UiState>, library: &Library, ids: &[MediaId]) -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(ids.len());
        let st = state.read();
        for &id in ids {
            if let Some(item) = st.all_folder_media.find(id) {
                paths.push(item.full_path());
            } else if let Ok(p) = library.resolve_media_path(id) {
                paths.push(p);
            }
        }
        paths
    }

    /// Looks up the applications associated with `path` and keeps them in the state: the index of an application in that
    /// list is what `launch` takes afterwards.
    pub fn discover_apps(state: &RwLock<UiState>, path: &Path) -> Vec<DesktopApp> {
        let apps = DesktopIntegration::get_associated_apps(path);
        state.write().active_associated_apps = apps.clone();
        apps
    }

    /// The items of the "Open With" menu: one per application, a separator and "Other Application...". The submenu of the
    /// context menu and the drop-down of the Multiview action bar are both built from this.
    pub fn append_menu_items(mut menu: CtxMenuBuilder, apps: &[DesktopApp]) -> CtxMenuBuilder {
        for (idx, app) in apps.iter().enumerate() {
            let action_id = CTX_ACTION_OPEN_WITH_APP_BASE + (idx as CtxMenuActionId);
            menu = menu.action_with_icon_path(action_id, app.name.clone(), None, app.icon_path.clone(), true);
        }

        if !apps.is_empty() {
            menu = menu.separator();
        }

        menu.action(
            CTX_ACTION_OPEN_WITH_OTHER,
            "Other Application...",
            None,
            Some(Icon::OpenWith),
            true,
        )
    }

    /// The drop-down of the Multiview action bar.
    pub fn menu_items(apps: &[DesktopApp]) -> Vec<CtxMenuItem> {
        Self::append_menu_items(CtxMenuBuilder::new(), apps).build().items
    }

    /// What the menu item `action_id` stands for, or `None` if it is no item of the "Open With" menu.
    pub fn choice_of(action_id: CtxMenuActionId) -> Option<OpenWithChoice> {
        if action_id == CTX_ACTION_OPEN_WITH_OTHER {
            return Some(OpenWithChoice::Other);
        }
        let offset = action_id.checked_sub(CTX_ACTION_OPEN_WITH_APP_BASE)?;
        (0..MAX_APPS)
            .contains(&offset)
            .then_some(OpenWithChoice::App(offset as usize))
    }

    /// Opens `paths` with application `app_idx` of the list `discover_apps` (or the context menu) stored last.
    pub fn launch(window: &MainWindow, state: &RwLock<UiState>, paths: &[PathBuf], app_idx: usize) {
        let app_opt = state.read().active_associated_apps.get(app_idx).cloned();
        let Some(app) = app_opt else {
            tracing::warn!(app_idx, "[OpenWith] Target desktop app index not found");
            return;
        };

        if paths.is_empty() {
            tracing::warn!(app = %app.name, "[OpenWith] No file paths found to open with app");
            return;
        }

        tracing::info!(
            app = %app.name,
            paths_count = paths.len(),
            first_path = %LogObscurer::path(&paths[0]),
            "[OpenWith] Launching selected file(s) with specific desktop app"
        );

        if let Err(e) = DesktopIntegration::launch_app(&app, paths) {
            tracing::error!(error = %e, app = %app.name, "[OpenWith] Failed to launch desktop app");
            StatusBarBridge::error(window, format!("Failed to launch '{}': {}", app.name, e));
        } else {
            StatusBarBridge::success(window, format!("Launched '{}' with {} file(s).", app.name, paths.len()));
        }
    }

    /// "Other Application...": the native application chooser for the first of `paths`.
    pub fn choose_application(window: &MainWindow, paths: &[PathBuf]) {
        let Some(first_path) = paths.first() else {
            tracing::warn!("[OpenWith] No valid file path found for application chooser");
            return;
        };

        tracing::info!(
            target_file = %LogObscurer::path(first_path),
            "[OpenWith] 'Other Application...' triggered"
        );

        if let Err(e) = DesktopIntegration::open_with_dialog(first_path) {
            tracing::error!(error = %e, "[OpenWith] Failed to open native app chooser");
            StatusBarBridge::error(window, format!("Failed to open application chooser: {}", e));
        } else {
            StatusBarBridge::info(window, "Opened native application chooser dialog.");
        }
    }
}
