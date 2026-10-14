use super::error::ViewerError;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void, CString};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

mod overlay_server;
pub use overlay_server::OverlayServer;

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

#[repr(C)]
struct ClientMessageData {
    data: [c_long; 5],
}

#[repr(C)]
struct ClientMessageEv {
    type_: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: DisplayPtr,
    window: WindowId,
    message_type: AtomId,
    format: c_int,
    data: ClientMessageData,
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
    x_send_event:
        unsafe extern "C" fn(DisplayPtr, WindowId, c_int, c_long, *mut c_void) -> c_int,
    x_change_property: unsafe extern "C" fn(
        DisplayPtr,
        WindowId,
        AtomId,
        AtomId,
        c_int,
        c_int,
        *const u8,
        c_int,
    ) -> c_int,
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
    x_query_pointer: unsafe extern "C" fn(
        DisplayPtr,
        WindowId,
        *mut WindowId,
        *mut WindowId,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_uint,
    ) -> c_int,
    x_set_error_handler: unsafe extern "C" fn(XErrorHandler) -> XErrorHandler,
    x_default_screen: unsafe extern "C" fn(DisplayPtr) -> c_int,
    x_get_selection_owner: unsafe extern "C" fn(DisplayPtr, AtomId) -> WindowId,
    x_translate_coordinates: unsafe extern "C" fn(
        DisplayPtr,
        WindowId,
        WindowId,
        c_int,
        c_int,
        *mut c_int,
        *mut c_int,
        *mut WindowId,
    ) -> c_int,
    x_get_geometry: unsafe extern "C" fn(
        DisplayPtr,
        WindowId,
        *mut WindowId,
        *mut c_int,
        *mut c_int,
        *mut c_uint,
        *mut c_uint,
        *mut c_uint,
        *mut c_uint,
    ) -> c_int,
    x_raise_window: unsafe extern "C" fn(DisplayPtr, WindowId) -> c_int,
}

#[repr(C)]
struct XErrorEvent {
    type_: c_int,
    display: DisplayPtr,
    resource_id: c_ulong,
    serial: c_ulong,
    error_code: u8,
    request_code: u8,
    minor_code: u8,
}

type XErrorHandler = Option<unsafe extern "C" fn(DisplayPtr, *mut XErrorEvent) -> c_int>;

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
        x_send_event: unsafe { load_sym(lib_x11, "XSendEvent")? },
        x_change_property: unsafe { load_sym(lib_x11, "XChangeProperty")? },
        x_shape_combine_rectangles,
        x_shape_combine_mask,
        x_query_pointer: unsafe { load_sym(lib_x11, "XQueryPointer")? },
        x_set_error_handler: unsafe { load_sym(lib_x11, "XSetErrorHandler")? },
        x_default_screen: unsafe { load_sym(lib_x11, "XDefaultScreen")? },
        x_get_selection_owner: unsafe { load_sym(lib_x11, "XGetSelectionOwner")? },
        x_translate_coordinates: unsafe { load_sym(lib_x11, "XTranslateCoordinates")? },
        x_get_geometry: unsafe { load_sym(lib_x11, "XGetGeometry")? },
        x_raise_window: unsafe { load_sym(lib_x11, "XRaiseWindow")? },
    });

    unsafe {
        X11_API = Some(symbols.clone());
        X11_LOADED.store(true, Ordering::Release);
    }

    Ok(symbols)
}

/// Retrieves the raw native X11 Window ID directly from Slint's internal window handle with zero heuristic searching.
pub fn get_raw_window_xid(window: &slint::Window) -> Option<u64> {
    if let Ok(handle) = window.window_handle().window_handle() {
        match handle.as_raw() {
            RawWindowHandle::Xlib(h) => {
                if h.window > 0 {
                    return Some(h.window);
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
    None
}

/// Retrieves the native X11 Window ID (`XID`) directly from a Slint Window with "RustView" title fallback.
pub fn get_slint_window_xid(window: &slint::Window) -> Option<u64> {
    get_slint_window_xid_with_title_fallback(window, "RustView")
}

/// Retrieves the native X11 Window ID (`XID`) from a Slint Window with process PID and strict title verification.
pub fn get_slint_window_xid_with_title_fallback(
    window: &slint::Window,
    fallback_title: &str,
) -> Option<u64> {
    if let Some(xid) = get_raw_window_xid(window) {
        return Some(xid);
    }

    find_window_by_pid_and_title(std::process::id(), fallback_title)
}

/// Discovers the native X11 Window ID (`XID`) belonging to `target_pid` whose title contains `title_contains`.
/// Performs STRICT title matching to prevent accidentally capturing other windows of the same process.
pub fn find_window_by_pid_and_title(target_pid: u32, title_contains: &str) -> Option<u64> {
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
                    0,
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
                        if title_contains.is_empty() {
                            found_xid = Some(win_id);
                            break;
                        }

                        let mut title_found = None;
                        search_tree_by_title(&api, display, win_id, title_contains, &mut title_found);
                        if title_found.is_some() {
                            found_xid = Some(win_id);
                            break;
                        }
                    }
                }
            }

            (api.x_free)(children_ptr as *mut c_void);
        }

        (api.x_close_display)(display);

        found_xid
    }
}

pub fn find_window_by_title(title_contains: &str) -> Option<u64> {
    find_window_by_pid_and_title(std::process::id(), title_contains)
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
                    // Match window title strictly to avoid matching "RustView - Settings" when searching for "RustView"
                    let matches = if title_match == "RustView" {
                        title_str.trim() == "RustView"
                    } else {
                        title_str.contains(title_match)
                    };

                    if matches {
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

pub fn is_x11_window_maximized(window_xid: u64) -> Option<bool> {
    if window_xid == 0 {
        return None;
    }

    let api = get_x11_api().ok()?;

    unsafe {
        let display = (api.x_open_display)(std::ptr::null());
        if display.is_null() {
            return None;
        }

        let atom_net_wm_state = (api.x_intern_atom)(display, c"_NET_WM_STATE".as_ptr(), 0);
        let atom_max_vert =
            (api.x_intern_atom)(display, c"_NET_WM_STATE_MAXIMIZED_VERT".as_ptr(), 0);
        let atom_max_horz =
            (api.x_intern_atom)(display, c"_NET_WM_STATE_MAXIMIZED_HORZ".as_ptr(), 0);

        let mut actual_type = 0;
        let mut actual_format = 0;
        let mut nitems = 0;
        let mut bytes_after = 0;
        let mut prop_data: *mut u8 = std::ptr::null_mut();

        let res = (api.x_get_window_property)(
            display,
            window_xid,
            atom_net_wm_state,
            0,
            1024,
            0,
            0,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop_data,
        );

        let mut has_vert = false;
        let mut has_horz = false;

        if res == 0 && !prop_data.is_null() && nitems > 0 {
            let slice = std::slice::from_raw_parts(prop_data as *const AtomId, nitems as usize);
            for &atom in slice {
                if atom == atom_max_vert {
                    has_vert = true;
                }
                if atom == atom_max_horz {
                    has_horz = true;
                }
            }
            (api.x_free)(prop_data as *mut c_void);
        }

        (api.x_close_display)(display);

        let is_max = has_vert && has_horz;
        Some(is_max)
    }
}

pub fn force_x11_maximized(window_xid: u64, maximized: bool) {
    if window_xid == 0 {
        return;
    }

    let Ok(api) = get_x11_api() else {
        return;
    };

    unsafe {
        let display = (api.x_open_display)(std::ptr::null());
        if display.is_null() {
            return;
        }

        let root = (api.x_default_root_window)(display);
        let atom_net_wm_state = (api.x_intern_atom)(display, c"_NET_WM_STATE".as_ptr(), 0);
        let atom_max_vert =
            (api.x_intern_atom)(display, c"_NET_WM_STATE_MAXIMIZED_VERT".as_ptr(), 0);
        let atom_max_horz =
            (api.x_intern_atom)(display, c"_NET_WM_STATE_MAXIMIZED_HORZ".as_ptr(), 0);
        let atom_atom = (api.x_intern_atom)(display, c"ATOM".as_ptr(), 0);

        if maximized {
            let states = [atom_max_vert, atom_max_horz];
            (api.x_change_property)(
                display,
                window_xid,
                atom_net_wm_state,
                atom_atom,
                32,
                0,
                states.as_ptr() as *const u8,
                2,
            );
        }

        let mut ev = ClientMessageEv {
            type_: 33,
            serial: 0,
            send_event: 1,
            display,
            window: window_xid,
            message_type: atom_net_wm_state,
            format: 32,
            data: ClientMessageData {
                data: [
                    if maximized { 1 } else { 0 },
                    atom_max_vert as c_long,
                    atom_max_horz as c_long,
                    1,
                    0,
                ],
            },
        };

        let mask = (1 << 19) | (1 << 20);
        (api.x_send_event)(
            display,
            root,
            0,
            mask,
            &mut ev as *mut _ as *mut c_void,
        );

        (api.x_flush)(display);
        (api.x_close_display)(display);
    }
}

pub fn force_x11_fullscreen(window_xid: u64) {
    if window_xid == 0 {
        return;
    }

    let Ok(api) = get_x11_api() else {
        return;
    };

    unsafe {
        let display = (api.x_open_display)(std::ptr::null());
        if display.is_null() {
            return;
        }

        let root = (api.x_default_root_window)(display);
        let atom_net_wm_state = (api.x_intern_atom)(display, c"_NET_WM_STATE".as_ptr(), 0);
        let atom_net_wm_fullscreen =
            (api.x_intern_atom)(display, c"_NET_WM_STATE_FULLSCREEN".as_ptr(), 0);
        let atom_atom = (api.x_intern_atom)(display, c"ATOM".as_ptr(), 0);
        let atom_motif = (api.x_intern_atom)(display, c"_MOTIF_WM_HINTS".as_ptr(), 0);

        #[repr(C)]
        struct MotifHints {
            flags: c_ulong,
            functions: c_ulong,
            decorations: c_ulong,
            input_mode: c_long,
            status: c_ulong,
        }
        let hints = MotifHints {
            flags: 2,
            functions: 0,
            decorations: 0,
            input_mode: 0,
            status: 0,
        };
        (api.x_change_property)(
            display,
            window_xid,
            atom_motif,
            atom_motif,
            32,
            0,
            &hints as *const _ as *const u8,
            5,
        );

        let states = [atom_net_wm_fullscreen];
        (api.x_change_property)(
            display,
            window_xid,
            atom_net_wm_state,
            atom_atom,
            32,
            0,
            states.as_ptr() as *const u8,
            1,
        );

        let mut ev = ClientMessageEv {
            type_: 33,
            serial: 0,
            send_event: 1,
            display,
            window: window_xid,
            message_type: atom_net_wm_state,
            format: 32,
            data: ClientMessageData {
                data: [
                    1,
                    atom_net_wm_fullscreen as c_long,
                    0,
                    1,
                    0,
                ],
            },
        };

        let mask = (1 << 19) | (1 << 20);
        (api.x_send_event)(
            display,
            root,
            0,
            mask,
            &mut ev as *mut _ as *mut c_void,
        );

        (api.x_flush)(display);
        (api.x_close_display)(display);

        tracing::debug!(
            window_xid,
            "[X11Fullscreen: Enforce] Enforced true EWMH fullscreen state and stripped decorations via X11"
        );
    }
}

/// Enforces true EWMH fullscreen state on a specific monitor using `_NET_WM_FULLSCREEN_MONITORS`.
pub fn force_x11_fullscreen_on_monitor(
    window_xid: u64,
    monitor_index: usize,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) {
    if window_xid == 0 {
        return;
    }

    let Ok(api) = get_x11_api() else {
        return;
    };

    unsafe {
        let display = (api.x_open_display)(std::ptr::null());
        if display.is_null() {
            return;
        }

        let root = (api.x_default_root_window)(display);
        let atom_net_wm_state = (api.x_intern_atom)(display, c"_NET_WM_STATE".as_ptr(), 0);
        let atom_net_wm_fullscreen =
            (api.x_intern_atom)(display, c"_NET_WM_STATE_FULLSCREEN".as_ptr(), 0);
        let atom_fullscreen_monitors =
            (api.x_intern_atom)(display, c"_NET_WM_FULLSCREEN_MONITORS".as_ptr(), 0);
        let atom_atom = (api.x_intern_atom)(display, c"ATOM".as_ptr(), 0);
        let atom_cardinal = (api.x_intern_atom)(display, c"CARDINAL".as_ptr(), 0);
        let atom_motif = (api.x_intern_atom)(display, c"_MOTIF_WM_HINTS".as_ptr(), 0);

        // 1. Strip window decorations
        #[repr(C)]
        struct MotifHints {
            flags: c_ulong,
            functions: c_ulong,
            decorations: c_ulong,
            input_mode: c_long,
            status: c_ulong,
        }
        let hints = MotifHints {
            flags: 2,
            functions: 0,
            decorations: 0,
            input_mode: 0,
            status: 0,
        };
        (api.x_change_property)(
            display,
            window_xid,
            atom_motif,
            atom_motif,
            32,
            0,
            &hints as *const _ as *const u8,
            5,
        );

        // 2. Set window property _NET_WM_FULLSCREEN_MONITORS on client window
        let mon_idx = monitor_index as c_long;
        let mon_indices = [mon_idx, mon_idx, mon_idx, mon_idx];
        (api.x_change_property)(
            display,
            window_xid,
            atom_fullscreen_monitors,
            atom_cardinal,
            32,
            0,
            mon_indices.as_ptr() as *const u8,
            4,
        );

        // 3. Send _NET_WM_FULLSCREEN_MONITORS client message to root window
        let mut ev_mon = ClientMessageEv {
            type_: 33,
            serial: 0,
            send_event: 1,
            display,
            window: window_xid,
            message_type: atom_fullscreen_monitors,
            format: 32,
            data: ClientMessageData {
                data: [
                    mon_idx,
                    mon_idx,
                    mon_idx,
                    mon_idx,
                    1, // source indication (1 = normal application)
                ],
            },
        };
        let mask = (1 << 19) | (1 << 20);
        (api.x_send_event)(
            display,
            root,
            0,
            mask,
            &mut ev_mon as *mut _ as *mut c_void,
        );

        // 4. Move and resize window to the target monitor bounds
        (api.x_move_resize_window)(
            display,
            window_xid,
            x as c_int,
            y as c_int,
            width.max(16) as c_uint,
            height.max(16) as c_uint,
        );

        // 5. Set _NET_WM_STATE property
        let states = [atom_net_wm_fullscreen];
        (api.x_change_property)(
            display,
            window_xid,
            atom_net_wm_state,
            atom_atom,
            32,
            0,
            states.as_ptr() as *const u8,
            1,
        );

        // 6. Send _NET_WM_STATE_FULLSCREEN client message to root window
        let mut ev_fs = ClientMessageEv {
            type_: 33,
            serial: 0,
            send_event: 1,
            display,
            window: window_xid,
            message_type: atom_net_wm_state,
            format: 32,
            data: ClientMessageData {
                data: [
                    1, // _NET_WM_STATE_ADD
                    atom_net_wm_fullscreen as c_long,
                    0,
                    1,
                    0,
                ],
            },
        };
        (api.x_send_event)(
            display,
            root,
            0,
            mask,
            &mut ev_fs as *mut _ as *mut c_void,
        );

        (api.x_flush)(display);
        (api.x_close_display)(display);

        tracing::info!(
            window_xid,
            monitor_index,
            x,
            y,
            width,
            height,
            "[X11Fullscreen: TargetMonitor] Positioned and fullscreened window onto target monitor via EWMH"
        );
    }
}

unsafe extern "C" fn nonfatal_x_error(_display: DisplayPtr, event: *mut XErrorEvent) -> c_int {
    if let Some(ev) = unsafe { event.as_ref() } {
        tracing::debug!(
            error_code = ev.error_code,
            request_code = ev.request_code,
            minor_code = ev.minor_code,
            resource_id = ev.resource_id,
            "[X11: Error] Ignored non-fatal X protocol error"
        );
    }
    0
}

/// Installs an X error handler that only logs. libmpv resets the process-wide Xlib error handler to Xlib's default when a
/// video output shuts down, and that default terminates the whole process on any later X error (for example a stale
/// window id). Call this after a player has been destroyed.
pub fn install_nonfatal_error_handler() {
    if let Ok(api) = get_x11_api() {
        unsafe {
            (api.x_set_error_handler)(Some(nonfatal_x_error));
        }
    }
}

/// Reads the global pointer position with a single `XQueryPointer` round trip on a private connection.
pub struct PointerProbe {
    api: Arc<X11Symbols>,
    display: DisplayPtr,
    root: WindowId,
}

unsafe impl Send for PointerProbe {}
unsafe impl Sync for PointerProbe {}

impl PointerProbe {
    pub fn new() -> Result<Self, ViewerError> {
        let api = get_x11_api()?;
        unsafe {
            let display = (api.x_open_display)(std::ptr::null());
            if display.is_null() {
                return Err(ViewerError::X11(
                    "Failed to open X11 display for the pointer probe".into(),
                ));
            }
            let root = (api.x_default_root_window)(display);
            Ok(Self { api, display, root })
        }
    }

    /// Pointer position in root window (screen) coordinates.
    pub fn position(&self) -> Option<(i32, i32)> {
        self.query(self.root).map(|(root_x, root_y, _, _)| (root_x, root_y))
    }

    /// Pointer position relative to the top-left corner of `window_xid` (it does not have to contain the pointer).
    pub fn position_in(&self, window_xid: u64) -> Option<(i32, i32)> {
        self.query(window_xid).map(|(_, _, win_x, win_y)| (win_x, win_y))
    }

    /// Returns `(root_x, root_y, win_x, win_y)` of the pointer, the last two relative to `window`.
    fn query(&self, window: WindowId) -> Option<(i32, i32, i32, i32)> {
        let mut root_ret: WindowId = 0;
        let mut child_ret: WindowId = 0;
        let (mut root_x, mut root_y, mut win_x, mut win_y): (c_int, c_int, c_int, c_int) =
            (0, 0, 0, 0);
        let mut mask: c_uint = 0;
        let on_screen = unsafe {
            (self.api.x_query_pointer)(
                self.display,
                window,
                &mut root_ret,
                &mut child_ret,
                &mut root_x,
                &mut root_y,
                &mut win_x,
                &mut win_y,
                &mut mask,
            )
        };
        (on_screen != 0).then_some((root_x, root_y, win_x, win_y))
    }
}

impl Drop for PointerProbe {
    fn drop(&mut self) {
        unsafe {
            (self.api.x_close_display)(self.display);
        }
    }
}

/// Managed X11 Video Subwindow container embedded directly into Slint's central media viewport.
pub struct X11VideoSubwindow {
    api: Arc<X11Symbols>,
    display: DisplayPtr,
    container_xid: WindowId,
    subwindow_xid: WindowId,
    parent_xid: WindowId,
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
}

unsafe impl Send for X11VideoSubwindow {}
unsafe impl Sync for X11VideoSubwindow {}

impl X11VideoSubwindow {
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

            let w_clamped = width.max(16);
            let h_clamped = height.max(16);

            let container_xid = (api.x_create_simple_window)(
                display,
                parent_xid,
                x as c_int,
                y as c_int,
                w_clamped as c_uint,
                h_clamped as c_uint,
                0,
                0,
                0,
            );

            if container_xid == 0 {
                (api.x_close_display)(display);
                return Err(ViewerError::X11(
                    "Failed to create X11 container subwindow".into(),
                ));
            }

            let subwindow_xid = (api.x_create_simple_window)(
                display,
                container_xid,
                0,
                0,
                w_clamped as c_uint,
                h_clamped as c_uint,
                0,
                0,
                0,
            );

            if subwindow_xid == 0 {
                (api.x_destroy_window)(display, container_xid);
                (api.x_close_display)(display);
                return Err(ViewerError::X11(
                    "Failed to create X11 video subwindow".into(),
                ));
            }

            (api.x_select_input)(
                display,
                container_xid,
                (1 << 17) | (1 << 15) | (1 << 0),
            );

            (api.x_select_input)(
                display,
                subwindow_xid,
                (1 << 17) | (1 << 15) | (1 << 0),
            );

            (api.x_map_window)(display, subwindow_xid);
            (api.x_sync)(display, 0);

            tracing::info!(
                parent_xid,
                container_xid,
                subwindow_xid,
                x,
                y,
                width,
                height,
                elapsed_us = t0.elapsed().as_micros(),
                "[X11VideoSubwindow: Create] Created container + video subwindow pair for unscaled clipping"
            );

            Ok(Self {
                api,
                display,
                container_xid,
                subwindow_xid,
                parent_xid,
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

    pub fn move_resize(&mut self, x: i32, y: i32, width: u32, height: u32) {
        let w_clamped = width.max(16);
        let h_clamped = height.max(16);

        unsafe {
            if self.current_container_x != x
                || self.current_container_y != y
                || self.current_container_w != w_clamped
                || self.current_container_h != h_clamped
            {
                self.current_container_x = x;
                self.current_container_y = y;
                self.current_container_w = w_clamped;
                self.current_container_h = h_clamped;

                (self.api.x_move_resize_window)(
                    self.display,
                    self.container_xid,
                    x as c_int,
                    y as c_int,
                    w_clamped as c_uint,
                    h_clamped as c_uint,
                );
            }

            if self.current_sub_x != 0
                || self.current_sub_y != 0
                || self.current_sub_w != w_clamped
                || self.current_sub_h != h_clamped
            {
                self.current_sub_x = 0;
                self.current_sub_y = 0;
                self.current_sub_w = w_clamped;
                self.current_sub_h = h_clamped;

                (self.api.x_move_resize_window)(
                    self.display,
                    self.subwindow_xid,
                    0,
                    0,
                    w_clamped as c_uint,
                    h_clamped as c_uint,
                );
            }

            (self.api.x_flush)(self.display);
        }
    }

    pub fn update_geometry_and_clip(
        &mut self,
        geom: SubwindowGeometry,
        viewport: ViewportBounds,
    ) -> bool {
        let w_clamped = geom.width.max(16) as i32;
        let h_clamped = geom.height.max(16) as i32;
        let view_w_i32 = viewport.width.max(16) as i32;
        let view_h_i32 = viewport.height.max(16) as i32;

        let vis_x1 = geom.x.max(viewport.x);
        let vis_y1 = geom.y.max(viewport.y);
        let vis_x2 = (geom.x + w_clamped).min(viewport.x + view_w_i32);
        let vis_y2 = (geom.y + h_clamped).min(viewport.y + view_h_i32);

        let vis_w = vis_x2 - vis_x1;
        let vis_h = vis_y2 - vis_y1;

        if vis_w <= 0 || vis_h <= 0 {
            self.hide();
            return false;
        }

        let sub_offset_x = geom.x - vis_x1;
        let sub_offset_y = geom.y - vis_y1;

        unsafe {
            if self.current_container_x != vis_x1
                || self.current_container_y != vis_y1
                || self.current_container_w != vis_w as u32
                || self.current_container_h != vis_h as u32
            {
                self.current_container_x = vis_x1;
                self.current_container_y = vis_y1;
                self.current_container_w = vis_w as u32;
                self.current_container_h = vis_h as u32;

                (self.api.x_move_resize_window)(
                    self.display,
                    self.container_xid,
                    vis_x1 as c_int,
                    vis_y1 as c_int,
                    vis_w as c_uint,
                    vis_h as c_uint,
                );
            }

            if self.current_sub_x != sub_offset_x
                || self.current_sub_y != sub_offset_y
                || self.current_sub_w != w_clamped as u32
                || self.current_sub_h != h_clamped as u32
            {
                self.current_sub_x = sub_offset_x;
                self.current_sub_y = sub_offset_y;
                self.current_sub_w = w_clamped as u32;
                self.current_sub_h = h_clamped as u32;

                (self.api.x_move_resize_window)(
                    self.display,
                    self.subwindow_xid,
                    sub_offset_x as c_int,
                    sub_offset_y as c_int,
                    w_clamped as c_uint,
                    h_clamped as c_uint,
                );
            }

            (self.api.x_flush)(self.display);
        }

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
        let target_parent = new_parent_xid;
        let w_clamped = width.max(16);
        let h_clamped = height.max(16);

        unsafe {
            let res = (self.api.x_reparent_window)(
                self.display,
                self.container_xid,
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

            self.current_container_x = x;
            self.current_container_y = y;
            self.current_container_w = w_clamped;
            self.current_container_h = h_clamped;

            (self.api.x_move_resize_window)(
                self.display,
                self.container_xid,
                x as c_int,
                y as c_int,
                w_clamped as c_uint,
                h_clamped as c_uint,
            );

            self.current_sub_x = 0;
            self.current_sub_y = 0;
            self.current_sub_w = w_clamped;
            self.current_sub_h = h_clamped;

            (self.api.x_move_resize_window)(
                self.display,
                self.subwindow_xid,
                0,
                0,
                w_clamped as c_uint,
                h_clamped as c_uint,
            );

            (self.api.x_map_window)(self.display, self.subwindow_xid);
            (self.api.x_map_window)(self.display, self.container_xid);
            (self.api.x_sync)(self.display, 0);

            self.parent_xid = target_parent;
            self.is_visible = true;
            self.is_clipped = false;
            self.last_applied_local_rects.clear();

            tracing::info!(
                container_xid = self.container_xid,
                subwindow_xid = self.subwindow_xid,
                new_parent_xid,
                width = w_clamped,
                height = h_clamped,
                elapsed_us = t0.elapsed().as_micros(),
                "[X11VideoSubwindow: Reparent] Subwindow container reparented seamlessly"
            );

            Ok(())
        }
    }

    /// Punches transparent cutout holes out of the X11 container window so that context menus and tooltips
    /// are ALWAYS rendered on top of the MPV player without hardware occlusion.
    pub fn clip_cutout_rectangles(&mut self, main_win_rects: &[XRectangle]) {
        let Some(ref shape_rects_fn) = self.api.x_shape_combine_rectangles else {
            return;
        };
        let Some(ref shape_mask_fn) = self.api.x_shape_combine_mask else {
            return;
        };

        if !self.is_visible {
            return;
        }

        let container_w = self.current_container_w;
        let container_h = self.current_container_h;
        if container_w == 0 || container_h == 0 {
            return;
        }

        let mut container_cutouts: Vec<XRectangle> = Vec::with_capacity(main_win_rects.len());

        for r in main_win_rects {
            let local_x = (r.x as i32) - self.current_container_x;
            let local_y = (r.y as i32) - self.current_container_y;
            let local_right = local_x + (r.width as i32);
            let local_bottom = local_y + (r.height as i32);

            if local_right > 0
                && local_bottom > 0
                && local_x < (container_w as i32)
                && local_y < (container_h as i32)
            {
                let clamped_x = local_x.max(0) as i16;
                let clamped_y = local_y.max(0) as i16;
                let clamped_w =
                    ((local_right.min(container_w as i32)) - (clamped_x as i32)).max(1) as u16;
                let clamped_h =
                    ((local_bottom.min(container_h as i32)) - (clamped_y as i32)).max(1) as u16;

                container_cutouts.push(XRectangle {
                    x: clamped_x,
                    y: clamped_y,
                    width: clamped_w,
                    height: clamped_h,
                });
            }
        }

        if self.last_applied_local_rects == container_cutouts
            && self.is_clipped == !container_cutouts.is_empty()
        {
            return;
        }

        unsafe {
            if container_cutouts.is_empty() {
                shape_mask_fn(self.display, self.container_xid, 0, 0, 0, 0, 0);
                self.is_clipped = false;
                self.last_applied_local_rects.clear();
            } else {
                let full_rect_container = XRectangle {
                    x: 0,
                    y: 0,
                    width: container_w.min(32767) as u16,
                    height: container_h.min(32767) as u16,
                };
                shape_rects_fn(
                    self.display,
                    self.container_xid,
                    0, // ShapeBounding
                    0,
                    0,
                    &full_rect_container,
                    1,
                    0, // ShapeSet
                    0, // Unsorted
                );

                shape_rects_fn(
                    self.display,
                    self.container_xid,
                    0, // ShapeBounding
                    0,
                    0,
                    container_cutouts.as_ptr(),
                    container_cutouts.len() as c_int,
                    3, // ShapeSubtract
                    0, // Unsorted
                );

                self.is_clipped = true;
                self.last_applied_local_rects = container_cutouts;
            }

            (self.api.x_flush)(self.display);
        }
    }

    pub fn reset_clip_shape(&mut self) {
        if let Some(ref shape_mask_fn) = self.api.x_shape_combine_mask {
            unsafe {
                shape_mask_fn(self.display, self.container_xid, 0, 0, 0, 0, 0);
                (self.api.x_flush)(self.display);
            }
        }
        self.is_clipped = false;
        self.last_applied_local_rects.clear();
    }

    pub fn show(&mut self) {
        if !self.is_visible {
            unsafe {
                (self.api.x_map_window)(self.display, self.subwindow_xid);
                (self.api.x_map_window)(self.display, self.container_xid);
                (self.api.x_flush)(self.display);
            }
            self.is_visible = true;
        }
    }

    pub fn hide(&mut self) {
        if self.is_visible {
            unsafe {
                (self.api.x_unmap_window)(self.display, self.container_xid);
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
            (self.api.x_destroy_window)(self.display, self.container_xid);
            (self.api.x_close_display)(self.display);
            tracing::debug!(
                container_xid = self.container_xid,
                subwindow_xid = self.subwindow_xid,
                "[X11VideoSubwindow: Drop] Destroyed subwindow pair and closed X11 display"
            );
        }
    }
}
