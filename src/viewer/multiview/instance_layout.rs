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

    /// Exchanges the two tiles next to a vertical gutter, or the two rows of a 2x2 grid. Sound states and pinned widths
    /// travel with the tiles.
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
            GutterKind::Horizontal => {
                if self.tiles.len() != 4 {
                    return;
                }
                self.tiles.swap(0, 2);
                self.tiles.swap(1, 3);
                if let Some(rows) = self.row_pins.as_mut() {
                    rows.swap(0, 1);
                }
            }
        }
        self.apply_layout();
        tracing::debug!(gutter = index, "[Multiview: Layout] Tiles swapped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::instance::SharedInstance;
    use crate::viewer::multiview::instance_host::HostSurface;
    use crate::viewer::multiview::layout::Rect;
    use crate::viewer::multiview::tile::TileSeed;
    use std::path::PathBuf;

    const ASPECT_TABLE: [&[f32]; 7] = [
        &[1.78],
        &[1.78, 1.78],
        &[1.78, 0.56],
        &[0.56, 0.56, 0.56],
        &[1.78, 0.56, 1.0],
        &[1.78, 1.78, 1.78, 1.78],
        &[1.78, 1.78, 1.78, 0.56],
    ];

    fn seeds_of(kind: TileKind, aspects: &[f32]) -> Vec<TileSeed> {
        aspects
            .iter()
            .enumerate()
            .map(|(i, &aspect)| TileSeed {
                media_id: i as u64 + 1,
                kind,
                aspect,
                path: PathBuf::new(),
                filename: format!("{i}.png"),
            })
            .collect()
    }

    fn fullscreen_of(
        kind: TileKind,
        aspects: &[f32],
        width: i32,
        height: i32,
        dpi: f32,
    ) -> SharedInstance {
        let instance = MultiviewInstance::detached(seeds_of(kind, aspects));
        assert!(
            instance
                .lock()
                .set_host(&HostSurface::fullscreen(0, width, height, dpi))
        );
        instance
    }

    fn fullscreen(aspects: &[f32], width: i32, height: i32, dpi: f32) -> SharedInstance {
        fullscreen_of(TileKind::Image, aspects, width, height, dpi)
    }

    fn params(width: i32, height: i32, dpi: f32) -> LayoutParams {
        LayoutParams {
            width,
            height,
            gutter: (16.0 * dpi).round() as i32,
            min_column: (160.0 * dpi).round() as i32,
            min_wide_aspect: 1.2,
        }
    }

    #[test]
    fn the_instance_layout_equals_the_solver_for_the_aspect_table() {
        for &(width, height, dpi) in &[(1920, 1080, 1.0), (2560, 1440, 1.25), (3840, 2160, 2.0)] {
            for aspects in ASPECT_TABLE {
                let instance = fullscreen(aspects, width, height, dpi);
                let expected =
                    layout::solve(&params(width, height, dpi), aspects, &Pins::default());
                let guard = instance.lock();
                assert_eq!(
                    guard.layout, expected,
                    "{aspects:?} on {width}x{height}@{dpi}"
                );
                let view = guard.view();
                assert_eq!(view.screen.w, width);
                for (tile, placed) in view.tiles.iter().zip(&expected.tiles) {
                    assert_eq!(tile.rect, placed.content, "fullscreen is the identity");
                }
            }
        }
    }

    #[test]
    fn a_drag_pins_the_two_neighbors_and_leaves_the_rest_automatic() {
        for &dpi in &[1.0f32, 1.25, 2.0] {
            let (width, height) = (1920, 1080);
            let aspects = [1.78, 0.56, 1.0];
            let instance = fullscreen(&aspects, width, height, dpi);
            let p = params(width, height, dpi);
            let start_layout = layout::solve(&p, &aspects, &Pins::default());
            let gutter = start_layout.gutters[0];

            let mut guard = instance.lock();
            guard.drag_started(0);
            guard.dragged(37.0);
            guard.dragged(-52.5);

            let delta = (-52.5f32 * dpi).round() as i32;
            let snap = (MULTIVIEW_SNAP_LOGICAL_PX * dpi).round() as i32;
            let next = layout_pins::drag_pins(
                &Pins::default(),
                &start_layout,
                &gutter,
                delta,
                &p,
                &aspects,
                snap,
            );
            assert_eq!(guard.layout, layout::solve(&p, &aspects, &next), "dpi {dpi}");
            assert!(guard.tiles[0].pin.is_some() && guard.tiles[1].pin.is_some());
            assert_eq!(guard.tiles[2].pin, None, "the third tile stays automatic");
        }
    }

    #[test]
    fn a_swap_exchanges_the_tiles_and_their_aspects() {
        let instance = fullscreen(&[1.78, 0.56, 1.0], 1920, 1080, 1.0);
        let mut guard = instance.lock();
        guard.swap(0);
        assert_eq!(guard.member_ids(), vec![2, 1, 3]);
        let expected = layout::solve(
            &params(1920, 1080, 1.0),
            &[0.56, 1.78, 1.0],
            &Pins::default(),
        );
        assert_eq!(guard.layout, expected);
    }

    #[test]
    fn pinned_widths_travel_with_swapped_tiles_and_reset_forgets_them() {
        let instance = fullscreen(&[1.78, 1.78], 1920, 1080, 1.0);
        let mut guard = instance.lock();
        guard.drag_started(0);
        guard.dragged(200.0);
        let (left, right) = (guard.tiles[0].pin, guard.tiles[1].pin);
        assert!(left.is_some() && right.is_some() && left > right);
        guard.swap(0);
        assert_eq!(guard.member_ids(), vec![2, 1]);
        assert_eq!(
            (guard.tiles[0].pin, guard.tiles[1].pin),
            (right, left),
            "each tile keeps the width the user gave it"
        );
        guard.reset_split();
        assert!(guard.pins().is_auto());
    }

    #[test]
    fn a_lone_video_fills_the_whole_display_but_a_lone_image_is_fitted() {
        let video = fullscreen_of(TileKind::Video, &[0.75], 1920, 1080, 1.0);
        assert_eq!(
            video.lock().layout.tiles[0].content,
            Rect::new(0, 0, 1920, 1080),
            "the mpv window is the whole display"
        );
        let image = fullscreen_of(TileKind::Image, &[0.75], 1920, 1080, 1.0);
        let content = image.lock().layout.tiles[0].content;
        assert_eq!((content.w, content.h), (810, 1080));

        let two = fullscreen_of(TileKind::Video, &[0.75, 0.75], 1920, 1080, 1.0);
        assert_eq!(
            two.lock().layout.tiles[0].content.w,
            810,
            "videos are fitted again as soon as there are two tiles"
        );
    }

    #[test]
    fn a_failed_lone_video_keeps_its_own_aspect() {
        let video = fullscreen_of(TileKind::Video, &[0.75], 1920, 1080, 1.0);
        let mut guard = video.lock();
        guard.tiles[0].error = Some("broken".into());
        guard.apply_layout();
        assert_eq!(guard.layout.tiles[0].content.w, 810);
    }

    #[test]
    fn pinned_row_heights_are_forgotten_when_the_grid_is_gone() {
        let instance = fullscreen(&[1.78; 4], 1920, 1080, 1.0);
        let mut guard = instance.lock();
        let horizontal = guard
            .layout
            .gutters
            .iter()
            .position(|g| g.kind == GutterKind::Horizontal)
            .expect("a 2x2 grid has a horizontal gutter");
        guard.drag_started(horizontal);
        guard.dragged(60.0);
        assert!(guard.row_pins.is_some());
        guard.tiles.pop();
        guard.apply_layout();
        assert_eq!(guard.row_pins, None);
    }

    #[test]
    fn the_same_surface_is_not_applied_twice() {
        let instance = fullscreen(&[1.78, 1.78], 1920, 1080, 1.0);
        let mut guard = instance.lock();
        let revision = guard.revision();
        assert!(!guard.set_host(&HostSurface::fullscreen(0, 1920, 1080, 1.0)));
        assert_eq!(guard.revision(), revision);
        assert!(guard.set_host(&HostSurface::fullscreen(0, 1280, 720, 1.0)));
        assert_ne!(guard.revision(), revision);
        assert_eq!(guard.container().width, 1280);
    }
}
