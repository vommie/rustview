use std::time::Instant;

/// Slot geometry descriptor for an individual dock panel tab in a sidebar tab bar.
#[derive(Debug, Clone, PartialEq)]
pub struct SidebarPanelTabSlotInfo {
    pub tab_id: String,
    pub index: usize,
    pub start_x: f32,
    pub width: f32,
    pub center_x: f32,
}

/// Pure geometric calculation engine for sidebar panel tab dragging, hysteresis, and neighbor displacement.
pub struct SidebarPanelTabDndGeometry;

impl SidebarPanelTabDndGeometry {
    /// Computes the content-based estimated tab width for sidebar dock tabs, clamped between 60px and 110px.
    pub fn estimate_tab_width(title: &str) -> f32 {
        let text_w = title.chars().count() as f32 * 7.5;
        (33.0 + text_w).clamp(60.0, 110.0)
    }

    /// Computes horizontal slots (`start_x`, width, `center_x`) for all panel tabs in sequence.
    pub fn compute_tab_slots(
        tab_ids: &[String],
        widths: &[f32],
        spacing: f32,
        padding_start: f32,
    ) -> Vec<SidebarPanelTabSlotInfo> {
        let mut slots = Vec::with_capacity(tab_ids.len());
        let mut current_x = padding_start;

        for (i, id) in tab_ids.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(80.0);
            slots.push(SidebarPanelTabSlotInfo {
                tab_id: id.clone(),
                index: i,
                start_x: current_x,
                width: w,
                center_x: current_x + (w / 2.0),
            });
            current_x += w + spacing;
        }

        slots
    }

    /// Determines the active target slot index with hysteresis protection to eliminate flickering around midpoint boundaries.
    pub fn calculate_target_index(
        slots: &[SidebarPanelTabSlotInfo],
        source_index: usize,
        dragged_center_x: f32,
        current_target_index: usize,
    ) -> usize {
        let total = slots.len();
        if total <= 1 || source_index >= total {
            return 0;
        }

        let source_idx = source_index.min(total - 1);
        let mut best_target = source_idx;

        const HYSTERESIS_PX: f32 = 6.0;

        if dragged_center_x > slots[source_idx].center_x {
            for (idx, slot) in slots.iter().enumerate().skip(source_idx + 1) {
                let threshold = if idx <= current_target_index {
                    slot.center_x - HYSTERESIS_PX
                } else {
                    slot.center_x + HYSTERESIS_PX
                };

                if dragged_center_x >= threshold {
                    best_target = idx;
                } else {
                    break;
                }
            }
        } else if dragged_center_x < slots[source_idx].center_x {
            for (idx, slot) in slots[..source_idx].iter().enumerate().rev() {
                let threshold = if idx >= current_target_index {
                    slot.center_x + HYSTERESIS_PX
                } else {
                    slot.center_x - HYSTERESIS_PX
                };

                if dragged_center_x <= threshold {
                    best_target = idx;
                } else {
                    break;
                }
            }
        }

        best_target.min(total - 1)
    }

    /// Computes exact relative shift offsets in pixels for each neighbor tab to smoothly push them aside and make room.
    pub fn compute_shift_offsets(
        slots: &[SidebarPanelTabSlotInfo],
        source_index: usize,
        target_index: usize,
        spacing: f32,
    ) -> Vec<f32> {
        let total = slots.len();
        let mut shifts = vec![0.0f32; total];
        if total <= 1 || source_index >= total || target_index >= total {
            return shifts;
        }

        let dragged_item_width = slots[source_index].width + spacing;

        if source_index < target_index {
            for shift in &mut shifts[(source_index + 1)..=target_index] {
                *shift = -dragged_item_width;
            }
        } else if source_index > target_index {
            for shift in &mut shifts[target_index..source_index] {
                *shift = dragged_item_width;
            }
        }

        shifts
    }
}

/// Active Drag and Drop session for sidebar dock panel tabs.
#[derive(Debug, Clone)]
pub struct SidebarPanelTabDndSession {
    pub is_active: bool,
    pub dragged_tab_id: String,
    pub source_index: usize,
    pub target_index: usize,
    pub grab_offset_x: f32,
    pub current_cursor_x: f32,
    pub slots: Vec<SidebarPanelTabSlotInfo>,
    pub start_time: Instant,
}

impl SidebarPanelTabDndSession {
    pub fn new(
        dragged_tab_id: String,
        source_index: usize,
        grab_offset_x: f32,
        cursor_x: f32,
        slots: Vec<SidebarPanelTabSlotInfo>,
    ) -> Self {
        Self {
            is_active: true,
            dragged_tab_id,
            source_index,
            target_index: source_index,
            grab_offset_x,
            current_cursor_x: cursor_x,
            slots,
            start_time: Instant::now(),
        }
    }

    #[inline]
    pub fn dragged_center_x(&self) -> f32 {
        let source_slot_w = self.slots.get(self.source_index).map_or(80.0, |s| s.width);
        let left = self.current_cursor_x - self.grab_offset_x;
        left + (source_slot_w / 2.0)
    }

    #[inline]
    pub fn ghost_left_x(&self) -> f32 {
        self.current_cursor_x - self.grab_offset_x
    }

    #[inline]
    pub fn current_slot_width(&self) -> f32 {
        self.slots.get(self.source_index).map_or(80.0, |s| s.width)
    }
}
