//! The tab-grid host of Multiview tabs: shows the instance of a tab as a miniature in its grid item.
//!
//! One `GridHost` is created per sync of the grid. It gives the instance the media canvas of its item (the videos draw into the
//! main window, Slint clips and scrolls them with the grid), pauses the players (the grid only shows the arrangement) and returns
//! what the Slint item draws. The instance stays the only copy of the Multiview.

use super::views::{self, ImageCache};
use crate::ui::tabs::grid::canvas::{LogicalRect, visible_grid};
use crate::ui::{MainWindow, MultiviewGutterView, MultiviewScreenView, MultiviewTileView};
use crate::viewer::get_slint_window_xid;
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::layout::Rect;
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
    /// The X11 window of the main window (0 on Wayland).
    main_xid: u64,
    scale: f32,
    /// The visible part of the grid in physical window pixels.
    visible: Rect,
    images: RefCell<ImageCache>,
}

impl GridHost {
    pub fn new(window: &MainWindow) -> Self {
        let scale = window.window().scale_factor();
        let size = window.window().size();
        Self {
            main_xid: get_slint_window_xid(window.window()).unwrap_or(0),
            scale,
            visible: visible_grid(size.width, size.height, scale),
            images: RefCell::new(ImageCache::default()),
        }
    }

    /// Places the Multiview on `canvas` (logical window pixels) and returns what the item has to draw.
    pub fn place(&self, instance: &SharedInstance, canvas: LogicalRect) -> GridItemView {
        let rect = views::physical_rect(canvas.x, canvas.y, canvas.width, canvas.height, self.scale);
        let surface = HostSurface::tab_grid(MAIN_WINDOW, self.main_xid, rect, self.scale, self.visible);
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
