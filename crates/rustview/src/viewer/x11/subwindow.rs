//! The native video window: a container window with a video window inside it, both children of the Slint window, so mpv can draw
//! into the inner window while the container clips it to what is visible and cuts holes for menus.

use super::session::{Session, x11_error};
use crate::viewer::ViewerError;
use std::time::Instant;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, CreateGCAux, CreateWindowAux,
    Cursor, Rectangle, Window, WindowClass,
};

/// A rectangle in window coordinates (`x`, `y` and size), as the X protocol has it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XRectangle {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

impl From<XRectangle> for Rectangle {
    fn from(rect: XRectangle) -> Self {
        Rectangle {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

/// The rectangles in the form the X protocol requests take.
pub(super) fn protocol_rectangles(rects: &[XRectangle]) -> Vec<Rectangle> {
    rects.iter().copied().map(Rectangle::from).collect()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SubwindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ViewportBounds {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// The smallest window the X server accepts without trouble for a video.
const MIN_SIZE: u32 = 16;
/// The largest rectangle of the shape extension.
const MAX_SHAPE_EXTENT: u32 = 32767;
/// Upper bound of the windows below the video window that get the invisible cursor (mpv creates only a handful).
const MAX_DESCENDANTS: usize = 64;

pub(super) fn coord(value: i32) -> i16 {
    i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX })
}

pub(super) fn extent(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

/// Managed X11 Video Subwindow container embedded directly into Slint's central media viewport.
pub struct X11VideoSubwindow {
    session: Session,
    container: Window,
    subwindow: Window,
    parent: Window,
    shape_available: bool,
    current_container_x: i32,
    current_container_y: i32,
    current_container_w: u32,
    current_container_h: u32,
    current_sub_x: i32,
    current_sub_y: i32,
    current_sub_w: u32,
    current_sub_h: u32,
    is_visible: bool,
    is_clipped: bool,
    last_applied_local_rects: Vec<XRectangle>,
    /// An invisible cursor, created when the cursor is hidden for the first time.
    blank_cursor: Option<Cursor>,
}

impl X11VideoSubwindow {
    pub fn new(parent_xid: u64, x: i32, y: i32, width: u32, height: u32) -> Result<Self, ViewerError> {
        let t0 = Instant::now();
        let parent =
            u32::try_from(parent_xid).map_err(|_| ViewerError::X11(format!("Invalid parent window #{parent_xid}")))?;
        let session = Session::open("a video window")?;

        let w_clamped = width.max(MIN_SIZE);
        let h_clamped = height.max(MIN_SIZE);
        let container = session.conn.generate_id().map_err(x11_error)?;
        let subwindow = session.conn.generate_id().map_err(x11_error)?;

        // Black windows without a border, depth, class and visual of the parent (like `XCreateSimpleWindow`).
        let attributes = CreateWindowAux::new().background_pixel(0).border_pixel(0);
        let create = |id: Window, window_parent: Window, wx: i32, wy: i32| {
            session.conn.create_window(
                x11rb::COPY_FROM_PARENT as u8,
                id,
                window_parent,
                coord(wx),
                coord(wy),
                extent(w_clamped),
                extent(h_clamped),
                0,
                WindowClass::COPY_FROM_PARENT,
                x11rb::COPY_FROM_PARENT,
                &attributes,
            )
        };
        create(container, parent, x, y)
            .map_err(x11_error)?
            .check()
            .map_err(|e| ViewerError::X11(format!("Failed to create X11 container subwindow: {e}")))?;
        create(subwindow, container, 0, 0)
            .map_err(x11_error)?
            .check()
            .map_err(|e| ViewerError::X11(format!("Failed to create X11 video subwindow: {e}")))?;

        let _ = session.conn.map_window(subwindow);
        session.sync();
        let shape_available = session
            .conn
            .extension_information(shape::X11_EXTENSION_NAME)
            .ok()
            .flatten()
            .is_some();

        tracing::info!(
            parent_xid,
            container_xid = container,
            subwindow_xid = subwindow,
            x,
            y,
            width,
            height,
            elapsed_us = t0.elapsed().as_micros(),
            "[X11VideoSubwindow: Create] Created container + video subwindow pair for unscaled clipping"
        );

        Ok(Self {
            session,
            container,
            subwindow,
            parent,
            shape_available,
            current_container_x: x,
            current_container_y: y,
            current_container_w: w_clamped,
            current_container_h: h_clamped,
            current_sub_x: 0,
            current_sub_y: 0,
            current_sub_w: w_clamped,
            current_sub_h: h_clamped,
            is_visible: false,
            is_clipped: false,
            last_applied_local_rects: Vec::new(),
            blank_cursor: None,
        })
    }

    #[inline]
    pub fn xid(&self) -> u64 {
        u64::from(self.subwindow)
    }

    #[inline]
    pub fn parent_xid(&self) -> u64 {
        u64::from(self.parent)
    }

    fn place(&self, window: Window, x: i32, y: i32, width: u32, height: u32) {
        let _ = self
            .session
            .conn
            .configure_window(window, &ConfigureWindowAux::new().x(x).y(y).width(width).height(height));
    }

    fn place_container(&mut self, x: i32, y: i32, width: u32, height: u32) {
        if self.current_container_x != x
            || self.current_container_y != y
            || self.current_container_w != width
            || self.current_container_h != height
        {
            self.current_container_x = x;
            self.current_container_y = y;
            self.current_container_w = width;
            self.current_container_h = height;
            self.place(self.container, x, y, width, height);
        }
    }

    fn place_video(&mut self, x: i32, y: i32, width: u32, height: u32) {
        if self.current_sub_x != x
            || self.current_sub_y != y
            || self.current_sub_w != width
            || self.current_sub_h != height
        {
            self.current_sub_x = x;
            self.current_sub_y = y;
            self.current_sub_w = width;
            self.current_sub_h = height;
            self.place(self.subwindow, x, y, width, height);
        }
    }

    pub fn move_resize(&mut self, x: i32, y: i32, width: u32, height: u32) {
        let w_clamped = width.max(MIN_SIZE);
        let h_clamped = height.max(MIN_SIZE);
        self.place_container(x, y, w_clamped, h_clamped);
        self.place_video(0, 0, w_clamped, h_clamped);
        self.session.flush();
    }

    pub fn update_geometry_and_clip(&mut self, geom: SubwindowGeometry, viewport: ViewportBounds) -> bool {
        let w_clamped = i32::try_from(geom.width.max(MIN_SIZE)).unwrap_or(i32::MAX);
        let h_clamped = i32::try_from(geom.height.max(MIN_SIZE)).unwrap_or(i32::MAX);
        let view_w = i32::try_from(viewport.width.max(MIN_SIZE)).unwrap_or(i32::MAX);
        let view_h = i32::try_from(viewport.height.max(MIN_SIZE)).unwrap_or(i32::MAX);

        let vis_x1 = geom.x.max(viewport.x);
        let vis_y1 = geom.y.max(viewport.y);
        let vis_x2 = (geom.x + w_clamped).min(viewport.x + view_w);
        let vis_y2 = (geom.y + h_clamped).min(viewport.y + view_h);

        let (Ok(vis_w), Ok(vis_h)) = (u32::try_from(vis_x2 - vis_x1), u32::try_from(vis_y2 - vis_y1)) else {
            self.hide();
            return false;
        };
        if vis_w == 0 || vis_h == 0 {
            self.hide();
            return false;
        }

        let sub_offset_x = geom.x - vis_x1;
        let sub_offset_y = geom.y - vis_y1;

        self.place_container(vis_x1, vis_y1, vis_w, vis_h);
        self.place_video(
            sub_offset_x,
            sub_offset_y,
            w_clamped.unsigned_abs(),
            h_clamped.unsigned_abs(),
        );
        self.session.flush();

        self.show();
        true
    }

    pub fn reparent(
        &mut self,
        new_parent_xid: u64,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), ViewerError> {
        let t0 = Instant::now();
        let target_parent = u32::try_from(new_parent_xid)
            .map_err(|_| ViewerError::X11(format!("Invalid target parent #{new_parent_xid}")))?;
        let w_clamped = width.max(MIN_SIZE);
        let h_clamped = height.max(MIN_SIZE);

        self.session
            .conn
            .reparent_window(self.container, target_parent, coord(x), coord(y))
            .map_err(|e| {
                ViewerError::X11(format!(
                    "ReparentWindow failed for target parent #{new_parent_xid}: {e}"
                ))
            })?;

        // The new parent may have other bounds: force the geometry of both windows.
        self.current_container_x = x;
        self.current_container_y = y;
        self.current_container_w = w_clamped;
        self.current_container_h = h_clamped;
        self.place(self.container, x, y, w_clamped, h_clamped);

        self.current_sub_x = 0;
        self.current_sub_y = 0;
        self.current_sub_w = w_clamped;
        self.current_sub_h = h_clamped;
        self.place(self.subwindow, 0, 0, w_clamped, h_clamped);

        let _ = self.session.conn.map_window(self.subwindow);
        let _ = self.session.conn.map_window(self.container);
        self.session.sync();

        self.parent = target_parent;
        self.is_visible = true;
        self.is_clipped = false;
        self.last_applied_local_rects.clear();

        tracing::info!(
            container_xid = self.container,
            subwindow_xid = self.subwindow,
            new_parent_xid,
            width = w_clamped,
            height = h_clamped,
            elapsed_us = t0.elapsed().as_micros(),
            "[X11VideoSubwindow: Reparent] Subwindow container reparented seamlessly"
        );

        Ok(())
    }

    /// Punches transparent cutout holes out of the X11 container window so that context menus and tooltips
    /// are ALWAYS rendered on top of the MPV player without hardware occlusion.
    pub fn clip_cutout_rectangles(&mut self, main_win_rects: &[XRectangle]) {
        if !self.shape_available || !self.is_visible {
            return;
        }

        let container_w = self.current_container_w;
        let container_h = self.current_container_h;
        if container_w == 0 || container_h == 0 {
            return;
        }
        let (container_w_i32, container_h_i32) = (
            i32::try_from(container_w).unwrap_or(i32::MAX),
            i32::try_from(container_h).unwrap_or(i32::MAX),
        );

        let mut container_cutouts: Vec<XRectangle> = Vec::with_capacity(main_win_rects.len());

        for r in main_win_rects {
            let local_x = i32::from(r.x) - self.current_container_x;
            let local_y = i32::from(r.y) - self.current_container_y;
            let local_right = local_x + i32::from(r.width);
            let local_bottom = local_y + i32::from(r.height);

            if local_right > 0 && local_bottom > 0 && local_x < container_w_i32 && local_y < container_h_i32 {
                let clamped_x = local_x.max(0);
                let clamped_y = local_y.max(0);
                let clamped_w = (local_right.min(container_w_i32) - clamped_x).max(1);
                let clamped_h = (local_bottom.min(container_h_i32) - clamped_y).max(1);

                container_cutouts.push(XRectangle {
                    x: coord(clamped_x),
                    y: coord(clamped_y),
                    width: extent(clamped_w.unsigned_abs()),
                    height: extent(clamped_h.unsigned_abs()),
                });
            }
        }

        if self.last_applied_local_rects == container_cutouts && self.is_clipped == !container_cutouts.is_empty() {
            return;
        }

        if container_cutouts.is_empty() {
            self.reset_shape();
            self.is_clipped = false;
            self.last_applied_local_rects.clear();
        } else {
            let full = XRectangle {
                x: 0,
                y: 0,
                width: extent(container_w.min(MAX_SHAPE_EXTENT)),
                height: extent(container_h.min(MAX_SHAPE_EXTENT)),
            };
            let conn = &self.session.conn;
            let _ = conn.shape_rectangles(
                shape::SO::SET,
                shape::SK::BOUNDING,
                ClipOrdering::UNSORTED,
                self.container,
                0,
                0,
                &[full.into()],
            );
            let _ = conn.shape_rectangles(
                shape::SO::SUBTRACT,
                shape::SK::BOUNDING,
                ClipOrdering::UNSORTED,
                self.container,
                0,
                0,
                &protocol_rectangles(&container_cutouts),
            );
            self.is_clipped = true;
            self.last_applied_local_rects = container_cutouts;
        }

        self.session.flush();
    }

    /// Gives the container its default (rectangular) shape again.
    fn reset_shape(&self) {
        let _ = self
            .session
            .conn
            .shape_mask(shape::SO::SET, shape::SK::BOUNDING, self.container, 0, 0, x11rb::NONE);
    }

    /// Hides or shows the mouse cursor over this video window, whatever mpv does with its own cursor. The invisible cursor is
    /// set on the container, the subwindow and every window below it (the window mpv draws in): mpv's window then shows
    /// the cursor of its parent unless mpv defines its own. Showing removes the cursor from all of them again, so they
    /// inherit the cursor of the host window.
    pub fn set_cursor_hidden(&mut self, hidden: bool) {
        let cursor = if hidden {
            match self.blank_cursor.or_else(|| self.create_blank_cursor()) {
                Some(cursor) => {
                    self.blank_cursor = Some(cursor);
                    cursor
                }
                None => return,
            }
        } else {
            x11rb::NONE
        };
        let mut windows = vec![self.container, self.subwindow];
        windows.extend(self.descendants_of(self.subwindow));
        for window in windows {
            let _ = self
                .session
                .conn
                .change_window_attributes(window, &ChangeWindowAttributesAux::new().cursor(cursor));
        }
        self.session.flush();
    }

    /// A 1x1 cursor whose mask is empty, so nothing of it is drawn.
    fn create_blank_cursor(&self) -> Option<Cursor> {
        let conn = &self.session.conn;
        let pixmap = conn.generate_id().ok()?;
        let gc = conn.generate_id().ok()?;
        let cursor = conn.generate_id().ok()?;
        conn.create_pixmap(1, pixmap, self.container, 1, 1).ok()?;
        // A new pixmap has undefined contents: clear it, so the mask has no set bit.
        conn.create_gc(gc, pixmap, &CreateGCAux::new().foreground(0)).ok()?;
        conn.poly_fill_rectangle(
            pixmap,
            gc,
            &[Rectangle {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            }],
        )
        .ok()?;
        let created = conn
            .create_cursor(cursor, pixmap, pixmap, 0, 0, 0, 0, 0, 0, 0, 0)
            .ok()
            .map(|cookie| cookie.check().is_ok());
        let _ = conn.free_gc(gc);
        let _ = conn.free_pixmap(pixmap);
        created.unwrap_or(false).then_some(cursor)
    }

    /// Every window below `window` (children, grandchildren, ...).
    fn descendants_of(&self, window: Window) -> Vec<Window> {
        let mut found = Vec::new();
        let mut pending = vec![window];
        while let Some(current) = pending.pop() {
            if let Some(reply) = self
                .session
                .conn
                .query_tree(current)
                .ok()
                .and_then(|cookie| cookie.reply().ok())
            {
                found.extend_from_slice(&reply.children);
                pending.extend_from_slice(&reply.children);
            }
            if found.len() > MAX_DESCENDANTS {
                break;
            }
        }
        found
    }

    pub fn reset_clip_shape(&mut self) {
        if self.shape_available {
            self.reset_shape();
            self.session.flush();
        }
        self.is_clipped = false;
        self.last_applied_local_rects.clear();
    }

    pub fn show(&mut self) {
        if !self.is_visible {
            let _ = self.session.conn.map_window(self.subwindow);
            let _ = self.session.conn.map_window(self.container);
            self.session.flush();
            self.is_visible = true;
        }
    }

    pub fn hide(&mut self) {
        if self.is_visible {
            let _ = self.session.conn.unmap_window(self.container);
            self.session.flush();
            self.is_visible = false;
        }
    }

    #[inline]
    pub fn is_visible(&self) -> bool {
        self.is_visible
    }
}

impl Drop for X11VideoSubwindow {
    fn drop(&mut self) {
        let conn = &self.session.conn;
        let _ = conn.destroy_window(self.subwindow);
        let _ = conn.destroy_window(self.container);
        if let Some(cursor) = self.blank_cursor {
            let _ = conn.free_cursor(cursor);
        }
        self.session.flush();
        tracing::debug!(
            container_xid = self.container,
            subwindow_xid = self.subwindow,
            "[X11VideoSubwindow: Drop] Destroyed subwindow pair and closed the X11 connection"
        );
    }
}
