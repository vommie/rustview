//! The tab-grid host of Multiview tabs: shows the instance of a tab as a miniature in its grid item.
//!
//! One `GridHost` is created per sync of the grid. It gives the instance the media canvas of its item (clipped to the visible
//! part of the grid, so scrolled-out windows disappear), pauses the players (the grid only shows the arrangement) and returns
//! what the Slint item draws. The instance stays the only copy of the Multiview.

use super::views::{self, ImageCache};
use crate::ui::tabs::grid::canvas::{LogicalRect, viewport_bounds};
use crate::ui::{MainWindow, MultiviewGutterView, MultiviewScreenView, MultiviewTileView};
use crate::viewer::multiview::HostSurface;
use crate::viewer::multiview::instance::SharedInstance;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::{ViewerTabManager, ViewportBounds, XRectangle, get_slint_window_xid};
use slint::ComponentHandle;
use std::cell::RefCell;

/// What a grid item draws for its Multiview (coordinates relative to the media canvas of the item).
pub struct GridItemView {
    pub tiles: Vec<MultiviewTileView>,
    pub gutters: Vec<MultiviewGutterView>,
    pub screen: MultiviewScreenView,
}

pub struct GridHost {
    main_xid: u64,
    scale: f32,
    /// The visible part of the grid in physical window pixels.
    pub viewport: ViewportBounds,
    cutouts: Vec<XRectangle>,
    images: RefCell<ImageCache>,
}

impl GridHost {
    /// `None` while the main window has no native handle yet.
    pub fn new(window: &MainWindow, tab_manager: &ViewerTabManager) -> Option<Self> {
        let main_xid = get_slint_window_xid(window.window())?;
        let scale = window.window().scale_factor();
        let size = window.window().size();
        Some(Self {
            main_xid,
            scale,
            viewport: viewport_bounds(size.width, size.height, scale),
            cutouts: tab_manager.get_combined_cutouts(),
            images: RefCell::new(ImageCache::default()),
        })
    }

    pub fn main_xid(&self) -> u64 {
        self.main_xid
    }

    /// Places the Multiview on `canvas` (logical window pixels) and returns what the item has to draw.
    pub fn place(&self, instance: &SharedInstance, canvas: LogicalRect) -> GridItemView {
        let rect =
            views::physical_rect(canvas.x, canvas.y, canvas.width, canvas.height, self.scale);
        let clip = Rect::new(
            self.viewport.x,
            self.viewport.y,
            self.viewport.width as i32,
            self.viewport.height as i32,
        );
        let surface =
            HostSurface::tab_grid(self.main_xid, rect, self.scale, clip, self.cutouts.clone());
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
