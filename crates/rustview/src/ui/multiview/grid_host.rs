//! The tab-grid host of Multiview tabs: shows the instance of a tab as a miniature in its grid item.
//!
//! One `GridHost` is created per sync of the grid. It gives the instance the media canvas of its item in grid content coordinates
//! (the videos draw into the main window and Slint clips and scrolls them with the grid, so scrolling never re-places a
//! Multiview), pauses the players (the grid only shows the arrangement) and returns what the Slint item draws. The instance stays
//! the only copy of the Multiview.

use super::views::{self, ImageCache};
use crate::ui::tabs::grid::canvas::LogicalRect;
use crate::ui::{MainWindow, MultiviewGutterView, MultiviewScreenView, MultiviewTileView};
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::video_surface::MAIN_WINDOW;
use slint::ComponentHandle;
use std::cell::RefCell;

/// What a grid item draws for its Multiview (coordinates relative to the media canvas of the item).
pub struct GridItemView {
    pub tiles: Vec<MultiviewTileView>,
    pub gutters: Vec<MultiviewGutterView>,
    pub screen: MultiviewScreenView,
}

pub struct GridHost {
    scale: f32,
    images: RefCell<ImageCache>,
}

impl GridHost {
    pub fn new(window: &MainWindow) -> Self {
        Self {
            scale: window.window().scale_factor(),
            images: RefCell::new(ImageCache::default()),
        }
    }

    /// Places the Multiview on `canvas` (logical pixels of the grid content, i.e. the window at scroll position 0) and returns what
    /// the item has to draw.
    pub fn place(&self, instance: &SharedInstance, canvas: LogicalRect) -> GridItemView {
        let rect = views::physical_rect(canvas.x, canvas.y, canvas.width, canvas.height, self.scale);
        let surface = HostSurface::tab_grid(MAIN_WINDOW, rect, self.scale);
        let view = {
            let mut guard = instance.lock();
            guard.set_host(&surface);
            guard.view()
        };
        GridItemView {
            tiles: views::tile_views(&view, &mut self.images.borrow_mut()),
            gutters: views::gutter_views(&view),
            screen: views::screen_view(&view),
        }
    }
}
