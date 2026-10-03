use super::types::{CtxMenu, CtxMenuActionId, CtxMenuItem, CtxMenuItemKind};
use std::path::PathBuf;

/// Fluent, declarative builder for constructing multi-level context menus.
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

    pub fn header_with_tooltip(
        mut self,
        text: impl Into<String>,
        tooltip: impl Into<String>,
    ) -> Self {
        self.items
            .push(CtxMenuItem::new_header_with_tooltip(text, tooltip));
        self
    }

    /// Single Point of Truth (SPoT): Formats a dynamic header with a sticky prefix and left-side ellipsis (…filename.ext)
    /// to prevent visual overflow in context menus while preserving the category prefix and the file extension.
    /// Returns `(display_text, full_tooltip_text)`.
    pub fn format_header_title(
        prefix: &str,
        name: &str,
        max_name_chars: usize,
    ) -> (String, String) {
        let trimmed_name = name.trim();
        let full_text = format!("{}{}", prefix, trimmed_name);

        let char_count = trimmed_name.chars().count();
        if char_count <= max_name_chars {
            (full_text.clone(), full_text)
        } else {
            let keep_count = max_name_chars.saturating_sub(1);
            let chars: Vec<char> = trimmed_name.chars().collect();
            let start_idx = char_count.saturating_sub(keep_count);
            let suffix: String = chars[start_idx..].iter().collect();
            let display_text = format!("{}…{}", prefix, suffix);
            (display_text, full_text)
        }
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
            icon_path: None,
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
        });
        self
    }

    pub fn action_with_icon_path(
        mut self,
        id: CtxMenuActionId,
        text: impl Into<String>,
        shortcut: Option<&str>,
        icon_path: Option<PathBuf>,
        is_enabled: bool,
    ) -> Self {
        self.items.push(CtxMenuItem {
            id,
            text: text.into(),
            shortcut: shortcut.map(|s| s.to_string()),
            icon_name: None,
            icon_path,
            kind: CtxMenuItemKind::Action,
            is_checked: false,
            is_enabled,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
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
            icon_path: None,
            kind: CtxMenuItemKind::Checkable,
            is_checked,
            is_enabled,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
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
            icon_path: None,
            kind: CtxMenuItemKind::Radio,
            is_checked,
            is_enabled,
            children: Vec::new(),
            menu_index: 0,
            tooltip: None,
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
            icon_path: None,
            kind: CtxMenuItemKind::Submenu,
            is_checked: false,
            is_enabled,
            children: child_menu.items,
            menu_index: 0,
            tooltip: None,
        });
        self
    }

    pub fn item(mut self, item: CtxMenuItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn build(self) -> CtxMenu {
        let mut menu = CtxMenu {
            title: self.title,
            items: self.items,
        };
        menu.index_items();
        menu
    }
}
