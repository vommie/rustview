use crate::viewer::{MediaTabSession, TabId};
use rustview_core::util::cast::Az;
use slint::Timer;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// Edge auto-scroll constants for the Tab Bar
pub const TAB_BAR_AUTOSCROLL_EDGE_ZONE_PX: f32 = 36.0;
pub const TAB_BAR_AUTOSCROLL_BASE_SPEED_PX: f32 = 4.0;
pub const TAB_BAR_AUTOSCROLL_MAX_SPEED_PX: f32 = 20.0;
pub const TAB_BAR_AUTOSCROLL_TICK_INTERVAL_MS: u64 = 16; // ~60 FPS
pub const TAB_BAR_DRAG_THRESHOLD_SQ_PX: f32 = 25.0; // 5px movement threshold before drag starts

/// Geometric slot information for a single tab in the horizontal tab bar layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabSlotInfo {
    pub tab_id: TabId,
    pub index: usize,
    pub start_x: f32,
    pub width: f32,
    pub center_x: f32,
}

/// Pure geometric calculation engine for tab drag-and-drop slot detection, width estimation, and neighbor displacement.
pub struct TabDndGeometry;

impl TabDndGeometry {
    /// Calculates the estimated rendered width in pixels of a tab item given its title, format, and rating badge state.
    pub fn estimate_tab_width(title: &str, has_rating_badge: bool) -> f32 {
        let char_count = title.chars().count().az::<f32>();
        let text_w = (char_count * 7.5).min(180.0);
        let base_w = if has_rating_badge { 77.0 } else { 57.0 };
        (base_w + text_w).clamp(70.0, 240.0)
    }

    /// Computes unshifted horizontal slots (`start_x`, width, `center_x`) for all tabs in sequence.
    pub fn compute_tab_slots(tab_ids: &[TabId], widths: &[f32], spacing: f32, padding_start: f32) -> Vec<TabSlotInfo> {
        let mut slots = Vec::with_capacity(tab_ids.len());
        let mut current_x = padding_start;

        for (i, &id) in tab_ids.iter().enumerate() {
            let w = widths.get(i).copied().unwrap_or(120.0);
            slots.push(TabSlotInfo {
                tab_id: id,
                index: i,
                start_x: current_x,
                width: w,
                center_x: current_x + (w / 2.0),
            });
            current_x += w + spacing;
        }

        slots
    }

    /// Computes the exact horizontal slot geometry (`start_x`, width) for a specific tab ID among open sessions.
    pub fn find_tab_slot_geometry(
        tabs: &[MediaTabSession],
        target_tab_id: TabId,
        show_rating_badge: bool,
    ) -> Option<(f32, f32)> {
        let mut current_x = 2.0f32; // padding-left: 2px in tab-layout
        const SPACING: f32 = 4.0f32; // spacing: 4px in tab-layout

        for session in tabs {
            let has_rating = show_rating_badge && session.record.rating > 0;
            let width = Self::estimate_tab_width(&session.title, has_rating);

            if session.id == target_tab_id {
                return Some((current_x, width));
            }

            current_x += width + SPACING;
        }

        None
    }

    /// Determines the active target slot index with hysteresis protection to eliminate flickering around midpoint boundaries.
    pub fn calculate_target_index(
        slots: &[TabSlotInfo],
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

        // 6px hysteresis margin prevents rapid flip-flopping at exact midpoint boundaries
        const HYSTERESIS_PX: f32 = 6.0;

        if dragged_center_x > slots[source_idx].center_x {
            // Dragging to the RIGHT
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
            // Dragging to the LEFT
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
        slots: &[TabSlotInfo],
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
            // Dragged RIGHT: Intermediate tabs shift LEFT (-dragged_item_width)
            for shift in &mut shifts[(source_index + 1)..=target_index] {
                *shift = -dragged_item_width;
            }
        } else if source_index > target_index {
            // Dragged LEFT: Intermediate tabs shift RIGHT (+dragged_item_width)
            for shift in &mut shifts[target_index..source_index] {
                *shift = dragged_item_width;
            }
        }

        shifts
    }
}

/// Active Tab Drag and Drop state session.
#[derive(Debug, Clone)]
pub struct TabDndSession {
    pub is_active: bool,
    pub dragged_tab_id: TabId,
    pub source_index: usize,
    pub target_index: usize,
    pub grab_offset_x: f32,
    pub current_cursor_x: f32,
    pub slots: Vec<TabSlotInfo>,
    pub start_time: Instant,
}

impl TabDndSession {
    pub fn new(
        dragged_tab_id: TabId,
        source_index: usize,
        grab_offset_x: f32,
        cursor_x: f32,
        slots: Vec<TabSlotInfo>,
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
        let source_slot_w = self.slots.get(self.source_index).map_or(120.0, |s| s.width);
        let left = self.current_cursor_x - self.grab_offset_x;
        left + (source_slot_w / 2.0)
    }

    #[inline]
    pub fn ghost_left_x(&self) -> f32 {
        self.current_cursor_x - self.grab_offset_x
    }
}

/// Direction of horizontal auto-scrolling along the tab bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabAutoScrollDirection {
    Left,
    Right,
    Idle,
}

/// High-precision edge auto-scroller for active tab bar drag sessions.
#[derive(Default, Clone)]
pub struct TabDndAutoScroller {
    inner: Rc<RefCell<TabDndAutoScrollerInner>>,
}

struct TabDndAutoScrollerInner {
    direction: TabAutoScrollDirection,
    proximity: f32,
    timer: Option<Timer>,
}

impl Default for TabDndAutoScrollerInner {
    fn default() -> Self {
        Self {
            direction: TabAutoScrollDirection::Idle,
            proximity: 0.0,
            timer: None,
        }
    }
}

impl TabDndAutoScroller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates cursor position relative to the horizontal tab bar viewport and triggers edge auto-scrolling.
    pub fn update_cursor<F>(&self, cursor_x: f32, viewport_width: f32, on_tick: F)
    where
        F: Fn(f32) + 'static,
    {
        if viewport_width <= 0.0 {
            self.stop();
            return;
        }

        // 1. Left Hot Zone (Scroll Content RIGHT / Viewport LEFT)
        if (-20.0..=TAB_BAR_AUTOSCROLL_EDGE_ZONE_PX).contains(&cursor_x) {
            let dist = cursor_x.max(0.0);
            let proximity = 1.0 - (dist / TAB_BAR_AUTOSCROLL_EDGE_ZONE_PX).clamp(0.0, 1.0);
            self.set_active_zone(TabAutoScrollDirection::Left, proximity, on_tick);
            return;
        }

        // 2. Right Hot Zone (Scroll Content LEFT / Viewport RIGHT)
        let right_start = viewport_width - TAB_BAR_AUTOSCROLL_EDGE_ZONE_PX;
        let right_end = viewport_width + 20.0;
        if (right_start..=right_end).contains(&cursor_x) {
            let dist = (viewport_width - cursor_x).max(0.0);
            let proximity = 1.0 - (dist / TAB_BAR_AUTOSCROLL_EDGE_ZONE_PX).clamp(0.0, 1.0);
            self.set_active_zone(TabAutoScrollDirection::Right, proximity, on_tick);
            return;
        }

        // 3. Neutral Center Zone
        self.stop();
    }

    fn set_active_zone<F>(&self, direction: TabAutoScrollDirection, proximity: f32, on_tick: F)
    where
        F: Fn(f32) + 'static,
    {
        let mut inner = self.inner.borrow_mut();
        inner.proximity = proximity;

        if inner.direction == direction && inner.timer.is_some() {
            return;
        }

        inner.direction = direction;
        let inner_ref = self.inner.clone();
        let cb = Rc::new(on_tick);

        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(TAB_BAR_AUTOSCROLL_TICK_INTERVAL_MS),
            move || {
                let (dir, prox) = {
                    let guard = inner_ref.borrow();
                    (guard.direction, guard.proximity)
                };

                if dir == TabAutoScrollDirection::Idle {
                    return;
                }

                let speed_boost = (TAB_BAR_AUTOSCROLL_MAX_SPEED_PX - TAB_BAR_AUTOSCROLL_BASE_SPEED_PX) * prox.powf(1.4);
                let current_speed = TAB_BAR_AUTOSCROLL_BASE_SPEED_PX + speed_boost;

                let delta_x = match dir {
                    TabAutoScrollDirection::Left => current_speed,
                    TabAutoScrollDirection::Right => -current_speed,
                    TabAutoScrollDirection::Idle => 0.0,
                };

                cb(delta_x);
            },
        );

        inner.timer = Some(timer);
    }

    pub fn stop(&self) {
        let mut inner = self.inner.borrow_mut();
        if inner.direction != TabAutoScrollDirection::Idle || inner.timer.is_some() {
            inner.direction = TabAutoScrollDirection::Idle;
            inner.proximity = 0.0;
            inner.timer = None;
        }
    }

    #[inline]
    pub fn is_active(&self) -> bool {
        self.inner.borrow().direction != TabAutoScrollDirection::Idle
    }
}
