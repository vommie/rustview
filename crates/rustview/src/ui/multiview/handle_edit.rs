//! The image editor's side of the handle: finding the Multiview a tile belongs to, drawing it again in whatever host shows it,
//! the single-shot timer of the Save button (its fade after a save) and the edit actions that need the application (saving,
//! the drop-down of the Save button).

use super::bar_overlay::{self, BarKind};
use super::handle::MultiviewHandle;
use super::{router, session, tab_host};
use crate::ctx_menu::CtxMenuItem;
use crate::icons::Icon;
use crate::ui::image_edit::{self, SaveIntent};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::media_actions::{MediaActionId, SaveMenuItem};
use rustview_core::util::cast::SaturatingAs;
use std::sync::Arc;
use std::time::{Duration, Instant};

impl MultiviewHandle {
    /// The Multiview that shows tile `key`: the fullscreen, the shown tab, or any other tab.
    pub fn instance_with_tile(&self, key: TileKey) -> Option<SharedInstance> {
        let has = |instance: &SharedInstance| instance.lock().has_tile(key);
        session::instance()
            .filter(has)
            .or_else(|| tab_host::current_instance().filter(has))
            .or_else(|| {
                self.app()
                    .and_then(|app| app.tab_manager.all_instances().into_iter().find(has))
            })
    }

    /// Draws `instance` again in the host that shows it, and arms the timer of its Save buttons.
    pub fn refresh_instance(&self, instance: &SharedInstance) {
        let mode = instance.lock().host_mode();
        router::refresh_host(self, mode);
        self.arm_edit_timer(instance);
    }

    /// One single-shot timer for the next change of the Save buttons of `instance` (the fade after a save); nothing runs while
    /// nothing changes.
    pub fn arm_edit_timer(&self, instance: &SharedInstance) {
        let now = Instant::now();
        let Some(when) = instance.lock().edit_wakeup(now) else {
            return;
        };
        let delay = when.saturating_duration_since(now) + Duration::from_millis(5);
        let (handle, weak) = (self.clone(), Arc::downgrade(instance));
        slint::Timer::single_shot(delay, move || {
            if let Some(instance) = weak.upgrade() {
                instance.lock().edit_tick(Instant::now());
                handle.refresh_instance(&instance);
            }
        });
    }

    /// The Save button, its drop-down or Ctrl+S / Ctrl+Shift+S.
    pub(super) fn save_tile(&self, key: TileKey, intent: SaveIntent) {
        let Some((app, window)) = self.app_and_window() else {
            return;
        };
        let Some(instance) = self.instance_with_tile(key) else {
            tracing::warn!(tile = key, "[ImageEdit: Save] The tile to save is gone");
            return;
        };
        image_edit::save_tile(&self.edit_context(app, &window), &instance, key, intent, None);
    }

    /// The application and the main window, for the editor's questions and saves.
    pub fn app_and_window_for_edit(&self) -> Option<(super::handle::AppContext, crate::ui::MainWindow)> {
        self.app_and_window()
    }

    pub fn edit_context(
        &self,
        app: super::handle::AppContext,
        window: &crate::ui::MainWindow,
    ) -> image_edit::EditContext {
        image_edit::EditContext {
            handle: self.clone(),
            app,
            window: slint::ComponentHandle::as_weak(window),
        }
    }

    /// The remove button of a tile with unsaved edits: asks, then removes the tile.
    pub(super) fn confirm_remove_tile(&self, key: TileKey) {
        let Some((app, window)) = self.app_and_window() else {
            return;
        };
        let Some(instance) = self.instance_with_tile(key) else {
            return;
        };
        let context = self.edit_context(app, &window);
        let (handle, weak) = (self.clone(), Arc::downgrade(&instance));
        let instance_id = instance.lock().id();
        image_edit::close_guard::confirm(
            &context,
            vec![(instance, Some(vec![key]))],
            crate::viewer::edit::unsaved::CloseReason::Tile,
            move || {
                if let Some(instance) = weak.upgrade() {
                    let action = instance.lock().remove_tile_by_key(key);
                    handle.refresh_instance(&instance);
                    router::execute(action, &handle, Some(instance_id));
                }
            },
        );
    }

    /// The arrow of the Save button: its drop-down opens (Save, Save As..., Discard Changes), or closes when it is open.
    pub(super) fn toggle_save_menu() {
        if bar_overlay::close_clicked_menu(BarKind::Media) {
            return;
        }
        let item = |entry: SaveMenuItem, text: &str, icon: Icon| {
            CtxMenuItem::new_action(entry.code().saturating_as::<u64>(), text).with_icon(icon)
        };
        let items = [
            item(SaveMenuItem::Save, "Save", Icon::Save),
            item(SaveMenuItem::SaveAs, "Save As...", Icon::SaveAs),
            item(SaveMenuItem::Discard, "Discard Changes", Icon::DiscardChanges),
        ];
        bar_overlay::open_menu(BarKind::Media, MediaActionId::Save.code(), &items);
    }
}
