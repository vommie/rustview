use super::types::{CtxMenu, CtxMenuActionId, CtxMenuContext, CtxMenuItem};
use crate::ui::ContextMenuItemModel;
use parking_lot::RwLock;
use slint::{ModelRc, VecModel};
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

    /// Converts a CtxMenu into top-level items and separate flyout submenu items for Slint.
    pub fn to_slint_models(
        menu: &CtxMenu,
    ) -> (ModelRc<ContextMenuItemModel>, ModelRc<ContextMenuItemModel>) {
        let mut top_level = Vec::new();
        let mut sub_items = Vec::new();

        for item in &menu.items {
            let has_sub = !item.children.is_empty();
            top_level.push(ContextMenuItemModel {
                id: item.id as i32,
                text: item.text.clone().into(),
                shortcut: item.shortcut.clone().unwrap_or_default().into(),
                icon_name: item.icon_name.clone().unwrap_or_default().into(),
                kind: item.kind as i32,
                is_checked: item.is_checked,
                is_enabled: item.is_enabled,
                has_submenu: has_sub,
                submenu_depth: 0,
            });

            if has_sub {
                for child in &item.children {
                    sub_items.push(ContextMenuItemModel {
                        id: child.id as i32,
                        text: child.text.clone().into(),
                        shortcut: child.shortcut.clone().unwrap_or_default().into(),
                        icon_name: child.icon_name.clone().unwrap_or_default().into(),
                        kind: child.kind as i32,
                        is_checked: child.is_checked,
                        is_enabled: child.is_enabled,
                        has_submenu: false,
                        submenu_depth: 1,
                    });
                }
            }
        }

        (
            ModelRc::new(VecModel::from(top_level)),
            ModelRc::new(VecModel::from(sub_items)),
        )
    }

    /// Flattens hierarchical CtxMenuItems into a single Slint UI model (e.g. for simple menus or tests).
    pub fn to_slint_model(menu: &CtxMenu) -> ModelRc<ContextMenuItemModel> {
        let mut flat_items = Vec::new();
        Self::flatten_items(&menu.items, 0, &mut flat_items);
        ModelRc::new(VecModel::from(flat_items))
    }

    fn flatten_items(items: &[CtxMenuItem], depth: i32, output: &mut Vec<ContextMenuItemModel>) {
        for item in items {
            let has_sub = !item.children.is_empty();
            output.push(ContextMenuItemModel {
                id: item.id as i32,
                text: item.text.clone().into(),
                shortcut: item.shortcut.clone().unwrap_or_default().into(),
                icon_name: item.icon_name.clone().unwrap_or_default().into(),
                kind: item.kind as i32,
                is_checked: item.is_checked,
                is_enabled: item.is_enabled,
                has_submenu: has_sub,
                submenu_depth: depth,
            });

            if has_sub {
                Self::flatten_items(&item.children, depth + 1, output);
            }
        }
    }
}
