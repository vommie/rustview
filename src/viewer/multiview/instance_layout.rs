//! Instance behavior around the arrangement: solving the layout in container pixels, gutter drag, swap and reset.
//!
//! The widths the user sets by dragging a gutter are pins on the two neighboring tiles (`TileState::pin`), so they stay
//! with the tiles when media join or leave; everything the user never touched adapts automatically.

use super::container::{Container, lone_medium_fills_host};
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
    ///
    /// In the tab view every lone medium (also an image) takes the whole tab area: its tile is laid out with the aspect of the
    /// solving container, which is the shape of that area (`lone_medium_fills_host`), and an image is fitted inside the tile.
    fn aspects(&self) -> Vec<f32> {
        if let [only] = self.tiles.as_slice()
            && ((only.kind == TileKind::Video && only.error.is_none()) || lone_medium_fills_host(self.host.surface.mode, 1))
        {
            let container = self.solving_container();
            let height = container.height.max(1);
            return vec![container.width.max(1) as f32 / height as f32];
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
        self.refresh_geometry();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::events::{MultiviewEventSink, PlayerEvent};
    use crate::viewer::multiview::follow::Wanted;
    use crate::viewer::multiview::instance::{SharedInstance, WeakInstance};
    use crate::viewer::multiview::instance_host::HostSurface;
    use crate::viewer::multiview::layout::Rect;
    use crate::viewer::multiview::tile::TileSeed;
    use slint::{Rgb8Pixel, SharedPixelBuffer};
    use std::path::PathBuf;
    use std::sync::Arc;

    struct NullSink;

    impl MultiviewEventSink for NullSink {
        fn post(&self, _instance: WeakInstance, _event: PlayerEvent) {}
    }

    fn seed(id: u64, kind: TileKind, aspect: f32) -> TileSeed {
        let mut seed = TileSeed::new(id, kind, aspect, PathBuf::from(format!("/x/{id}")), format!("{id}.jpg"));
        if kind == TileKind::Image {
            seed.pixels = Some(SharedPixelBuffer::<Rgb8Pixel>::new(4, 3));
        }
        seed
    }

    fn follow(instance: &SharedInstance, ids: &[u64]) {
        let mut mv = instance.lock();
        let wanted: Vec<Wanted> = ids.iter().map(|&id| Wanted { id, is_video: false }).collect();
        let plan = mv.plan_follow(&wanted);
        let seeds = plan.add.iter().map(|a| Some(seed(*a, TileKind::Image, 1.5))).collect();
        mv.apply_follow(&plan, seeds);
    }

    /// The tab area of the main window (4:3) next to a 16:9 display of the Multiview.
    const TAB_AREA: Rect = Rect::new(240, 80, 1200, 900);

    fn in_tab_view(seeds: Vec<TileSeed>) -> SharedInstance {
        let instance = MultiviewInstance::create(Arc::new(NullSink), seeds);
        instance.lock().set_host(&HostSurface::tab_view(1, TAB_AREA, 1.0, Vec::new()));
        instance
    }

    #[test]
    fn a_lone_image_and_a_lone_video_cover_the_whole_tab_area() {
        for kind in [TileKind::Image, TileKind::Video] {
            let instance = in_tab_view(vec![seed(1, kind, 16.0 / 9.0)]);
            let view = instance.lock().view();
            let all = Rect::new(0, 0, TAB_AREA.w, TAB_AREA.h);
            assert_eq!(view.screen, all, "{kind:?}: no bars around the display");
            assert_eq!(view.tiles[0].rect, all, "{kind:?}: the tile is the whole area");
            assert!(view.gutters.is_empty());
        }
    }

    #[test]
    fn several_media_keep_the_miniature_of_the_display() {
        let instance = in_tab_view(vec![seed(1, TileKind::Image, 1.5), seed(2, TileKind::Image, 1.5)]);
        let view = instance.lock().view();
        // The 16:9 display scaled into the 4:3 area: full width, bars above and below.
        assert_eq!(view.screen.w, TAB_AREA.w);
        assert!(view.screen.h < TAB_AREA.h, "{:?}", view.screen);
        assert_eq!(view.tiles.len(), 2);
    }

    #[test]
    fn the_layout_follows_the_number_of_media_both_ways() {
        let instance = in_tab_view(vec![seed(1, TileKind::Image, 1.5)]);
        let all = Rect::new(0, 0, TAB_AREA.w, TAB_AREA.h);
        assert_eq!(instance.lock().view().screen, all);

        // A second medium joins: the arrangement of the display's shape takes over.
        follow(&instance, &[1, 2]);
        let two = instance.lock().view();
        assert_eq!(two.tiles.len(), 2);
        assert!(two.screen.h < TAB_AREA.h, "{:?}", two.screen);

        // It leaves again: the remaining medium fills the area at once.
        follow(&instance, &[1]);
        let one = instance.lock().view();
        assert_eq!(one.tiles.len(), 1);
        assert_eq!(one.screen, all);
        assert_eq!(one.tiles[0].rect, all);
    }

    #[test]
    fn the_area_changing_while_a_medium_is_alone_is_followed() {
        let instance = in_tab_view(vec![seed(1, TileKind::Image, 1.5)]);
        let wider = Rect::new(100, 60, 1500, 700);
        instance.lock().set_host(&HostSurface::tab_view(1, wider, 1.0, Vec::new()));
        let view = instance.lock().view();
        assert_eq!(view.screen, Rect::new(0, 0, 1500, 700));
        assert_eq!(view.tiles[0].rect, Rect::new(0, 0, 1500, 700));
    }

    #[test]
    fn the_fullscreen_and_the_tab_grid_never_use_the_special_case() {
        let instance = MultiviewInstance::create(Arc::new(NullSink), vec![seed(1, TileKind::Image, 1.5)]);
        instance.lock().set_host(&HostSurface::fullscreen(1, 2560, 1440, 1.0));
        let full = instance.lock().view();
        // The display itself: the image is fitted into it by its own aspect.
        assert_eq!(full.screen, Rect::new(0, 0, 2560, 1440));
        assert!(full.tiles[0].rect.w < 2560, "{:?}", full.tiles[0].rect);
        let grid = HostSurface::tab_grid(1, Rect::new(0, 0, 400, 400), 1.0, Rect::new(0, 0, 400, 400), Vec::new());
        instance.lock().set_host(&grid);
        let miniature = instance.lock().view();
        assert_eq!(miniature.screen.w, 400);
        assert!(miniature.screen.h < 400, "{:?}", miniature.screen);
    }
}
