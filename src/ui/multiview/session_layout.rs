//! Session behavior around geometry: solving the layout, moving the video windows, gutter drag/swap/reset and clicks.

use super::layout::{self, Gutter, GutterKind, Layout, LayoutParams, ManualSplit, Split};
use super::session::{After, Session};
use super::views;
use crate::settings::schema::{
    MULTIVIEW_DOUBLE_CLICK_MS, MULTIVIEW_GUTTER_LOGICAL_PX, MULTIVIEW_MIN_COLUMN_LOGICAL_PX,
    MULTIVIEW_SNAP_LOGICAL_PX, MULTIVIEW_WRAP_MIN_ASPECT,
};
use slint::ComponentHandle;
use std::time::{Duration, Instant};

/// A gutter drag in progress: distances are always applied to the proportions at drag start, never accumulated.
pub(super) struct DragState {
    gutter: Gutter,
    start: ManualSplit,
    start_layout: Layout,
}

impl Session {
    /// Reads the window size and scale factor. Returns `true` if the layout parameters changed.
    pub(super) fn refresh_geometry(&mut self) -> bool {
        let Some(win) = self.window.upgrade() else {
            return false;
        };
        let size = win.window().size();
        let scale = win.window().scale_factor().max(0.5);
        let params = LayoutParams {
            width: size.width.max(1) as i32,
            height: size.height.max(1) as i32,
            gutter: (MULTIVIEW_GUTTER_LOGICAL_PX * scale).round() as i32,
            min_column: (MULTIVIEW_MIN_COLUMN_LOGICAL_PX * scale).round() as i32,
            min_wide_aspect: MULTIVIEW_WRAP_MIN_ASPECT,
        };
        let changed = params != self.params || (scale - self.scale).abs() > f32::EPSILON;
        self.params = params;
        self.scale = scale;
        changed
    }

    pub(super) fn on_size_changed(&mut self) {
        if self.populated && self.refresh_geometry() {
            tracing::debug!(
                width = self.params.width,
                height = self.params.height,
                scale = self.scale,
                "[Multiview: Layout] Window geometry changed"
            );
            self.apply_layout();
        }
    }

    fn aspects(&self) -> Vec<f32> {
        self.tiles.iter().map(|t| t.aspect).collect()
    }

    pub(super) fn apply_layout(&mut self) {
        self.drop_badge();
        let aspects = self.aspects();
        let mut solved = layout::solve(&self.params, &aspects, &self.split);
        if matches!(self.split, Split::Manual(_)) && !layout::manual_fits(&self.split, &solved.rows)
        {
            self.split = Split::Auto;
            solved = layout::solve(&self.params, &aspects, &self.split);
        }
        self.layout = solved;

        for (tile, placed) in self.tiles.iter_mut().zip(&self.layout.tiles) {
            if let Some(video) = tile.video.as_mut() {
                let c = placed.content;
                video
                    .window
                    .move_resize(c.x, c.y, c.w.max(1) as u32, c.h.max(1) as u32);
            }
        }
        self.sync_models();
        self.refresh_badge();
    }

    pub(super) fn sync_models(&self) {
        let tiles = self
            .tiles
            .iter()
            .zip(&self.layout.tiles)
            .map(|(tile, placed)| views::tile_view(tile, placed.content, self.scale))
            .collect();
        views::sync_model(&self.tile_model, tiles);
        let gutters = self
            .layout
            .gutters
            .iter()
            .map(|g| views::gutter_view(g, self.scale))
            .collect();
        views::sync_model(&self.gutter_model, gutters);
    }

    pub(super) fn drag_started(&mut self, index: usize) {
        let Some(gutter) = self.layout.gutters.get(index).copied() else {
            return;
        };
        let start = match &self.split {
            Split::Manual(manual) => manual.clone(),
            Split::Auto => self.layout.to_manual(),
        };
        self.drag = Some(DragState {
            gutter,
            start,
            start_layout: self.layout.clone(),
        });
    }

    /// `distance` is the pointer travel since the drag started, in logical pixels.
    pub(super) fn dragged(&mut self, distance: f32) {
        let aspects = self.aspects();
        let Some(drag) = self.drag.as_ref() else {
            return;
        };
        let delta = (distance * self.scale).round() as i32;
        let snap = (MULTIVIEW_SNAP_LOGICAL_PX * self.scale).round() as i32;
        let next = layout::drag(
            &drag.start,
            &drag.start_layout,
            &drag.gutter,
            delta,
            &self.params,
            &aspects,
            snap,
        );
        self.split = Split::Manual(next);
        self.apply_layout();
    }

    pub(super) fn reset_split(&mut self) {
        self.drag = None;
        self.split = Split::Auto;
        self.apply_layout();
        tracing::debug!("[Multiview: Layout] Split reset to automatic");
    }

    /// Exchanges the two tiles next to a vertical gutter, or the two rows of a 2x2 grid. Sound states travel with the tiles.
    pub(super) fn swap(&mut self, index: usize) {
        let Some(gutter) = self.layout.gutters.get(index).copied() else {
            return;
        };
        self.drag = None;
        self.clear_hover();
        match gutter.kind {
            GutterKind::Vertical => {
                let Some(row) = self.layout.rows.get(gutter.row) else {
                    return;
                };
                let (Some(&a), Some(&b)) = (row.get(gutter.left), row.get(gutter.left + 1)) else {
                    return;
                };
                self.tiles.swap(a, b);
                if let Split::Manual(manual) = &mut self.split {
                    manual.swap_columns(gutter.row, gutter.left);
                }
            }
            GutterKind::Horizontal => {
                if self.tiles.len() != 4 {
                    return;
                }
                self.tiles.swap(0, 2);
                self.tiles.swap(1, 3);
                if let Split::Manual(manual) = &mut self.split {
                    manual.swap_rows();
                }
            }
        }
        self.apply_layout();
        tracing::debug!(gutter = index, "[Multiview: Layout] Tiles swapped");
    }

    pub(super) fn on_click(&mut self, is_ctrl: bool) -> After {
        let now = Instant::now();
        let window = Duration::from_millis(MULTIVIEW_DOUBLE_CLICK_MS);
        let is_double = matches!(self.last_click, Some((at, ctrl)) if ctrl == is_ctrl && now.duration_since(at) < window);
        if is_double {
            self.last_click = None;
            tracing::info!(
                is_ctrl,
                "[Multiview: Click] Double click closes the Multiview"
            );
            After::Close
        } else {
            self.last_click = Some((now, is_ctrl));
            After::Nothing
        }
    }
}
