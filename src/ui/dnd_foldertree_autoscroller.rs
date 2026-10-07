use crate::ui::MainWindow;
use slint::{ComponentHandle, Model, Timer};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// Constants controlling FolderTree auto-scroll physics and geometry
pub const FOLDER_TREE_AUTOSCROLL_EDGE_ZONE_PX: f32 = 48.0;
pub const FOLDER_TREE_AUTOSCROLL_BASE_SPEED_PX: f32 = 3.0;
pub const FOLDER_TREE_AUTOSCROLL_MAX_SPEED_PX: f32 = 18.0;
pub const FOLDER_TREE_AUTOSCROLL_ACCEL_DURATION_SECS: f32 = 1.5;
pub const FOLDER_TREE_AUTOSCROLL_TICK_INTERVAL_MS: u64 = 16; // ~60 FPS

/// Fixed Y offset from window top to the first folder row:
/// TopToolbar (38px) + Sidebar padding (8px) + SidebarPanelTabBar (28px) + spacing (6px) + FolderPanelToolbar (26px) + spacing (6px) = 112px.
pub const FOLDER_TREE_TOP_OFFSET_PX: f32 = 112.0;
pub const FOLDER_TREE_ROW_HEIGHT_PX: f32 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderTreeAutoScrollDirection {
    Up,
    Down,
    Idle,
}

#[derive(Default, Clone)]
pub struct DndFolderTreeAutoScroller {
    inner: Rc<RefCell<FolderTreeAutoScrollerInner>>,
}

struct FolderTreeAutoScrollerInner {
    direction: FolderTreeAutoScrollDirection,
    zone_enter_time: Option<Instant>,
    cursor_x: f32,
    cursor_y: f32,
    proximity: f32,
    timer: Option<Timer>,
    last_logged_tick: Option<Instant>,
}

impl Default for FolderTreeAutoScrollerInner {
    fn default() -> Self {
        Self {
            direction: FolderTreeAutoScrollDirection::Idle,
            zone_enter_time: None,
            cursor_x: 0.0,
            cursor_y: 0.0,
            proximity: 0.0,
            timer: None,
            last_logged_tick: None,
        }
    }
}

impl DndFolderTreeAutoScroller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates cursor position relative to the FolderTree boundaries and starts/adjusts or stops auto-scrolling.
    pub fn update_cursor(&self, window: &MainWindow, cursor_x: f32, cursor_y: f32) {
        let active_tab = window.get_active_sidebar_panel_tab();
        if active_tab.as_str() != "folders" {
            self.stop();
            return;
        }

        let sidebar_w = window.get_sidebar_left_width();
        let tree_container_h = window.get_folder_tree_container_height();
        let tree_bottom_y = FOLDER_TREE_TOP_OFFSET_PX + tree_container_h;

        let in_sidebar_x = cursor_x >= 0.0 && cursor_x <= sidebar_w;
        let in_tree_y = cursor_y >= FOLDER_TREE_TOP_OFFSET_PX && cursor_y <= tree_bottom_y;

        if !in_sidebar_x || !in_tree_y {
            self.stop();
            return;
        }

        // 1. Top Hot Zone (Scroll UP)
        if cursor_y <= FOLDER_TREE_TOP_OFFSET_PX + FOLDER_TREE_AUTOSCROLL_EDGE_ZONE_PX {
            let dist_from_top = (cursor_y - FOLDER_TREE_TOP_OFFSET_PX).max(0.0);
            let proximity = 1.0 - (dist_from_top / FOLDER_TREE_AUTOSCROLL_EDGE_ZONE_PX).clamp(0.0, 1.0);
            self.set_active_zone(window, FolderTreeAutoScrollDirection::Up, cursor_x, cursor_y, proximity);
            return;
        }

        // 2. Bottom Hot Zone (Scroll DOWN)
        if cursor_y >= tree_bottom_y - FOLDER_TREE_AUTOSCROLL_EDGE_ZONE_PX {
            let dist_from_bottom = (tree_bottom_y - cursor_y).max(0.0);
            let proximity = 1.0 - (dist_from_bottom / FOLDER_TREE_AUTOSCROLL_EDGE_ZONE_PX).clamp(0.0, 1.0);
            self.set_active_zone(window, FolderTreeAutoScrollDirection::Down, cursor_x, cursor_y, proximity);
            return;
        }

        // 3. Neutral Zone
        self.stop();
    }

    fn set_active_zone(
        &self,
        window: &MainWindow,
        direction: FolderTreeAutoScrollDirection,
        cursor_x: f32,
        cursor_y: f32,
        proximity: f32,
    ) {
        let mut inner = self.inner.borrow_mut();
        inner.cursor_x = cursor_x;
        inner.cursor_y = cursor_y;
        inner.proximity = proximity;

        if inner.direction == direction && inner.timer.is_some() {
            return;
        }

        inner.direction = direction;
        inner.zone_enter_time = Some(Instant::now());

        tracing::info!(
            ?direction,
            proximity = format!("{:.2}", proximity),
            cursor_x,
            cursor_y,
            "[DndFolderTreeAutoScroll: Active] Cursor entered auto-scroll hot zone in folder tree"
        );

        let scroller_weak = self.clone();
        let win_weak = window.as_weak();

        let timer = Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(FOLDER_TREE_AUTOSCROLL_TICK_INTERVAL_MS),
            move || {
                if let Some(w) = win_weak.upgrade() {
                    scroller_weak.tick(&w);
                }
            },
        );
        inner.timer = Some(timer);
    }

    /// Executes a single auto-scroll tick, computes dynamic velocity acceleration, applies scroll delta, and updates drop target hover.
    pub fn tick(&self, window: &MainWindow) {
        let (dir, elapsed_secs, proximity, cursor_y) = {
            let inner = self.inner.borrow();
            if inner.direction == FolderTreeAutoScrollDirection::Idle {
                return;
            }
            let elapsed = inner
                .zone_enter_time
                .map(|t| t.elapsed().as_secs_f32())
                .unwrap_or(0.0);
            (inner.direction, elapsed, inner.proximity, inner.cursor_y)
        };

        // Smooth acceleration physics: base speed + non-linear power curve over time, scaled by edge proximity
        let accel_ratio = (elapsed_secs / FOLDER_TREE_AUTOSCROLL_ACCEL_DURATION_SECS).clamp(0.0, 1.0);
        let time_boost = (FOLDER_TREE_AUTOSCROLL_MAX_SPEED_PX - FOLDER_TREE_AUTOSCROLL_BASE_SPEED_PX) * accel_ratio.powf(1.4);
        let proximity_factor = 0.35 + (0.65 * proximity);
        let current_speed = (FOLDER_TREE_AUTOSCROLL_BASE_SPEED_PX + time_boost) * proximity_factor;

        let prev_scroll_y = window.get_folder_tree_scroll_y();

        let delta_y = match dir {
            FolderTreeAutoScrollDirection::Up => current_speed,
            FolderTreeAutoScrollDirection::Down => -current_speed,
            FolderTreeAutoScrollDirection::Idle => 0.0,
        };

        // Apply scroll delta to Slint UI
        window.invoke_scroll_folder_tree_by(delta_y);

        let new_scroll_y = window.get_folder_tree_scroll_y();

        // Hit-test update while content scrolls underneath cursor
        let local_y = cursor_y - FOLDER_TREE_TOP_OFFSET_PX - new_scroll_y;
        let row_idx = (local_y / FOLDER_TREE_ROW_HEIGHT_PX).floor() as i32;
        let folders = window.get_folders();

        if row_idx >= 0
            && (row_idx as usize) < folders.row_count()
            && let Some(row) = folders.row_data(row_idx as usize)
            && !row.is_deleted
        {
            let target_id = row.id as u64;
            if window.get_drop_target_folder_id() != target_id as i32 {
                tracing::info!(
                    target_folder_id = target_id,
                    folder_name = %row.name,
                    row_index = row_idx,
                    scroll_offset_y = new_scroll_y,
                    "[DndFolderTreeAutoScroll: TargetChange] Auto-scrolled to new drop target folder"
                );
                window.set_drop_target_folder_id(target_id as i32);
            }
        }

        // Boundary check logging
        if (new_scroll_y - prev_scroll_y).abs() < 0.01 {
            let mut inner = self.inner.borrow_mut();
            let should_log = inner
                .last_logged_tick
                .map(|t| t.elapsed().as_millis() >= 500)
                .unwrap_or(true);
            if should_log {
                inner.last_logged_tick = Some(Instant::now());
                tracing::debug!(
                    ?dir,
                    scroll_offset_y = new_scroll_y,
                    "[DndFolderTreeAutoScroll: Boundary] Reached scroll boundary limit in folder tree"
                );
            }
        }
    }

    /// Returns true if auto-scrolling is currently active.
    pub fn is_active(&self) -> bool {
        self.inner.borrow().direction != FolderTreeAutoScrollDirection::Idle
    }

    /// Stops auto-scrolling immediately and resets the acceleration timer.
    pub fn stop(&self) {
        let mut inner = self.inner.borrow_mut();
        if inner.direction != FolderTreeAutoScrollDirection::Idle || inner.timer.is_some() {
            tracing::debug!(
                prev_direction = ?inner.direction,
                "[DndFolderTreeAutoScroll: Stop] Auto-scrolling stopped"
            );
            inner.direction = FolderTreeAutoScrollDirection::Idle;
            inner.zone_enter_time = None;
            inner.timer = None;
            inner.proximity = 0.0;
        }
    }
}
