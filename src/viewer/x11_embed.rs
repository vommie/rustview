use super::error::ViewerError;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

type DisplayPtr = *mut c_void;
type WindowId = c_ulong;
type AtomId = c_ulong;

const RTLD_LAZY: c_int = 1;

unsafe extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct XRectangle {
    pub x: i16,
    pub y: i16,
    pub width: u16,
    pub height: u16,
}

struct X11Symbols {
    _lib_x11: *mut c_void,
    _lib_xext: *mut c_void,
    x_open_display: unsafe extern "C" fn(*const c_char) -> DisplayPtr,
    x_close_display: unsafe extern "C" fn(DisplayPtr) -> c_int,
    x_default_root_window: unsafe extern "C" fn(DisplayPtr) -> WindowId,
    x_create_simple_window: unsafe extern "C" fn(
        DisplayPtr,
        WindowId,
        c_int,
        c_int,
        c_uint,
        c_uint,
        c_uint,
        c_ulong,
        c_ulong,
    ) -> WindowId,
    x_destroy_window: unsafe extern "C" fn(DisplayPtr, WindowId) -> c_int,
    x_move_resize_window:
        unsafe extern "C" fn(DisplayPtr, WindowId, c_int, c_int, c_uint, c_uint) -> c_int,
    x_reparent_window:
        unsafe extern "C" fn(DisplayPtr, WindowId, WindowId, c_int, c_int) -> c_int,
    x_map_window: unsafe extern "C" fn(DisplayPtr, WindowId) -> c_int,
    x_unmap_window: unsafe extern "C" fn(DisplayPtr, WindowId) -> c_int,
    x_select_input: unsafe extern "C" fn(DisplayPtr, WindowId, c_long) -> c_int,
    x_flush: unsafe extern "C" fn(DisplayPtr) -> c_int,
    x_sync: unsafe extern "C" fn(DisplayPtr, c_int) -> c_int,
    x_intern_atom: unsafe extern "C" fn(DisplayPtr, *const c_char, c_int) -> AtomId,
    x_get_window_property: unsafe extern "C" fn(
        DisplayPtr,
        WindowId,
        AtomId,
        c_long,
        c_long,
        c_int,
        AtomId,
        *mut AtomId,
        *mut c_int,
        *mut c_ulong,
        *mut c_ulong,
        *mut *mut u8,
    ) -> c_int,
    x_query_tree: unsafe extern "C" fn(
        DisplayPtr,
        WindowId,
        *mut WindowId,
        *mut WindowId,
        *mut *mut WindowId,
        *mut c_uint,
    ) -> c_int,
    x_free: unsafe extern "C" fn(*mut c_void) -> c_int,
    x_shape_combine_rectangles: Option<
        unsafe extern "C" fn(
            DisplayPtr,
            WindowId,
            c_int,
            c_int,
            c_int,
            *const XRectangle,
            c_int,
            c_int,
            c_int,
        ),
    >,
    x_shape_combine_mask:
        Option<unsafe extern "C" fn(DisplayPtr, WindowId, c_int, c_int, c_int, c_ulong, c_int)>,
}

unsafe impl Send for X11Symbols {}
unsafe impl Sync for X11Symbols {}

static X11_LOADED: AtomicBool = AtomicBool::new(false);
static mut X11_API: Option<Arc<X11Symbols>> = None;

fn get_x11_api() -> Result<Arc<X11Symbols>, ViewerError> {
    if X11_LOADED.load(Ordering::Acquire) {
        unsafe {
            if let Some(ref api) = X11_API {
                return Ok(api.clone());
            }
        }
    }

    unsafe {
        let lib_x11 = dlopen(c"libX11.so.6".as_ptr(), RTLD_LAZY);
        let handle_x11 = if !lib_x11.is_null() {
            lib_x11
        } else {
            let lib_fallback = dlopen(c"libX11.so".as_ptr(), RTLD_LAZY);
            if lib_fallback.is_null() {
                return Err(ViewerError::X11("Failed to load libX11.so.6 library".into()));
            }
            lib_fallback
        };

        let lib_xext = dlopen(c"libXext.so.6".as_ptr(), RTLD_LAZY);
        let handle_xext = if !lib_xext.is_null() {
            lib_xext
        } else {
            dlopen(c"libXext.so".as_ptr(), RTLD_LAZY)
        };

        load_symbols_from_handles(handle_x11, handle_xext)
    }
}

unsafe fn load_symbols_from_handles(
    lib_x11: *mut c_void,
    lib_xext: *mut c_void,
) -> Result<Arc<X11Symbols>, ViewerError> {
    unsafe fn load_sym<T>(handle: *mut c_void, name: &str) -> Result<T, ViewerError> {
        let c_name = CString::new(name).map_err(|e| ViewerError::X11(e.to_string()))?;
        let sym = unsafe { dlsym(handle, c_name.as_ptr()) };
        if sym.is_null() {
            return Err(ViewerError::X11(format!(
                "Symbol '{}' not found in library",
                name
            )));
        }
        Ok(unsafe { std::mem::transmute_copy(&sym) })
    }

    let x_shape_combine_rectangles = if !lib_xext.is_null() {
        unsafe { load_sym(lib_xext, "XShapeCombineRectangles").ok() }
    } else {
        None
    };

    let x_shape_combine_mask = if !lib_xext.is_null() {
        unsafe { load_sym(lib_xext, "XShapeCombineMask").ok() }
    } else {
        None
    };

    let symbols = Arc::new(X11Symbols {
        _lib_x11: lib_x11,
        _lib_xext: lib_xext,
        x_open_display: unsafe { load_sym(lib_x11, "XOpenDisplay")? },
        x_close_display: unsafe { load_sym(lib_x11, "XCloseDisplay")? },
        x_default_root_window: unsafe { load_sym(lib_x11, "XDefaultRootWindow")? },
        x_create_simple_window: unsafe { load_sym(lib_x11, "XCreateSimpleWindow")? },
        x_destroy_window: unsafe { load_sym(lib_x11, "XDestroyWindow")? },
        x_move_resize_window: unsafe { load_sym(lib_x11, "XMoveResizeWindow")? },
        x_reparent_window: unsafe { load_sym(lib_x11, "XReparentWindow")? },
        x_map_window: unsafe { load_sym(lib_x11, "XMapWindow")? },
        x_unmap_window: unsafe { load_sym(lib_x11, "XUnmapWindow")? },
        x_select_input: unsafe { load_sym(lib_x11, "XSelectInput")? },
        x_flush: unsafe { load_sym(lib_x11, "XFlush")? },
        x_sync: unsafe { load_sym(lib_x11, "XSync")? },
        x_intern_atom: unsafe { load_sym(lib_x11, "XInternAtom")? },
        x_get_window_property: unsafe { load_sym(lib_x11, "XGetWindowProperty")? },
        x_query_tree: unsafe { load_sym(lib_x11, "XQueryTree")? },
        x_free: unsafe { load_sym(lib_x11, "XFree")? },
        x_shape_combine_rectangles,
        x_shape_combine_mask,
    });

    unsafe {
        X11_API = Some(symbols.clone());
        X11_LOADED.store(true, Ordering::Release);
    }

    Ok(symbols)
}

/// Retrieves the native X11 Window ID (`XID`) directly from a Slint Window via `raw-window-handle-06`.
pub fn get_slint_window_xid(window: &slint::Window) -> Option<u64> {
    if let Ok(handle) = window.window_handle().window_handle() {
        match handle.as_raw() {
            RawWindowHandle::Xlib(h) => {
                let xid = h.window;
                if xid > 0 {
                    return Some(xid);
                }
            }
            RawWindowHandle::Xcb(h) => {
                let xid = h.window.get() as u64;
                if xid > 0 {
                    return Some(xid);
                }
            }
            _ => {}
        }
    }

    find_window_by_title("RustView")
}

/// Recursively searches the X11 window hierarchy for a window whose title contains `title_contains`.
pub fn find_window_by_title(title_contains: &str) -> Option<u64> {
    let t0 = Instant::now();
    let api = get_x11_api().ok()?;

    unsafe {
        let display = (api.x_open_display)(std::ptr::null());
        if display.is_null() {
            return None;
        }

        let root = (api.x_default_root_window)(display);
        let mut found_xid = None;
        search_tree_by_title(&api, display, root, title_contains, &mut found_xid);
        (api.x_close_display)(display);

        tracing::debug!(
            title_contains,
            found_xid = ?found_xid,
            elapsed_us = t0.elapsed().as_micros(),
            "[X11WindowFinder: TitleSearch] Window tree search completed"
        );

        found_xid
    }
}

unsafe fn search_tree_by_title(
    api: &X11Symbols,
    display: DisplayPtr,
    window: WindowId,
    title_match: &str,
    out_found: &mut Option<u64>,
) {
    if out_found.is_some() {
        return;
    }

    unsafe {
        let atom_name = (api.x_intern_atom)(display, c"_NET_WM_NAME".as_ptr(), 0);
        let atom_wm_name = (api.x_intern_atom)(display, c"WM_NAME".as_ptr(), 0);
        let atom_utf8 = (api.x_intern_atom)(display, c"UTF8_STRING".as_ptr(), 0);
        let atom_string = (api.x_intern_atom)(display, c"STRING".as_ptr(), 0);

        for &name_atom in &[atom_name, atom_wm_name] {
            let mut name_type = 0;
            let mut name_format = 0;
            let mut name_items = 0;
            let mut name_after = 0;
            let mut name_data: *mut u8 = std::ptr::null_mut();

            for &req_type in &[atom_utf8, atom_string, 0] {
                let res = (api.x_get_window_property)(
                    display,
                    window,
                    name_atom,
                    0,
                    256,
                    0,
                    req_type,
                    &mut name_type,
                    &mut name_format,
                    &mut name_items,
                    &mut name_after,
                    &mut name_data,
                );

                if res == 0 && !name_data.is_null() && name_items > 0 {
                    let slice = std::slice::from_raw_parts(name_data, name_items as usize);
                    let title_str = String::from_utf8_lossy(slice);
                    if title_str.contains(title_match) {
                        *out_found = Some(window);
                    }
                    (api.x_free)(name_data as *mut c_void);
                    if out_found.is_some() {
                        return;
                    }
                }
            }
        }

        let mut root_ret = 0;
        let mut parent_ret = 0;
        let mut children_ptr: *mut WindowId = std::ptr::null_mut();
        let mut nchildren: c_uint = 0;

        let query_res = (api.x_query_tree)(
            display,
            window,
            &mut root_ret,
            &mut parent_ret,
            &mut children_ptr,
            &mut nchildren,
        );

        if query_res != 0 && !children_ptr.is_null() && nchildren > 0 {
            let children_slice = std::slice::from_raw_parts(children_ptr, nchildren as usize);
            for &child in children_slice.iter().rev() {
                search_tree_by_title(api, display, child, title_match, out_found);
                if out_found.is_some() {
                    break;
                }
            }
            (api.x_free)(children_ptr as *mut c_void);
        }
    }
}

/// Helper discovering the native X11 Window ID (`XID`) of a Slint window by PID and title string.
pub fn find_window_by_pid_and_title(target_pid: u32, title_contains: &str) -> Option<u64> {
    if let Some(xid) = find_window_by_title(title_contains) {
        return Some(xid);
    }

    let t0 = Instant::now();
    let api = get_x11_api().ok()?;

    unsafe {
        let display = (api.x_open_display)(std::ptr::null());
        if display.is_null() {
            return None;
        }

        let root = (api.x_default_root_window)(display);
        let atom_pid = (api.x_intern_atom)(display, c"_NET_WM_PID".as_ptr(), 0);

        let mut root_return = 0;
        let mut parent_return = 0;
        let mut children_ptr: *mut WindowId = std::ptr::null_mut();
        let mut nchildren: c_uint = 0;

        let query_res = (api.x_query_tree)(
            display,
            root,
            &mut root_return,
            &mut parent_return,
            &mut children_ptr,
            &mut nchildren,
        );

        let mut found_xid = None;

        if query_res != 0 && !children_ptr.is_null() && nchildren > 0 {
            let slice = std::slice::from_raw_parts(children_ptr, nchildren as usize);

            for &win_id in slice.iter().rev() {
                let mut actual_type = 0;
                let mut actual_format = 0;
                let mut nitems = 0;
                let mut bytes_after = 0;
                let mut prop_data: *mut u8 = std::ptr::null_mut();

                let res_pid = (api.x_get_window_property)(
                    display,
                    win_id,
                    atom_pid,
                    0,
                    1,
                    0,
                    6,
                    &mut actual_type,
                    &mut actual_format,
                    &mut nitems,
                    &mut bytes_after,
                    &mut prop_data,
                );

                if res_pid == 0 && !prop_data.is_null() && nitems > 0 {
                    let win_pid = *(prop_data as *const u32);
                    (api.x_free)(prop_data as *mut c_void);

                    if win_pid == target_pid {
                        found_xid = Some(win_id);
                        break;
                    }
                }
            }

            (api.x_free)(children_ptr as *mut c_void);
        }

        (api.x_close_display)(display);

        tracing::debug!(
            target_pid,
            title_contains,
            found_xid = ?found_xid,
            elapsed_us = t0.elapsed().as_micros(),
            "[X11WindowFinder: PidSearch] Window handle query completed"
        );

        found_xid
    }
}

/// Managed X11 Video Subwindow container embedded directly into Slint's central media viewport.
/// Supports dynamic nonrectangular XShape clipping to render Slint context menus and popups on top.
pub struct X11VideoSubwindow {
    api: Arc<X11Symbols>,
    display: DisplayPtr,
    subwindow_xid: WindowId,
    parent_xid: WindowId,
    current_x: i32,
    current_y: i32,
    current_w: u32,
    current_h: u32,
    is_visible: bool,
}

unsafe impl Send for X11VideoSubwindow {}
unsafe impl Sync for X11VideoSubwindow {}

impl X11VideoSubwindow {
    /// Creates and initializes a new embedded subwindow attached to `parent_xid`.
    pub fn new(
        parent_xid: u64,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<Self, ViewerError> {
        let t0 = Instant::now();
        let api = get_x11_api()?;

        unsafe {
            let display = (api.x_open_display)(std::ptr::null());
            if display.is_null() {
                return Err(ViewerError::X11("Failed to open default X11 display".into()));
            }

            let subwindow_xid = (api.x_create_simple_window)(
                display,
                parent_xid,
                x as c_int,
                y as c_int,
                width.max(16) as c_uint,
                height.max(16) as c_uint,
                0,
                0,
                0,
            );

            if subwindow_xid == 0 {
                (api.x_close_display)(display);
                return Err(ViewerError::X11(
                    "Failed to create X11 simple subwindow".into(),
                ));
            }

            (api.x_select_input)(
                display,
                subwindow_xid,
                (1 << 17) | (1 << 15) | (1 << 0),
            );

            (api.x_sync)(display, 0);

            tracing::info!(
                parent_xid,
                subwindow_xid,
                x,
                y,
                width,
                height,
                elapsed_us = t0.elapsed().as_micros(),
                "[X11VideoSubwindow: Create] Created embedded X11 subwindow for native MPV output"
            );

            Ok(Self {
                api,
                display,
                subwindow_xid,
                parent_xid,
                current_x: x,
                current_y: y,
                current_w: width.max(16),
                current_h: height.max(16),
                is_visible: false,
            })
        }
    }

    #[inline]
    pub fn xid(&self) -> u64 {
        self.subwindow_xid
    }

    #[inline]
    pub fn parent_xid(&self) -> u64 {
        self.parent_xid
    }

    /// Adjusts coordinates and dimensions of the subwindow to match Slint's central media viewport.
    pub fn move_resize(&mut self, x: i32, y: i32, width: u32, height: u32) {
        let w_clamped = width.max(16);
        let h_clamped = height.max(16);

        if self.current_x == x
            && self.current_y == y
            && self.current_w == w_clamped
            && self.current_h == h_clamped
        {
            return;
        }

        self.current_x = x;
        self.current_y = y;
        self.current_w = w_clamped;
        self.current_h = h_clamped;

        unsafe {
            (self.api.x_move_resize_window)(
                self.display,
                self.subwindow_xid,
                x as c_int,
                y as c_int,
                w_clamped as c_uint,
                h_clamped as c_uint,
            );
            (self.api.x_flush)(self.display);
        }
    }

    /// Seamlessly reparents the running video subwindow to a new parent window without restarting playback.
    pub fn reparent(
        &mut self,
        new_parent_xid: u64,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> Result<(), ViewerError> {
        let t0 = Instant::now();
        let target_parent = new_parent_xid;
        let w_clamped = width.max(16);
        let h_clamped = height.max(16);

        unsafe {
            let res = (self.api.x_reparent_window)(
                self.display,
                self.subwindow_xid,
                target_parent,
                x as c_int,
                y as c_int,
            );

            if res == 0 {
                return Err(ViewerError::X11(format!(
                    "XReparentWindow failed for target parent #{}",
                    new_parent_xid
                )));
            }

            (self.api.x_move_resize_window)(
                self.display,
                self.subwindow_xid,
                x as c_int,
                y as c_int,
                w_clamped as c_uint,
                h_clamped as c_uint,
            );

            (self.api.x_map_window)(self.display, self.subwindow_xid);
            (self.api.x_sync)(self.display, 0);

            self.parent_xid = target_parent;
            self.current_x = x;
            self.current_y = y;
            self.current_w = w_clamped;
            self.current_h = h_clamped;
            self.is_visible = true;

            tracing::info!(
                subwindow_xid = self.subwindow_xid,
                new_parent_xid,
                width = w_clamped,
                height = h_clamped,
                elapsed_us = t0.elapsed().as_micros(),
                "[X11VideoSubwindow: Reparent] Subwindow reparented seamlessly"
            );

            Ok(())
        }
    }

    /// Clips out specified cutout rectangles from the video subwindow using X11 Nonrectangular Shape extension.
    /// The video continues playing and remains visible everywhere else while Slint context menus display on top!
    pub fn clip_cutout_rectangles(&mut self, main_win_rects: &[XRectangle]) {
        let Some(ref shape_rects_fn) = self.api.x_shape_combine_rectangles else {
            return;
        };
        let Some(ref shape_mask_fn) = self.api.x_shape_combine_mask else {
            return;
        };

        unsafe {
            // 1. Reset shape to full bounding rectangle first
            shape_mask_fn(self.display, self.subwindow_xid, 0, 0, 0, 0, 0);

            // 2. Convert coordinates from main window space to subwindow space
            let mut local_rects: Vec<XRectangle> = Vec::with_capacity(main_win_rects.len());
            for r in main_win_rects {
                let local_x = (r.x as i32) - self.current_x;
                let local_y = (r.y as i32) - self.current_y;
                let local_right = local_x + (r.width as i32);
                let local_bottom = local_y + (r.height as i32);

                if local_right > 0
                    && local_bottom > 0
                    && local_x < (self.current_w as i32)
                    && local_y < (self.current_h as i32)
                {
                    let clamped_x = local_x.max(0) as i16;
                    let clamped_y = local_y.max(0) as i16;
                    let clamped_w = ((local_right.min(self.current_w as i32)) - (clamped_x as i32)).max(1) as u16;
                    let clamped_h = ((local_bottom.min(self.current_h as i32)) - (clamped_y as i32)).max(1) as u16;

                    local_rects.push(XRectangle {
                        x: clamped_x,
                        y: clamped_y,
                        width: clamped_w,
                        height: clamped_h,
                    });
                }
            }

            // 3. Subtract all intersecting cutout rectangles
            if !local_rects.is_empty() {
                shape_rects_fn(
                    self.display,
                    self.subwindow_xid,
                    0, // ShapeBounding
                    0,
                    0,
                    local_rects.as_ptr(),
                    local_rects.len() as c_int,
                    3, // ShapeSubtract
                    0, // Unsorted
                );
            }

            (self.api.x_flush)(self.display);
        }
    }

    /// Resets the window shape to the full standard rectangle.
    pub fn reset_clip_shape(&mut self) {
        if let Some(ref shape_mask_fn) = self.api.x_shape_combine_mask {
            unsafe {
                shape_mask_fn(self.display, self.subwindow_xid, 0, 0, 0, 0, 0);
                (self.api.x_flush)(self.display);
            }
        }
    }

    /// Makes the video subwindow visible and raises it.
    pub fn show(&mut self) {
        if !self.is_visible {
            unsafe {
                (self.api.x_map_window)(self.display, self.subwindow_xid);
                (self.api.x_flush)(self.display);
            }
            self.is_visible = true;
        }
    }

    /// Unmaps and hides the video subwindow.
    pub fn hide(&mut self) {
        if self.is_visible {
            unsafe {
                (self.api.x_unmap_window)(self.display, self.subwindow_xid);
                (self.api.x_flush)(self.display);
            }
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
        unsafe {
            (self.api.x_destroy_window)(self.display, self.subwindow_xid);
            (self.api.x_close_display)(self.display);
            tracing::debug!(
                subwindow_xid = self.subwindow_xid,
                "[X11VideoSubwindow: Drop] Destroyed video subwindow and closed X11 display"
            );
        }
    }
}
