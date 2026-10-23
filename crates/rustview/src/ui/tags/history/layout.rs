use super::models::{TagHistoryItem, TagHistoryRow};

/// High-performance flow layout engine packing variable-width tag history items into rows
/// with realistic character metric calculation.
pub struct TagHistoryLayoutEngine;

impl TagHistoryLayoutEngine {
    pub const PILL_HEIGHT: f32 = 22.0;
    pub const PILL_HORIZONTAL_PADDING: f32 = 10.0; // 5px left + 5px right padding
    pub const PILL_HORIZONTAL_SPACING: f32 = 4.0;
    pub const ROW_VERTICAL_SPACING: f32 = 4.0;

    /// Fixed gutter space reserved for the vertical scrollbar to guarantee that
    /// the appearance of a scrollbar does not alter available width and trigger wrapping jitter.
    pub const SCROLLBAR_GUTTER_RESERVE: f32 = 14.0;

    /// Realistic average character width for 11px font in sans-serif fonts.
    pub const CHAR_WIDTH_METRIC_BOLD: f32 = 5.8;

    /// Computes the estimated pill width for row-packing purposes.
    #[inline]
    pub fn estimate_stable_pill_width(text: &str, has_icon: bool) -> f32 {
        let clean_text = text.trim();
        let char_count = clean_text.chars().count() as f32;
        let icon_width = if has_icon { 14.0 } else { 0.0 };
        let content_w = (char_count * Self::CHAR_WIDTH_METRIC_BOLD) + icon_width;
        (content_w + Self::PILL_HORIZONTAL_PADDING).max(24.0)
    }

    /// Flow-packs tag history items into rows based on available container width.
    pub fn pack_into_rows(
        items: Vec<TagHistoryItem>,
        container_width: f32,
    ) -> (Vec<TagHistoryRow>, f32) {
        if items.is_empty() {
            return (Vec::new(), 0.0);
        }

        let available_w = (container_width - Self::SCROLLBAR_GUTTER_RESERVE).max(80.0);

        let mut rows = Vec::new();
        let mut current_row_items = Vec::new();
        let mut current_row_w = 0.0;

        for item in items {
            let item_w = item.width;
            if !current_row_items.is_empty()
                && (current_row_w + Self::PILL_HORIZONTAL_SPACING + item_w > available_w)
            {
                rows.push(TagHistoryRow {
                    items: std::mem::take(&mut current_row_items),
                });
                current_row_w = 0.0;
            }

            if !current_row_items.is_empty() {
                current_row_w += Self::PILL_HORIZONTAL_SPACING;
            }
            current_row_w += item_w;
            current_row_items.push(item);
        }

        if !current_row_items.is_empty() {
            rows.push(TagHistoryRow {
                items: current_row_items,
            });
        }

        let total_rows = rows.len();
        let total_height = if total_rows > 0 {
            (total_rows as f32 * Self::PILL_HEIGHT)
                + (total_rows.saturating_sub(1) as f32 * Self::ROW_VERTICAL_SPACING)
                + 4.0
        } else {
            0.0
        };

        (rows, total_height)
    }
}
