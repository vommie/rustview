use crate::ui::MainWindow;
use rustview_core::library::models::{FolderId, MediaId};
use rustview_core::util::cast::{Az, FloatToInt};
use slint::{ComponentHandle, Timer};
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Constants controlling Rubberband auto-scroll edge detection and velocity curve
pub const RUBBERBAND_AUTOSCROLL_EDGE_ZONE_PX: f32 = 40.0;
pub const RUBBERBAND_AUTOSCROLL_BASE_SPEED_PX: f32 = 4.0;
pub const RUBBERBAND_AUTOSCROLL_MAX_SPEED_PX: f32 = 24.0;
pub const RUBBERBAND_AUTOSCROLL_TICK_INTERVAL_MS: u64 = 16; // ~60 FPS
pub const MEDIA_HEADER_BAR_HEIGHT_PX: f32 = 28.0;

/// Geometric bounding box rectangle in 2D coordinate space.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RectF32 {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectF32 {
    #[inline]
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width: width.max(0.0),
            height: height.max(0.0),
        }
    }

    #[inline]
    pub fn from_points(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        let min_x = x1.min(x2);
        let min_y = y1.min(y2);
        let w = (x1 - x2).abs();
        let h = (y1 - y2).abs();
        Self {
            x: min_x,
            y: min_y,
            width: w,
            height: h,
        }
    }

    #[inline]
    pub fn intersects(&self, other: &RectF32) -> bool {
        let r1_right = self.x + self.width;
        let r1_bottom = self.y + self.height;
        let r2_right = other.x + other.width;
        let r2_bottom = other.y + other.height;

        self.x < r2_right && r1_right > other.x && self.y < r2_bottom && r1_bottom > other.y
    }
}

/// Geometric layout parameters for Grid View item bounding box projection.
#[derive(Debug, Clone, Copy)]
pub struct GridLayoutConfig {
    pub columns: usize,
    pub item_tile_width: f32,
    pub item_tile_height: f32,
    pub grid_spacing: f32,
    pub grid_padding: f32,
}

/// Geometric layout parameters for Table View row bounding box projection.
#[derive(Debug, Clone, Copy)]
pub struct TableLayoutConfig {
    pub row_height: f32,
    pub content_width: f32,
}

/// Pure geometric intersection evaluator for virtualized media views.
pub struct RubberbandGeometry;

impl RubberbandGeometry {
    /// The list positions of the cells of a Grid View that the selection rectangle (Content Space) touches; `total_items` is the length
    /// of the whole list (folder rows and media). O(cells under the rectangle).
    pub fn grid_hit_positions(rect: RectF32, total_items: usize, config: GridLayoutConfig) -> Vec<usize> {
        if total_items == 0 || config.columns == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
            return Vec::new();
        }

        let col_stride = config.item_tile_width + config.grid_spacing;
        let row_stride = config.item_tile_height + config.grid_spacing;

        let rect_right = rect.x + rect.width;
        let rect_bottom = rect.y + rect.height;

        // O(1) Mathematical row and column index boundaries bounding the box
        let min_r = ((rect.y - config.grid_padding).max(0.0) / row_stride.max(1.0))
            .floor()
            .to_int::<usize>();
        let max_r = ((rect_bottom - config.grid_padding).max(0.0) / row_stride.max(1.0))
            .floor()
            .to_int::<usize>();

        let min_c = ((rect.x - config.grid_padding).max(0.0) / col_stride.max(1.0))
            .floor()
            .to_int::<usize>();
        let max_c = ((rect_right - config.grid_padding).max(0.0) / col_stride.max(1.0))
            .floor()
            .to_int::<usize>();

        let max_c_clamped = max_c.min(config.columns.saturating_sub(1));
        let mut hits = Vec::new();

        for r in min_r..=max_r {
            let row_y = config.grid_padding + (r.az::<f32>() * row_stride);
            let item_h = config.item_tile_height;

            if row_y >= rect_bottom || (row_y + item_h) <= rect.y {
                continue;
            }

            for c in min_c..=max_c_clamped {
                let item_idx = (r * config.columns) + c;
                if item_idx >= total_items {
                    break;
                }

                let item_x = config.grid_padding + (c.az::<f32>() * col_stride);
                let item_rect = RectF32::new(item_x, row_y, config.item_tile_width, item_h);
                if rect.intersects(&item_rect) {
                    hits.push(item_idx);
                }
            }
        }

        hits
    }

    /// The list positions of the rows of a Table View that the selection rectangle (Content Space) touches.
    pub fn table_hit_positions(rect: RectF32, total_items: usize, config: TableLayoutConfig) -> Vec<usize> {
        if total_items == 0 || config.row_height <= 0.0 || rect.width <= 0.0 || rect.height <= 0.0 {
            return Vec::new();
        }

        // Verify horizontal overlap with table content width
        if rect.x >= config.content_width || (rect.x + rect.width) <= 0.0 {
            return Vec::new();
        }

        let min_r = (rect.y.max(0.0) / config.row_height).floor().to_int::<usize>();
        let max_r = ((rect.y + rect.height).max(0.0) / config.row_height)
            .floor()
            .to_int::<usize>();
        let max_r_clamped = max_r.min(total_items.saturating_sub(1));

        (min_r..=max_r_clamped)
            .filter(|&r| {
                rect.intersects(&RectF32::new(
                    0.0,
                    r.az::<f32>() * config.row_height,
                    config.content_width,
                    config.row_height,
                ))
            })
            .collect()
    }

    /// Applies modifier arithmetic to derive the target selection set from the baseline and intersected items.
    #[inline]
    pub fn evaluate_modifier_selection<T: Copy + Eq + std::hash::Hash>(
        baseline: &HashSet<T>,
        intersected: &HashSet<T>,
        is_ctrl: bool,
        is_shift: bool,
    ) -> HashSet<T> {
        if is_ctrl {
            // Invert / Toggle / Symmetric Difference: (Baseline \ Intersected) ∪ (Intersected \ Baseline)
            let mut result = baseline.clone();
            for &id in intersected {
                if result.contains(&id) {
                    result.remove(&id);
                } else {
                    result.insert(id);
                }
            }
            result
        } else if is_shift {
            // Additive / Union: Baseline ∪ Intersected
            let mut result = baseline.clone();
            result.extend(intersected);
            result
        } else {
            // Replace / Normal: Intersected items only
            intersected.clone()
        }
    }
}

/// Parameter container grouping start parameters for a rubberband drag session.
#[derive(Debug, Clone)]
pub struct RubberbandStartParams {
    pub start_content_x: f32,
    pub start_content_y: f32,
    pub view_x: f32,
    pub view_y: f32,
    pub is_ctrl: bool,
    pub is_shift: bool,
    pub baseline: HashSet<MediaId>,
    pub baseline_folders: HashSet<FolderId>,
}

/// Active Rubberband Selection drag state session.
#[derive(Debug, Clone, Default)]
pub struct RubberbandSession {
    pub is_active: bool,
    pub start_content_x: f32,
    pub start_content_y: f32,
    pub current_content_x: f32,
    pub current_content_y: f32,
    pub last_view_x: f32,
    pub last_view_y: f32,
    pub is_ctrl: bool,
    pub is_shift: bool,
    pub baseline_selection: HashSet<MediaId>,
    pub baseline_folders: HashSet<FolderId>,
}

impl RubberbandSession {
    pub fn start(&mut self, params: RubberbandStartParams) {
        self.is_active = true;
        self.start_content_x = params.start_content_x;
        self.start_content_y = params.start_content_y;
        self.current_content_x = params.start_content_x;
        self.current_content_y = params.start_content_y;
        self.last_view_x = params.view_x;
        self.last_view_y = params.view_y;
        self.is_ctrl = params.is_ctrl;
        self.is_shift = params.is_shift;
        self.baseline_selection = params.baseline;
        self.baseline_folders = params.baseline_folders;
    }

    #[inline]
    pub fn update_content_point(&mut self, content_x: f32, content_y: f32, view_x: f32, view_y: f32) {
        self.current_content_x = content_x;
        self.current_content_y = content_y;
        self.last_view_x = view_x;
        self.last_view_y = view_y;
    }

    #[inline]
    pub fn content_rect(&self) -> RectF32 {
        RectF32::from_points(
            self.start_content_x,
            self.start_content_y,
            self.current_content_x,
            self.current_content_y,
        )
    }

    pub fn reset(&mut self) {
        self.is_active = false;
        self.start_content_x = 0.0;
        self.start_content_y = 0.0;
        self.current_content_x = 0.0;
        self.current_content_y = 0.0;
        self.last_view_x = 0.0;
        self.last_view_y = 0.0;
        self.is_ctrl = false;
        self.is_shift = false;
        self.baseline_selection.clear();
        self.baseline_folders.clear();
    }
}

/// High-precision edge auto-scroller for active Rubberband selection drags.
#[derive(Default, Clone)]
pub struct RubberbandAutoScroller {
    inner: Rc<RefCell<RubberbandAutoScrollerInner>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RubberbandScrollDirection {
    Up,
    Down,
    Idle,
}

struct RubberbandAutoScrollerInner {
    direction: RubberbandScrollDirection,
    proximity: f32,
    timer: Option<Timer>,
}

impl Default for RubberbandAutoScrollerInner {
    fn default() -> Self {
        Self {
            direction: RubberbandScrollDirection::Idle,
            proximity: 0.0,
            timer: None,
        }
    }
}

impl RubberbandAutoScroller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates cursor position relative to the Media Viewport bounds and starts/updates or stops edge auto-scrolling.
    pub fn update_cursor<F>(&self, window: &MainWindow, view_y: f32, on_tick: F)
    where
        F: Fn(f32) + 'static,
    {
        let window_h = window.window().size().height.az::<f32>();
        let viewport_top = MEDIA_HEADER_BAR_HEIGHT_PX; // ~28px header
        let viewport_bottom = window_h - 26.0; // the status bar (26 px, `ui/status_bar.slint`)
        self.update_cursor_in(view_y, viewport_top, viewport_bottom, on_tick);
    }

    /// The same for a viewport that spans `top..bottom` in the coordinate space of `y` (the drag of media uses window coordinates).
    pub fn update_cursor_in<F>(&self, y: f32, top: f32, bottom: f32, on_tick: F)
    where
        F: Fn(f32) + 'static,
    {
        // 1. Top Hot Zone (Scroll UP)
        if y <= top + RUBBERBAND_AUTOSCROLL_EDGE_ZONE_PX && y >= top - 20.0 {
            let dist = (y - top).max(0.0);
            let proximity = 1.0 - (dist / RUBBERBAND_AUTOSCROLL_EDGE_ZONE_PX).clamp(0.0, 1.0);
            self.set_active_zone(RubberbandScrollDirection::Up, proximity, on_tick);
            return;
        }

        // 2. Bottom Hot Zone (Scroll DOWN)
        if y >= bottom - RUBBERBAND_AUTOSCROLL_EDGE_ZONE_PX && y <= bottom + 20.0 {
            let dist = (bottom - y).max(0.0);
            let proximity = 1.0 - (dist / RUBBERBAND_AUTOSCROLL_EDGE_ZONE_PX).clamp(0.0, 1.0);
            self.set_active_zone(RubberbandScrollDirection::Down, proximity, on_tick);
            return;
        }

        // 3. Neutral Zone
        self.stop();
    }

    fn set_active_zone<F>(&self, direction: RubberbandScrollDirection, proximity: f32, on_tick: F)
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
            std::time::Duration::from_millis(RUBBERBAND_AUTOSCROLL_TICK_INTERVAL_MS),
            move || {
                let (dir, prox) = {
                    let guard = inner_ref.borrow();
                    (guard.direction, guard.proximity)
                };

                if dir == RubberbandScrollDirection::Idle {
                    return;
                }

                let speed_boost =
                    (RUBBERBAND_AUTOSCROLL_MAX_SPEED_PX - RUBBERBAND_AUTOSCROLL_BASE_SPEED_PX) * prox.powf(1.5);
                let current_speed = RUBBERBAND_AUTOSCROLL_BASE_SPEED_PX + speed_boost;

                let delta_y = match dir {
                    RubberbandScrollDirection::Up => current_speed,
                    RubberbandScrollDirection::Down => -current_speed,
                    RubberbandScrollDirection::Idle => 0.0,
                };

                cb(delta_y);
            },
        );

        inner.timer = Some(timer);
    }

    /// Stops auto-scrolling immediately.
    pub fn stop(&self) {
        let mut inner = self.inner.borrow_mut();
        if inner.direction != RubberbandScrollDirection::Idle || inner.timer.is_some() {
            inner.direction = RubberbandScrollDirection::Idle;
            inner.proximity = 0.0;
            inner.timer = None;
        }
    }

    /// Returns true if edge auto-scrolling is actively running.
    pub fn is_active(&self) -> bool {
        self.inner.borrow().direction != RubberbandScrollDirection::Idle
    }
}
