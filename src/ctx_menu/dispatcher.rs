use super::types::{CtxMenu, CtxMenuActionId, CtxMenuContext, CtxMenuItem};
use crate::ui::ContextMenuItemModel;
use parking_lot::RwLock;
use slint::{Image, ModelRc, VecModel};
use std::collections::HashMap;
use std::sync::Arc;

pub type CtxMenuActionCallback = Arc<dyn Fn(CtxMenuActionId, &CtxMenuContext) + Send + Sync>;

/// Thread-safe registry and dispatcher executing registered context menu actions.
#[derive(Default)]
pub struct CtxMenuDispatcher {
    handlers: RwLock<HashMap<CtxMenuActionId, CtxMenuActionCallback>>,
}

impl CtxMenuDispatcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<F>(&self, id: CtxMenuActionId, handler: F)
    where
        F: Fn(CtxMenuActionId, &CtxMenuContext) + Send + Sync + 'static,
    {
        let mut map = self.handlers.write();
        map.insert(id, Arc::new(handler));
        tracing::debug!(ctx_action_id = id, "Registered context menu action handler");
    }

    pub fn dispatch(&self, id: CtxMenuActionId, context: &CtxMenuContext) -> bool {
        let handler = {
            let map = self.handlers.read();
            map.get(&id).cloned()
        };

        if let Some(h) = handler {
            tracing::info!(ctx_action_id = id, context = ?context, "Dispatching context menu action");
            h(id, context);
            true
        } else {
            tracing::warn!(
                ctx_action_id = id,
                "No handler registered for context menu action"
            );
            false
        }
    }

    /// Converts one menu item into its Slint row model. External application icons (`icon_path`) are loaded
    /// as images; library icons are passed as the typed `IconId` and drawn in the leading icon slot.
    fn to_slint_item(item: &CtxMenuItem, depth: i32) -> ContextMenuItemModel {
        let app_icon = item
            .icon_path
            .as_ref()
            .and_then(|path| match Image::load_from_path(path) {
                Ok(image) => Some(image),
                Err(e) => {
                    tracing::debug!(path = %path.display(), error = ?e, "[CtxMenu] Cannot load application icon");
                    None
                }
            })
            .unwrap_or_default();

        ContextMenuItemModel {
            id: item.id as i32,
            text: item.text.clone().into(),
            shortcut: item.shortcut.clone().unwrap_or_default().into(),
            icon: crate::icons::to_slint(item.icon),
            app_icon,
            kind: item.kind as i32,
            is_checked: item.is_checked,
            is_enabled: item.is_enabled,
            has_submenu: !item.children.is_empty(),
            submenu_depth: depth,
            menu_index: item.menu_index as i32,
            tooltip: item.tooltip.clone().unwrap_or_default().into(),
        }
    }

    /// Converts a slice of CtxMenuItems into a Slint Model for any depth level with native icon and tooltip support.
    pub fn items_to_slint_model(
        items: &[CtxMenuItem],
        depth: i32,
    ) -> ModelRc<ContextMenuItemModel> {
        let models: Vec<ContextMenuItemModel> =
            items.iter().map(|item| Self::to_slint_item(item, depth)).collect();
        ModelRc::new(VecModel::from(models))
    }

    /// Converts a CtxMenu into top-level items and first-level submenu items for backward compatibility.
    pub fn to_slint_models(
        menu: &CtxMenu,
    ) -> (ModelRc<ContextMenuItemModel>, ModelRc<ContextMenuItemModel>) {
        let top_model = Self::items_to_slint_model(&menu.items, 0);

        let sub_items: Vec<ContextMenuItemModel> = menu
            .items
            .iter()
            .flat_map(|item| item.children.iter())
            .map(|child| Self::to_slint_item(child, 1))
            .collect();

        (top_model, ModelRc::new(VecModel::from(sub_items)))
    }
}
