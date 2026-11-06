//! The dock layout in `settings.json` (`view.dock`, a JSON value the core stores without knowing it), adapted from
//! `RustCut`'s `ui/dock/persist.rs`.
//!
//! The stored form is tolerant: panels are names (unknown ones, e.g. from a newer version, are dropped), every field has a
//! default, and the result is sanitized. The value carries its own `version` for future reshaping (none so far). A file
//! of 0.6.13 or older has no dock yet: [`legacy_layout`] builds the layout those versions showed from their fields.

use serde::{Deserialize, Serialize};

use super::model::{DockLayout, Group, PanelId, Side, Sidebar};

/// Version of the stored dock value.
const DOCK_VERSION: u32 = 1;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredDock {
    version: u32,
    left: Option<StoredSidebar>,
    right: Option<StoredSidebar>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredSidebar {
    width: Option<f32>,
    collapsed: bool,
    maximized: Option<usize>,
    /// The panel whose group is held over the center while collapsed.
    pinned: Option<String>,
    groups: Vec<StoredGroup>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredGroup {
    tabs: Vec<String>,
    active: Option<String>,
    weight: Option<f32>,
    collapsed: bool,
}

/// What the sidebars of 0.6.13 and older kept in their own settings fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LegacySidebars<'a> {
    pub left_width: f32,
    pub right_width: f32,
    /// `view.left_sidebar_panel_tabs` (the order of Folders and Favorites) and the active one.
    pub left_tabs: &'a [String],
    pub left_active: &'a str,
    pub info_collapsed: bool,
    pub history_collapsed: bool,
}

/// The layout of the sidebars before the dock, from their old settings fields.
pub fn legacy_layout(old: &LegacySidebars<'_>) -> DockLayout {
    let mut layout = DockLayout::default();
    layout.left.width = old.left_width;
    layout.right.width = old.right_width;
    let mut order: Vec<PanelId> = old.left_tabs.iter().filter_map(|key| PanelId::from_key(key)).collect();
    order.retain(|p| matches!(p, PanelId::Folders | PanelId::Favorites));
    order.dedup();
    if let Some(top) = layout.left.groups.get_mut(0) {
        if order.len() == 2 {
            top.tabs = order;
        }
        if let Some(active) = PanelId::from_key(old.left_active).filter(|p| top.tabs.contains(p)) {
            top.active = active;
        }
    }
    if let Some(info) = layout.left.groups.get_mut(1) {
        info.collapsed = old.info_collapsed;
    }
    if let Some(history) = layout.right.groups.get_mut(1) {
        history.collapsed = old.history_collapsed;
    }
    layout.sanitize();
    layout
}

/// The layout as a JSON value for `view.dock`.
pub fn to_json(layout: &DockLayout) -> Option<serde_json::Value> {
    let stored = StoredDock {
        version: DOCK_VERSION,
        left: Some(store_sidebar(&layout.left)),
        right: Some(store_sidebar(&layout.right)),
    };
    match serde_json::to_value(stored) {
        Ok(value) => Some(value),
        Err(e) => {
            tracing::warn!(error = %e, "[Dock] The layout could not be stored");
            None
        }
    }
}

/// The layout from `view.dock`; `fallback` (the layout of the old fields) without a value, the default layout when the
/// value cannot be read.
pub fn from_json(value: Option<&serde_json::Value>, fallback: DockLayout) -> DockLayout {
    let Some(value) = value else {
        return fallback;
    };
    let stored: StoredDock = match serde_json::from_value(value.clone()) {
        Ok(stored) => stored,
        Err(e) => {
            tracing::warn!(error = %e, "[Dock] The stored layout is unreadable; the default layout is used");
            return DockLayout::default();
        }
    };
    if stored.version > DOCK_VERSION {
        tracing::warn!(
            version = stored.version,
            "[Dock] The layout was stored by a newer version; reading what is known"
        );
    }
    let defaults = DockLayout::default();
    let mut layout = DockLayout {
        left: stored
            .left
            .map_or_else(|| defaults.left.clone(), |s| load_sidebar(s, Side::Left)),
        right: stored
            .right
            .map_or_else(|| defaults.right.clone(), |s| load_sidebar(s, Side::Right)),
    };
    layout.sanitize();
    layout
}

fn store_sidebar(sidebar: &Sidebar) -> StoredSidebar {
    StoredSidebar {
        width: Some(sidebar.width),
        collapsed: sidebar.collapsed,
        maximized: sidebar.maximized,
        pinned: sidebar.pinned.map(|p| p.key().to_owned()),
        groups: sidebar
            .groups
            .iter()
            .map(|g| StoredGroup {
                tabs: g.tabs.iter().map(|p| p.key().to_owned()).collect(),
                active: Some(g.active.key().to_owned()),
                weight: Some(g.weight),
                collapsed: g.collapsed,
            })
            .collect(),
    }
}

fn load_sidebar(stored: StoredSidebar, side: Side) -> Sidebar {
    let default_width = DockLayout::default().sidebar(side).width;
    let mut groups = Vec::new();
    let mut maximized = None;
    for (index, g) in stored.groups.into_iter().enumerate() {
        let tabs: Vec<PanelId> = g.tabs.iter().filter_map(|key| PanelId::from_key(key)).collect();
        let Some(mut group) = Group::new(tabs, g.weight.unwrap_or(1.0)) else {
            continue;
        };
        if stored.maximized == Some(index) {
            maximized = Some(groups.len());
        }
        if let Some(active) = g.active.as_deref().and_then(PanelId::from_key)
            && group.tabs.contains(&active)
        {
            group.active = active;
        }
        group.collapsed = g.collapsed;
        groups.push(group);
    }
    Sidebar {
        groups,
        width: stored.width.unwrap_or(default_width),
        collapsed: stored.collapsed,
        // Follows its group when groups before it were dropped (no known panel left).
        maximized,
        pinned: stored.pinned.as_deref().and_then(PanelId::from_key),
    }
}

#[cfg(test)]
#[path = "persist_tests.rs"]
mod tests;
