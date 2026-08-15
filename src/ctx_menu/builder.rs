use super::types::{CtxMenu, CtxMenuActionId, CtxMenuItem, CtxMenuItemKind};

/// Fluent, declarative builder for constructing hierarchical context menus.
#[derive(Debug, Default)]
pub struct CtxMenuBuilder {
    title: Option<String>,
    items: Vec<CtxMenuItem>,
}

impl CtxMenuBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn header(mut self, text: impl Into<String>) -> Self {
        self.items.push(CtxMenuItem::new_header(text));
        self
    }

    pub fn separator(mut self) -> Self {
        self.items.push(CtxMenuItem::new_separator());
        self
    }

    pub fn action(
        mut self,
        id: CtxMenuActionId,
        text: impl Into<String>,
        shortcut: Option<&str>,
        icon_name: Option<&str>,
        is_enabled: bool,
    ) -> Self {
        self.items.push(CtxMenuItem {
            id,
            text: text.into(),
            shortcut: shortcut.map(|s| s.to_string()),
            icon_name: icon_name.map(|s| s.to_string()),
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled,
            children: Vec::new(),
        });
        self
    }

    pub fn checkable(
        mut self,
        id: CtxMenuActionId,
        text: impl Into<String>,
        is_checked: bool,
        shortcut: Option<&str>,
        icon_name: Option<&str>,
        is_enabled: bool,
    ) -> Self {
        self.items.push(CtxMenuItem {
            id,
            text: text.into(),
            shortcut: shortcut.map(|s| s.to_string()),
            icon_name: icon_name.map(|s| s.to_string()),
            kind: CtxMenuItemKind::Checkable,
            is_checked,
            is_enabled,
            children: Vec::new(),
        });
        self
    }

    pub fn radio(
        mut self,
        id: CtxMenuActionId,
        text: impl Into<String>,
        is_checked: bool,
        shortcut: Option<&str>,
        icon_name: Option<&str>,
        is_enabled: bool,
    ) -> Self {
        self.items.push(CtxMenuItem {
            id,
            text: text.into(),
            shortcut: shortcut.map(|s| s.to_string()),
            icon_name: icon_name.map(|s| s.to_string()),
            kind: CtxMenuItemKind::Radio,
            is_checked,
            is_enabled,
            children: Vec::new(),
        });
        self
    }

    pub fn submenu<F>(
        mut self,
        text: impl Into<String>,
        icon_name: Option<&str>,
        is_enabled: bool,
        builder_fn: F,
    ) -> Self
    where
        F: FnOnce(CtxMenuBuilder) -> CtxMenuBuilder,
    {
        let child_builder = builder_fn(CtxMenuBuilder::new());
        let child_menu = child_builder.build();

        self.items.push(CtxMenuItem {
            id: 0,
            text: text.into(),
            shortcut: None,
            icon_name: icon_name.map(|s| s.to_string()),
            kind: CtxMenuItemKind::Submenu,
            is_checked: false,
            is_enabled,
            children: child_menu.items,
        });
        self
    }

    pub fn build(self) -> CtxMenu {
        CtxMenu {
            title: self.title,
            items: self.items,
        }
    }
}
