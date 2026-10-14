//! Instance behavior around the arrangement: solving the layout in container pixels, gutter drag, swap and reset.
//!
//! The widths the user sets by dragging a gutter are pins on the two neighboring tiles (`TileState::pin`), so they stay
//! with the tiles when media join or leave; everything the user never touched adapts automatically.

use super::container::Container;
use super::instance::MultiviewInstance;
use super::layout::{self, Gutter, GutterKind, Layout, LayoutParams};
use super::layout_pins::{self, Pins};
use super::model::TileKind;
use crate::settings::schema::{
    MULTIVIEW_GUTTER_LOGICAL_PX, MULTIVIEW_MIN_COLUMN_LOGICAL_PX, MULTIVIEW_SNAP_LOGICAL_PX,
    MULTIVIEW_WRAP_MIN_ASPECT,
};

/// A gutter drag in progress: distances are always applied to the arrangement at drag start, never accumulated.
pub(super) struct DragState {
    gutter: Gutter,
    start_layout: Layout,
    start_pins: Pins,
}

/// Layout parameters of the Multiview in the pixels of `container`.
pub(super) fn layout_params(container: &Container, gutter_floor: i32) -> LayoutParams {
    container.layout_params(
        MULTIVIEW_GUTTER_LOGICAL_PX,
        MULTIVIEW_MIN_COLUMN_LOGICAL_PX,
        MULTIVIEW_WRAP_MIN_ASPECT,
        gutter_floor,
    )
}

impl MultiviewInstance {
    /// The aspect ratios the layout is solved with. A lone video fills the whole display (mpv letterboxes inside its
    /// window, so wheel, keys and on-screen controller work everywhere); it is laid out with the aspect of the container,
    /// which makes its window the whole container.
    fn aspects(&self) -> Vec<f32> {
        if let [only] = self.tiles.as_slice()
            && only.kind == TileKind::Video
            && only.error.is_none()
        {
            let height = self.container.height.max(1);
            return vec![self.container.width.max(1) as f32 / height as f32];
        }
        self.tiles.iter().map(|t| t.aspect).collect()
    }

    pub(super) fn pins(&self) -> Pins {
        Pins {
            cols: self.tiles.iter().map(|t| t.pin).collect(),
            rows: self.row_pins,
        }
    }

    fn store_pins(&mut self, pins: &Pins) {
        for (index, tile) in self.tiles.iter_mut().enumerate() {
            tile.pin = pins.cols.get(index).copied().flatten();
        }
        self.row_pins = pins.rows;
    }

    fn solve_layout(&mut self) {
        let solved = layout::solve(&self.params, &self.aspects(), &self.pins());
        if solved.rows.len() != 2 {
            self.row_pins = None;
        }
        self.layout = solved;
    }

    /// Solves the layout again (after tiles, aspects, pins or container changed) and places the windows.
    pub(super) fn apply_layout(&mut self) {
        self.solve_layout();
        self.reproject();
    }

    pub fn drag_started(&mut self, index: usize) {
        let Some(gutter) = self.layout.gutters.get(index).copied() else {
            return;
        };
        self.drag = Some(DragState {
            gutter,
            start_layout: self.layout.clone(),
            start_pins: self.pins(),
        });
    }

    /// `distance` is the pointer travel since the drag started, in logical pixels of the host.
    pub fn dragged(&mut self, distance: f32) {
        let aspects = self.aspects();
        let Some(drag) = self.drag.as_ref() else {
            return;
        };
        let projection = self.host.projection;
        let dpi = self.host.surface.dpi;
        let delta = projection.length_to_container(distance * dpi).round() as i32;
        let snap = projection
            .length_to_container(MULTIVIEW_SNAP_LOGICAL_PX * dpi)
            .round() as i32;
        let next = layout_pins::drag_pins(
            &drag.start_pins,
            &drag.start_layout,
            &drag.gutter,
            delta,
            &self.params,
            &aspects,
            snap,
        );
        self.store_pins(&next);
        self.apply_layout();
    }

    pub fn drag_finished(&mut self) {
        self.drag = None;
    }

    /// Back to the automatic arrangement: every pinned width and row height is forgotten.
    pub fn reset_split(&mut self) {
        self.drag = None;
        self.store_pins(&Pins::default());
        self.apply_layout();
        tracing::debug!("[Multiview: Layout] Split reset to automatic");
    }

    /// Exchanges the two tiles next to a vertical gutter, or the two tiles of one column of a 2x2 grid (the swap button
    /// of that column). Sound states and pinned widths travel with the tiles.
    pub fn swap(&mut self, index: usize) {
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
            }
            GutterKind::ColumnSwap => {
                let [top, bottom] = self.layout.rows.as_slice() else {
                    return;
                };
                let (Some(&a), Some(&b)) = (top.get(gutter.left), bottom.get(gutter.left)) else {
                    return;
                };
                self.tiles.swap(a, b);
            }
            GutterKind::Horizontal => return,
        }
        self.apply_layout();
        tracing::debug!(gutter = index, "[Multiview: Layout] Tiles swapped");
    }
}
