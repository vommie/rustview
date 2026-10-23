//! The drag ghost: a small window that follows the pointer while files are dragged out of `RustView`. It has an alpha channel when a
//! compositing manager runs, ignores the pointer completely (an empty input shape, so it never takes a drop away from the window
//! below it) and is override-redirect, so the window manager neither decorates nor stacks it.

use super::session::{Session, x11_error};
use crate::viewer::ViewerError;
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ClipOrdering, ColormapAlloc, ConfigureWindowAux, ConnectionExt as _, CreateGCAux,
    CreateWindowAux, ImageFormat, PropMode, Screen, StackMode, VisualClass, Window, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

/// What the ghost shows: pre-multiplied BGRA pixels (the byte order of a 32-bit X11 pixel on a little-endian machine), no row padding.
pub struct GhostPixels {
    pub width: u16,
    pub height: u16,
    pub bgra: Vec<u8>,
}

/// The visual with an alpha channel (depth 32, true color), when the screen has one.
fn argb_visual(screen: &Screen) -> Option<u32> {
    screen
        .allowed_depths
        .iter()
        .filter(|depth| depth.depth == 32)
        .flat_map(|depth| &depth.visuals)
        .find(|visual| visual.class == VisualClass::TRUE_COLOR && visual.bits_per_rgb_value == 8)
        .map(|visual| visual.visual_id)
}

pub struct GhostWindow {
    session: Session,
    window: Window,
    /// Distance between the pointer and the top left corner of the ghost.
    offset: (i32, i32),
    last_position: (i32, i32),
}

impl GhostWindow {
    /// Opens the window at the pointer. `render` gets whether a compositing manager runs (only then is the alpha channel blended, so
    /// without one it has to draw an opaque picture) and returns the picture.
    pub fn open(render: impl FnOnce(bool) -> Option<GhostPixels>, offset: (i32, i32)) -> Result<Self, ViewerError> {
        let session = Session::open("the drag ghost")?;
        let compositor = session.compositor_running();
        let pixels =
            render(compositor).ok_or_else(|| ViewerError::X11("the drag ghost has nothing to draw".to_owned()))?;

        let conn = &session.conn;
        let screen = &conn.setup().roots[session.screen];
        let argb = if compositor { argb_visual(screen) } else { None };
        let window = conn.generate_id().map_err(x11_error)?;
        let position = Self::pointer_position(&session).unwrap_or((0, 0));
        let (x, y) = (position.0 + offset.0, position.1 + offset.1);

        let mut attributes = CreateWindowAux::new()
            .override_redirect(1)
            .background_pixel(0)
            .border_pixel(0);
        let (depth, visual) = if let Some(visual) = argb {
            // A window of another depth than its parent needs its own color map.
            let colormap = conn.generate_id().map_err(x11_error)?;
            conn.create_colormap(ColormapAlloc::NONE, colormap, session.root, visual)
                .map_err(x11_error)?;
            attributes = attributes.colormap(colormap);
            (32, visual)
        } else {
            (screen.root_depth, screen.root_visual)
        };
        conn.create_window(
            depth,
            window,
            session.root,
            clamp_i16(x),
            clamp_i16(y),
            pixels.width,
            pixels.height,
            0,
            WindowClass::INPUT_OUTPUT,
            visual,
            &attributes,
        )
        .map_err(x11_error)?;

        // Click-through: no part of the window takes pointer input.
        if conn
            .extension_information(shape::X11_EXTENSION_NAME)
            .ok()
            .flatten()
            .is_some()
        {
            let _ = conn.shape_rectangles(
                shape::SO::SET,
                shape::SK::INPUT,
                ClipOrdering::UNSORTED,
                window,
                0,
                0,
                &[],
            );
        }
        let _ = conn.change_property8(
            PropMode::REPLACE,
            window,
            x11rb::protocol::xproto::AtomEnum::WM_NAME,
            x11rb::protocol::xproto::AtomEnum::STRING,
            b"RustView drag ghost",
        );

        // The picture is the background of the window: the server repaints it whenever the window is exposed, which drawing into the
        // window would not survive (and drawing before the window is mapped has no effect at all).
        let pixmap = conn.generate_id().map_err(x11_error)?;
        conn.create_pixmap(depth, pixmap, window, pixels.width, pixels.height)
            .map_err(x11_error)?;
        let gc = conn.generate_id().map_err(x11_error)?;
        conn.create_gc(gc, pixmap, &CreateGCAux::new()).map_err(x11_error)?;
        conn.put_image(
            ImageFormat::Z_PIXMAP,
            pixmap,
            gc,
            pixels.width,
            pixels.height,
            0,
            0,
            0,
            depth,
            &pixels.bgra,
        )
        .map_err(x11_error)?;
        conn.change_window_attributes(window, &ChangeWindowAttributesAux::new().background_pixmap(pixmap))
            .map_err(x11_error)?;
        let _ = conn.free_gc(gc);
        let _ = conn.free_pixmap(pixmap);
        conn.map_window(window).map_err(x11_error)?;
        conn.configure_window(window, &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE))
            .map_err(x11_error)?;
        session.flush();
        Ok(Self {
            session,
            window,
            offset,
            last_position: (x, y),
        })
    }

    fn pointer_position(session: &Session) -> Option<(i32, i32)> {
        let reply = session.conn.query_pointer(session.root).ok()?.reply().ok()?;
        Some((i32::from(reply.root_x), i32::from(reply.root_y)))
    }

    /// Moves the ghost next to the pointer and keeps it on top. `false` when the X connection is gone.
    pub fn follow_pointer(&mut self) -> bool {
        let Some((px, py)) = Self::pointer_position(&self.session) else {
            return false;
        };
        let position = (px + self.offset.0, py + self.offset.1);
        if position != self.last_position {
            self.last_position = position;
            let aux = ConfigureWindowAux::new()
                .x(i32::from(clamp_i16(position.0)))
                .y(i32::from(clamp_i16(position.1)))
                .stack_mode(StackMode::ABOVE);
            if self.session.conn.configure_window(self.window, &aux).is_err() {
                return false;
            }
            self.session.flush();
        }
        true
    }
}

impl Drop for GhostWindow {
    fn drop(&mut self) {
        let _ = self.session.conn.destroy_window(self.window);
        self.session.sync();
    }
}

fn clamp_i16(value: i32) -> i16 {
    i16::try_from(value).unwrap_or(if value < 0 { i16::MIN } else { i16::MAX })
}
