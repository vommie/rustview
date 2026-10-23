use rustview_core::library::models::TagId;

/// Fully resolved, ready-to-render tag item belonging to the Assigned Tags History.
#[derive(Debug, Clone, PartialEq)]
pub struct TagHistoryItem {
    pub id: TagId,
    pub name: String,
    pub display_text: String,
    pub is_assigned: bool,
    pub is_partial: bool,
    pub is_bold: bool,
    pub is_italic: bool,
    pub width: f32,
    pub tooltip: String,
}

/// A horizontally packed row containing one or more flowing TagHistoryItems.
#[derive(Debug, Clone, PartialEq)]
pub struct TagHistoryRow {
    pub items: Vec<TagHistoryItem>,
}
