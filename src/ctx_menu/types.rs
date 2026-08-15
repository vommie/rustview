use serde::{Deserialize, Serialize};

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

/// Generic, decoupled context menu item data structure supporting arbitrary nesting depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CtxMenuItem {
    pub id: CtxMenuActionId,
    pub text: String,
    pub shortcut: Option<String>,
    pub icon_name: Option<String>,
    pub kind: CtxMenuItemKind,
    pub is_checked: bool,
    pub is_enabled: bool,
    pub children: Vec<CtxMenuItem>,
    pub menu_index: usize,
}

impl CtxMenuItem {
    pub fn new_action(id: CtxMenuActionId, text: impl Into<String>) -> Self {
        Self {
            id,
            text: text.into(),
            shortcut: None,
            icon_name: None,
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled: true,
            children: Vec::new(),
            menu_index: 0,
        }
    }

    pub fn new_checkable(id: CtxMenuActionId, text: impl Into<String>, is_checked: bool) -> Self {
        Self {
            id,
            text: text.into(),
            shortcut: None,
            icon_name: None,
            kind: CtxMenuItemKind::Checkable,
            is_checked,
            is_enabled: true,
            children: Vec::new(),
            menu_index: 0,
        }
    }

    pub fn new_separator() -> Self {
        Self {
            id: 0,
            text: String::new(),
            shortcut: None,
            icon_name: None,
            kind: CtxMenuItemKind::Separator,
            is_checked: false,
            is_enabled: false,
            children: Vec::new(),
            menu_index: 0,
        }
    }

    pub fn new_header(text: impl Into<String>) -> Self {
        Self {
            id: 0,
            text: text.into(),
            shortcut: None,
            icon_name: None,
            kind: CtxMenuItemKind::Header,
            is_checked: false,
            is_enabled: false,
            children: Vec::new(),
            menu_index: 0,
        }
    }

    pub fn new_submenu(
        text: impl Into<String>,
        icon_name: Option<&str>,
        is_enabled: bool,
        children: Vec<CtxMenuItem>,
    ) -> Self {
        Self {
            id: 0,
            text: text.into(),
            shortcut: None,
            icon_name: icon_name.map(|s| s.to_string()),
            kind: CtxMenuItemKind::Submenu,
            is_checked: false,
            is_enabled,
            children,
            menu_index: 0,
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
