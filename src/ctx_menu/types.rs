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

/// Generic, decoupled context menu item data structure.
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
        }
    }
}

/// Root context menu model holding an array of items and optional metadata.
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
}

/// Dynamic context menu invocation payload passing caller context without binding to concrete entities.
#[derive(Debug, Clone, Default)]
pub struct CtxMenuContext {
    pub position_x: f32,
    pub position_y: f32,
    pub selected_item_count: usize,
    pub target_ids: Vec<u64>,
}
