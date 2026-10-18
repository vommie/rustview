//! What a floating overlay window (the action bar of the Multiview) needs from the X server, on a private connection:
//! whether a compositing manager blends windows, where a window is on the screen, stacking and the input and bounding
//! shape of the overlay. Everything here is a single round trip; nothing touches the drive.

use super::{get_x11_api, DisplayPtr, WindowId, X11Symbols, XRectangle};
use crate::viewer::ViewerError;
use std::ffi::{c_int, c_uint, c_void, CString};
use std::sync::Arc;

/// `ShapeBounding` / `ShapeInput` of the SHAPE extension.
const SHAPE_BOUNDING: c_int = 0;
const SHAPE_INPUT: c_int = 2;
const SHAPE_SET: c_int = 0;
const UNSORTED: c_int = 0;

pub struct OverlayServer {
    api: Arc<X11Symbols>,
    display: DisplayPtr,
    root: WindowId,
    screen: c_int,
}

unsafe impl Send for OverlayServer {}
unsafe impl Sync for OverlayServer {}

impl OverlayServer {
    pub fn new() -> Result<Self, ViewerError> {
        let api = get_x11_api()?;
        unsafe {
            let display = (api.x_open_display)(std::ptr::null());
            if display.is_null() {
                return Err(ViewerError::X11(
                    "Failed to open X11 display for the overlay server".into(),
                ));
            }
            let root = (api.x_default_root_window)(display);
            let screen = (api.x_default_screen)(display);
            Ok(Self {
                api,
                display,
                root,
                screen,
            })
        }
    }

    /// True while a compositing manager owns the `_NET_WM_CM_S<screen>` selection: only then are the alpha channels of
    /// windows blended with what is below them. Without one a window with an alpha channel shows its raw color bits.
    pub fn compositor_running(&self) -> bool {
        let Ok(name) = CString::new(format!("_NET_WM_CM_S{}", self.screen)) else {
            return false;
        };
        unsafe {
            let atom = (self.api.x_intern_atom)(self.display, name.as_ptr(), 1);
            atom != 0 && (self.api.x_get_selection_owner)(self.display, atom) != 0
        }
    }

    /// The color depth of a window (32 = it has an alpha channel).
    pub fn window_depth(&self, window: u64) -> Option<u32> {
        let (mut root, mut x, mut y) = (0, 0, 0);
        let (mut w, mut h, mut border, mut depth): (c_uint, c_uint, c_uint, c_uint) = (0, 0, 0, 0);
        let ok = unsafe {
            (self.api.x_get_geometry)(
                self.display,
                window,
                &mut root,
                &mut x,
                &mut y,
                &mut w,
                &mut h,
                &mut border,
                &mut depth,
            )
        };
        (ok != 0).then_some(depth)
    }

    /// The position of the top-left corner of a window's client area on the screen (root window coordinates).
    pub fn origin_in_root(&self, window: u64) -> Option<(i32, i32)> {
        let (mut x, mut y): (c_int, c_int) = (0, 0);
        let mut child: WindowId = 0;
        let ok = unsafe {
            (self.api.x_translate_coordinates)(
                self.display,
                window,
                self.root,
                0,
                0,
                &mut x,
                &mut y,
                &mut child,
            )
        };
        (ok != 0).then_some((x, y))
    }

    /// The pointer in screen coordinates and whether a mouse button is held down (one round trip, `XQueryPointer`).
    pub fn pointer_and_buttons(&self) -> Option<((i32, i32), bool)> {
        const BUTTON_MASK: c_uint = (1 << 8) | (1 << 9) | (1 << 10);
        let (mut root_ret, mut child_ret): (WindowId, WindowId) = (0, 0);
        let (mut root_x, mut root_y, mut win_x, mut win_y): (c_int, c_int, c_int, c_int) = (0, 0, 0, 0);
        let mut mask: c_uint = 0;
        let on_screen = unsafe {
            (self.api.x_query_pointer)(
                self.display,
                self.root,
                &mut root_ret,
                &mut child_ret,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            )
        };
        (on_screen != 0).then_some(((root_x, root_y), mask & BUTTON_MASK != 0))
    }

    /// The top level window (a child of the root window) that is in front at the pointer, or `None` if the pointer is on
    /// another screen. A window of another application that covers the pointer position is the answer then, not the
    /// window below it.
    pub fn top_level_at_pointer(&self) -> Option<u64> {
        let (mut root_ret, mut child_ret): (WindowId, WindowId) = (0, 0);
        let (mut root_x, mut root_y, mut win_x, mut win_y): (c_int, c_int, c_int, c_int) = (0, 0, 0, 0);
        let mut mask: c_uint = 0;
        let on_screen = unsafe {
            (self.api.x_query_pointer)(
                self.display,
                self.root,
                &mut root_ret,
                &mut child_ret,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            )
        };
        (on_screen != 0 && child_ret != 0).then_some(child_ret)
    }

    /// The top level window that contains `window`: the window itself, or its frame if the window manager reparented it.
    pub fn top_level_of(&self, window: u64) -> Option<u64> {
        let mut current = window;
        for _ in 0..8 {
            let (mut root_ret, mut parent_ret): (WindowId, WindowId) = (0, 0);
            let mut children: *mut WindowId = std::ptr::null_mut();
            let mut count: c_uint = 0;
            let ok = unsafe {
                (self.api.x_query_tree)(
                    self.display,
                    current,
                    &mut root_ret,
                    &mut parent_ret,
                    &mut children,
                    &mut count,
                )
            };
            if !children.is_null() {
                unsafe { (self.api.x_free)(children as *mut c_void) };
            }
            if ok == 0 {
                return None;
            }
            if parent_ret == 0 || parent_ret == self.root {
                return Some(current);
            }
            current = parent_ret;
        }
        None
    }

    /// Puts the window on top of the stack of its siblings (the screen's top level windows).
    pub fn raise(&self, window: u64) {
        unsafe {
            (self.api.x_raise_window)(self.display, window);
            (self.api.x_flush)(self.display);
        }
    }

    /// The part of the window that receives pointer events; everything else passes them to the windows below.
    pub fn set_input_region(&self, window: u64, rects: &[XRectangle]) {
        self.combine(window, SHAPE_INPUT, Some(rects));
    }

    /// The visible shape of the window, or the full rectangle again for `None`.
    pub fn set_bounding_region(&self, window: u64, rects: Option<&[XRectangle]>) {
        self.combine(window, SHAPE_BOUNDING, rects);
    }

    fn combine(&self, window: u64, kind: c_int, rects: Option<&[XRectangle]>) {
        unsafe {
            match rects {
                Some(rects) => {
                    let Some(shape_rects) = self.api.x_shape_combine_rectangles else {
                        return;
                    };
                    shape_rects(
                        self.display,
                        window,
                        kind,
                        0,
                        0,
                        rects.as_ptr(),
                        rects.len() as c_int,
                        SHAPE_SET,
                        UNSORTED,
                    );
                }
                None => {
                    let Some(shape_mask) = self.api.x_shape_combine_mask else {
                        return;
                    };
                    shape_mask(self.display, window, kind, 0, 0, 0, SHAPE_SET);
                }
            }
            (self.api.x_flush)(self.display);
        }
    }
}

impl Drop for OverlayServer {
    fn drop(&mut self) {
        unsafe {
            (self.api.x_close_display)(self.display);
        }
    }
}
