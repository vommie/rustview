use crate::icons::Icon;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Explicit unique identifier for a context menu action.
pub type CtxMenuActionId = u64;

/// Semantic representation of item types supported in the context menu model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum CtxMenuItemKind {
    Action = 0,
    Checkable = 1,
    Radio = 2,
    Separator = 3,
    Header = 4,
    Submenu = 5,
}

/// Generic, decoupled context menu item data structure supporting arbitrary nesting depth and tooltips.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CtxMenuItem {
    pub id: CtxMenuActionId,
    pub text: String,
    pub shortcut: Option<String>,
    /// Library icon shown in the leading icon slot of the row.
    pub icon: Option<Icon>,
    /// Image file of an external application (system icon theme); takes precedence over `icon`.
    pub icon_path: Option<PathBuf>,
    pub kind: CtxMenuItemKind,
    pub is_checked: bool,
    pub is_enabled: bool,
    pub children: Vec<CtxMenuItem>,
    pub menu_index: usize,
    #[serde(default)]
    pub tooltip: Option<String>,
}

impl CtxMenuItem {
    pub fn new_action(id: CtxMenuActionId, text: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into(),
            shortcut: None,
            icon: None,
            icon_path: None,
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled: true,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
        }
    }

    /// Sets the leading library icon of the item.
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn new_checkable(id: CtxMenuActionId, text: impl Into<String>, is_checked: bool) -> Self {
        Self {
            id,
            text: text.into(),
            shortcut: None,
            icon: None,
            icon_path: None,
            kind: CtxMenuItemKind::Checkable,
            is_checked,
            is_enabled: true,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
        }
    }

    pub fn new_separator() -> Self {
        Self {
            id: 0,
            text: String::new(),
            shortcut: None,
            icon: None,
            icon_path: None,
            kind: CtxMenuItemKind::Separator,
            is_checked: false,
            is_enabled: false,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
        }
    }

    pub fn new_header(text: impl Into<String>) -> Self {
        Self {
            id: 0,
            text: text.into(),
            shortcut: None,
            icon: None,
            icon_path: None,
            kind: CtxMenuItemKind::Header,
            is_checked: false,
            is_enabled: false,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
        }
    }

    pub fn new_header_with_tooltip(text: impl Into<String>, tooltip: impl Into<String>) -> Self {
        Self {
            id: 0,
            text: text.into(),
            shortcut: None,
            icon: None,
            icon_path: None,
            kind: CtxMenuItemKind::Header,
            is_checked: false,
            is_enabled: false,
            children: Vec::new(),
            menu_index: 0,
            tooltip: Some(tooltip.into()),
        }
    }

    pub fn new_submenu(
        text: impl Into<String>,
        icon: Option<Icon>,
        is_enabled: bool,
        children: Vec<CtxMenuItem>,
    ) -> Self {
        Self {
            id: 0,
            text: text.into(),
            shortcut: None,
            icon,
            icon_path: None,
            kind: CtxMenuItemKind::Submenu,
            is_checked: false,
            is_enabled,
            children,
            menu_index: 0,
            tooltip: None,
        }
    }
}

/// Root context menu model holding an array of items and providing hierarchical traversal utilities.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CtxMenu {
    pub title: Option<String>,
    pub items: Vec<CtxMenuItem>,
}

impl CtxMenu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Indexes all nodes in the hierarchy with unique sequential indices for fast lookups.
    pub fn index_items(&mut self) {
        let mut counter = 1;
        Self::index_slice(&mut self.items, &mut counter);
    }

    fn index_slice(items: &mut [CtxMenuItem], counter: &mut usize) {
        for item in items.iter_mut() {
            item.menu_index = *counter;
            *counter += 1;
            if !item.children.is_empty() {
                Self::index_slice(&mut item.children, counter);
            }
        }
    }

    /// Texts of all actionable entries (actions, checkables, radios, submenus) without any icon. Headers and
    /// separators never have icons. Every actionable entry must show an icon in the reserved leading slot.
    pub fn entries_without_icon(&self) -> Vec<String> {
        let mut missing = Vec::new();
        Self::collect_without_icon(&self.items, &mut missing);
        missing
    }

    fn collect_without_icon(items: &[CtxMenuItem], missing: &mut Vec<String>) {
        for item in items {
            let actionable = !matches!(item.kind, CtxMenuItemKind::Separator | CtxMenuItemKind::Header);
            if actionable && item.icon.is_none() && item.icon_path.is_none() {
                missing.push(item.text.clone());
            }
            Self::collect_without_icon(&item.children, missing);
        }
    }

    /// Retrieves the children slice of the item matching `menu_idx`.
    pub fn find_children_by_menu_index(&self, menu_idx: usize) -> Option<&[CtxMenuItem]> {
        Self::search_children(&self.items, menu_idx)
    }

    fn search_children(items: &[CtxMenuItem], menu_idx: usize) -> Option<&[CtxMenuItem]> {
        for item in items {
            if item.menu_index == menu_idx {
                return Some(&item.children);
            }
            if let Some(res) = Self::search_children(&item.children, menu_idx) {
                return Some(res);
            }
        }
        None
    }
}

/// Dynamic context menu invocation payload passing caller context.
#[derive(Debug, Clone, Default)]
pub struct CtxMenuContext {
    pub position_x: f32,
    pub position_y: f32,
    pub selected_item_count: usize,
    pub target_ids: Vec<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx_menu::CtxMenuBuilder;

    #[test]
    fn reports_only_actionable_entries_without_icon() {
        let menu = CtxMenuBuilder::new()
            .header("Header")
            .action(1, "With icon", None, Some(Icon::Copy), true)
            .action(2, "Without icon", None, None, true)
            .separator()
            .submenu("Group", Some(Icon::Folder), true, |sub| sub.action(3, "Nested without icon", None, None, true))
            .build();
        assert_eq!(menu.entries_without_icon(), vec!["Without icon".to_string(), "Nested without icon".to_string()]);
    }
}
