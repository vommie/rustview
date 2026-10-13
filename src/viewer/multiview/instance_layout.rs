//! Instance behavior around the arrangement: solving the layout in container pixels, gutter drag, swap and reset.

use super::container::Container;
use super::instance::MultiviewInstance;
use super::layout::{self, Gutter, GutterKind, Layout, LayoutParams, ManualSplit, Split};
use crate::settings::schema::{
    MULTIVIEW_GUTTER_LOGICAL_PX, MULTIVIEW_MIN_COLUMN_LOGICAL_PX, MULTIVIEW_SNAP_LOGICAL_PX,
    MULTIVIEW_WRAP_MIN_ASPECT,
};

/// A gutter drag in progress: distances are always applied to the proportions at drag start, never accumulated.
pub(super) struct DragState {
    gutter: Gutter,
    start: ManualSplit,
    start_layout: Layout,
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
    fn aspects(&self) -> Vec<f32> {
        self.tiles.iter().map(|t| t.aspect).collect()
    }

    fn solve_layout(&mut self) {
        let aspects = self.aspects();
        let mut solved = layout::solve(&self.params, &aspects, &self.split);
        if matches!(self.split, Split::Manual(_)) && !layout::manual_fits(&self.split, &solved.rows)
        {
            self.split = Split::Auto;
            solved = layout::solve(&self.params, &aspects, &self.split);
        }
        self.layout = solved;
    }

    /// Solves the layout again (after tiles, aspects, split or container changed) and places the windows.
    pub(super) fn apply_layout(&mut self) {
        self.solve_layout();
        self.reproject();
    }

    pub fn drag_started(&mut self, index: usize) {
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

    pub fn drag_finished(&mut self) {
        self.drag = None;
    }

    pub fn reset_split(&mut self) {
        self.drag = None;
        self.split = Split::Auto;
        self.apply_layout();
        tracing::debug!("[Multiview: Layout] Split reset to automatic");
    }

    /// Exchanges the two tiles next to a vertical gutter, or the two rows of a 2x2 grid. Sound states travel with the tiles.
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::instance::SharedInstance;
    use crate::viewer::multiview::instance_host::HostSurface;
    use crate::viewer::multiview::model::TileKind;
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

    fn seeds(aspects: &[f32]) -> Vec<TileSeed> {
        aspects
            .iter()
            .enumerate()
            .map(|(i, &aspect)| TileSeed {
                media_id: i as u64 + 1,
                kind: TileKind::Image,
                aspect,
                path: PathBuf::new(),
                filename: format!("{i}.png"),
            })
            .collect()
    }

    fn fullscreen(aspects: &[f32], width: i32, height: i32, dpi: f32) -> SharedInstance {
        let instance = MultiviewInstance::detached(seeds(aspects));
        assert!(
            instance
                .lock()
                .set_host(&HostSurface::fullscreen(0, width, height, dpi))
        );
        instance
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
                let expected = layout::solve(&params(width, height, dpi), aspects, &Split::Auto);
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
    fn a_drag_follows_the_legacy_math() {
        for &dpi in &[1.0f32, 1.25, 2.0] {
            let (width, height) = (1920, 1080);
            let aspects = [1.78, 0.56, 1.0];
            let instance = fullscreen(&aspects, width, height, dpi);
            let p = params(width, height, dpi);
            let start_layout = layout::solve(&p, &aspects, &Split::Auto);
            let gutter = start_layout.gutters[0];

            let mut guard = instance.lock();
            guard.drag_started(0);
            guard.dragged(37.0);
            guard.dragged(-52.5);

            let delta = (-52.5f32 * dpi).round() as i32;
            let snap = (MULTIVIEW_SNAP_LOGICAL_PX * dpi).round() as i32;
            let next = layout::drag(
                &start_layout.to_manual(),
                &start_layout,
                &gutter,
                delta,
                &p,
                &aspects,
                snap,
            );
            let expected = layout::solve(&p, &aspects, &Split::Manual(next));
            assert_eq!(guard.layout, expected, "dpi {dpi}");
        }
    }

    #[test]
    fn a_swap_exchanges_the_tiles_and_their_aspects() {
        let instance = fullscreen(&[1.78, 0.56, 1.0], 1920, 1080, 1.0);
        let mut guard = instance.lock();
        guard.swap(0);
        assert_eq!(guard.member_ids(), vec![2, 1, 3]);
        let expected = layout::solve(&params(1920, 1080, 1.0), &[0.56, 1.78, 1.0], &Split::Auto);
        assert_eq!(guard.layout, expected);
    }

    #[test]
    fn a_manual_split_survives_a_swap() {
        let instance = fullscreen(&[1.78, 1.78], 1920, 1080, 1.0);
        let mut guard = instance.lock();
        guard.drag_started(0);
        guard.dragged(200.0);
        let Split::Manual(before) = guard.split.clone() else {
            panic!("the drag must produce a manual split");
        };
        guard.swap(0);
        let Split::Manual(after) = guard.split.clone() else {
            panic!("the split stays manual");
        };
        assert_eq!(
            after.col_fr[0],
            vec![before.col_fr[0][1], before.col_fr[0][0]]
        );
        guard.reset_split();
        assert_eq!(guard.split, Split::Auto);
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
