//! The XDND drop on another window and the external drop paths.

use super::{DndExportMetrics, SystemDndHandler};
use crate::system::error::SystemHandlerError;
use rustview_core::settings::LogObscurer;
use rustview_core::util::cast::Az;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Instant;

impl SystemDndHandler {
    /// Asynchronously performs the external Drop transaction into the 3rd-party window under the cursor.
    pub fn execute_external_drop_async(paths: &[PathBuf]) {
        if paths.is_empty() {
            return;
        }

        let paths_clone = paths.to_vec();
        std::thread::Builder::new()
            .name("sys-external-drop".into())
            .spawn(move || {
                let _ = Self::execute_external_drop_sync(&paths_clone);
            })
            .expect("Failed to spawn external drop worker thread");
    }

    /// Synchronously executes the XDND drop transaction on the target window under the global mouse cursor [2.4.1].
    pub fn execute_external_drop_sync(paths: &[PathBuf]) -> Result<DndExportMetrics, SystemHandlerError> {
        let t0 = Instant::now();
        let session_type = Self::detect_session_type();

        let mut canonical_paths = Vec::with_capacity(paths.len());
        let mut total_bytes = 0u64;

        for p in paths {
            if p.exists() {
                let cp = match p.canonicalize() {
                    Ok(c) => c,
                    Err(_) => p.clone(),
                };
                if let Ok(meta) = std::fs::metadata(&cp) {
                    total_bytes += meta.len();
                }
                canonical_paths.push(cp);
            }
        }

        if canonical_paths.is_empty() {
            return Err(SystemHandlerError::FileNotFound(
                paths.first().cloned().unwrap_or_default(),
            ));
        }

        let files_count = canonical_paths.len();

        tracing::info!(
            session_type = session_type,
            files_count = files_count,
            total_kb = format!("{:.2} KB", total_bytes.az::<f64>() / 1024.0),
            first_file = %LogObscurer::path(&canonical_paths[0]),
            "[SystemDndHandler: Drop] Executing external XDND drop transaction on window under cursor..."
        );

        let (success, target_window_id, target_app_class) = Self::perform_xdnd_drop_transaction(&canonical_paths);

        let elapsed_us = t0.elapsed().as_micros();
        let metrics = DndExportMetrics {
            files_count,
            total_bytes,
            session_type,
            target_window_id,
            target_app_class,
            elapsed_us,
            success,
        };

        tracing::info!(
            target_window_id = metrics.target_window_id,
            target_app = %metrics.target_app_class,
            success = metrics.success,
            elapsed_ms = metrics.elapsed_us.az::<f64>() / 1000.0,
            "[Benchmark: ExternalDrop] XDND Drop transaction completed"
        );

        Ok(metrics)
    }

    /// Performs the direct XDND Drop handshake with the target application window using Python ctypes and standard libX11 [2.1.1, 2.4.1].
    pub(super) fn perform_xdnd_drop_transaction(paths: &[PathBuf]) -> (bool, u64, String) {
        let python_script = r#"
import sys, os, time, ctypes

paths = sys.argv[1:]
valid_paths = [os.path.abspath(p) for p in paths if os.path.exists(p)]
if not valid_paths:
    sys.exit(1)

def path_to_uri(p):
    out = []
    for b in p.encode('utf-8'):
        if (b >= 48 and b <= 57) or (b >= 65 and b <= 90) or (b >= 97 and b <= 122) or b in (45, 46, 95, 126, 47):
            out.append(chr(b))
        elif b == 32:
            out.append('%20')
        else:
            out.append(f'%{b:02X}')
    return 'file://' + ''.join(out)

uri_list_bytes = ('\r\n'.join(path_to_uri(p) for p in valid_paths) + '\r\n').encode('utf-8')
plain_text_bytes = ('\n'.join(valid_paths) + '\n').encode('utf-8')

try:
    x11 = ctypes.cdll.LoadLibrary("libX11.so.6")
except Exception:
    try:
        x11 = ctypes.cdll.LoadLibrary("libX11.so")
    except Exception:
        sys.exit(1)

c_char_p = ctypes.c_char_p
c_void_p = ctypes.c_void_p
c_int = ctypes.c_int
c_uint = ctypes.c_uint
c_long = ctypes.c_long
c_ulong = ctypes.c_ulong
c_char = ctypes.c_char

Display_p = c_void_p
Window = c_ulong
Atom = c_ulong
Time = c_ulong
Bool = c_int

x11.XOpenDisplay.argtypes = [c_char_p]
x11.XOpenDisplay.restype = Display_p
x11.XCloseDisplay.argtypes = [Display_p]
x11.XCloseDisplay.restype = c_int
x11.XDefaultRootWindow.argtypes = [Display_p]
x11.XDefaultRootWindow.restype = Window
x11.XCreateSimpleWindow.argtypes = [Display_p, Window, c_int, c_int, c_uint, c_uint, c_uint, c_ulong, c_ulong]
x11.XCreateSimpleWindow.restype = Window
x11.XDestroyWindow.argtypes = [Display_p, Window]
x11.XDestroyWindow.restype = c_int
x11.XInternAtom.argtypes = [Display_p, c_char_p, Bool]
x11.XInternAtom.restype = Atom
x11.XChangeProperty.argtypes = [Display_p, Window, Atom, Atom, c_int, c_int, c_void_p, c_int]
x11.XChangeProperty.restype = c_int
x11.XGetWindowProperty.argtypes = [
    Display_p, Window, Atom, c_long, c_long, Bool, Atom,
    ctypes.POINTER(Atom), ctypes.POINTER(c_int), ctypes.POINTER(c_ulong),
    ctypes.POINTER(c_ulong), ctypes.POINTER(ctypes.POINTER(ctypes.c_ubyte))
]
x11.XGetWindowProperty.restype = c_int
x11.XFree.argtypes = [c_void_p]
x11.XFree.restype = c_int
x11.XSetSelectionOwner.argtypes = [Display_p, Atom, Window, Time]
x11.XSetSelectionOwner.restype = c_int
x11.XQueryPointer.argtypes = [
    Display_p, Window, ctypes.POINTER(Window), ctypes.POINTER(Window),
    ctypes.POINTER(c_int), ctypes.POINTER(c_int), ctypes.POINTER(c_int),
    ctypes.POINTER(c_int), ctypes.POINTER(c_uint)
]
x11.XQueryPointer.restype = Bool
x11.XQueryTree.argtypes = [
    Display_p, Window, ctypes.POINTER(Window), ctypes.POINTER(Window),
    ctypes.POINTER(ctypes.POINTER(Window)), ctypes.POINTER(c_uint)
]
x11.XQueryTree.restype = c_int
x11.XTranslateCoordinates.argtypes = [
    Display_p, Window, Window, c_int, c_int,
    ctypes.POINTER(c_int), ctypes.POINTER(c_int), ctypes.POINTER(Window)
]
x11.XTranslateCoordinates.restype = Bool
x11.XSendEvent.argtypes = [Display_p, Window, Bool, c_long, c_void_p]
x11.XSendEvent.restype = c_int
x11.XFlush.argtypes = [Display_p]
x11.XFlush.restype = c_int
x11.XPending.argtypes = [Display_p]
x11.XPending.restype = c_int
x11.XNextEvent.argtypes = [Display_p, c_void_p]
x11.XNextEvent.restype = c_int

class XClientMessageData(ctypes.Union):
    _fields_ = [("b", c_char * 20), ("s", ctypes.c_short * 10), ("l", c_long * 5)]

class XClientMessageEvent(ctypes.Structure):
    _fields_ = [
        ("type", c_int), ("serial", c_ulong), ("send_event", Bool),
        ("display", Display_p), ("window", Window), ("message_type", Atom),
        ("format", c_int), ("data", XClientMessageData)
    ]

class XSelectionRequestEvent(ctypes.Structure):
    _fields_ = [
        ("type", c_int), ("serial", c_ulong), ("send_event", Bool),
        ("display", Display_p), ("owner", Window), ("requestor", Window),
        ("selection", Atom), ("target", Atom), ("property", Atom), ("time", Time)
    ]

class XSelectionEvent(ctypes.Structure):
    _fields_ = [
        ("type", c_int), ("serial", c_ulong), ("send_event", Bool),
        ("display", Display_p), ("requestor", Window), ("selection", Atom),
        ("target", Atom), ("property", Atom), ("time", Time)
    ]

class XEvent(ctypes.Union):
    _fields_ = [
        ("type", c_int),
        ("xclient", XClientMessageEvent),
        ("xselectionrequest", XSelectionRequestEvent),
        ("xselection", XSelectionEvent),
        ("pad", c_long * 24),
    ]

ClientMessage = 33
SelectionRequest = 30
SelectionNotify = 31
PropModeReplace = 0
CurrentTime = 0
XA_ATOM = 4

dpy = x11.XOpenDisplay(None)
if not dpy:
    sys.exit(1)

root = x11.XDefaultRootWindow(dpy)

XdndAware = x11.XInternAtom(dpy, b"XdndAware", False)
XdndProxy = x11.XInternAtom(dpy, b"XdndProxy", False)
XdndSelection = x11.XInternAtom(dpy, b"XdndSelection", False)
XdndEnter = x11.XInternAtom(dpy, b"XdndEnter", False)
XdndPosition = x11.XInternAtom(dpy, b"XdndPosition", False)
XdndStatus = x11.XInternAtom(dpy, b"XdndStatus", False)
XdndDrop = x11.XInternAtom(dpy, b"XdndDrop", False)
XdndFinished = x11.XInternAtom(dpy, b"XdndFinished", False)
XdndActionCopy = x11.XInternAtom(dpy, b"XdndActionCopy", False)
XdndTypeList = x11.XInternAtom(dpy, b"XdndTypeList", False)
WM_CLASS = x11.XInternAtom(dpy, b"WM_CLASS", False)

text_uri_list = x11.XInternAtom(dpy, b"text/uri-list", False)
text_plain = x11.XInternAtom(dpy, b"text/plain", False)
text_plain_utf8 = x11.XInternAtom(dpy, b"text/plain;charset=utf-8", False)
utf8_string = x11.XInternAtom(dpy, b"UTF8_STRING", False)
targets_atom = x11.XInternAtom(dpy, b"TARGETS", False)

root_ret = Window()
child_ret = Window()
rx = c_int()
ry = c_int()
wx = c_int()
wy = c_int()
mask = c_uint()
x11.XQueryPointer(dpy, root, ctypes.byref(root_ret), ctypes.byref(child_ret), ctypes.byref(rx), ctypes.byref(ry), ctypes.byref(wx), ctypes.byref(wy), ctypes.byref(mask))

def check_dnd_aware(w):
    if w == 0 or w == root:
        return False
    actual_type = Atom()
    actual_format = c_int()
    nitems = c_ulong()
    bytes_after = c_ulong()
    prop_data = ctypes.POINTER(ctypes.c_ubyte)()
    res = x11.XGetWindowProperty(
        dpy, w, XdndAware, 0, 1, False, XA_ATOM,
        ctypes.byref(actual_type), ctypes.byref(actual_format),
        ctypes.byref(nitems), ctypes.byref(bytes_after), ctypes.byref(prop_data)
    )
    if res == 0 and prop_data and nitems.value > 0:
        x11.XFree(prop_data)
        return True
    if prop_data:
        x11.XFree(prop_data)
    return False

def get_dnd_proxy(w):
    actual_type = Atom()
    actual_format = c_int()
    nitems = c_ulong()
    bytes_after = c_ulong()
    prop_data = ctypes.POINTER(ctypes.c_ubyte)()
    res = x11.XGetWindowProperty(
        dpy, w, XdndProxy, 0, 1, False, XA_ATOM,
        ctypes.byref(actual_type), ctypes.byref(actual_format),
        ctypes.byref(nitems), ctypes.byref(bytes_after), ctypes.byref(prop_data)
    )
    if res == 0 and prop_data and nitems.value > 0:
        proxy_val = ctypes.cast(prop_data, ctypes.POINTER(Window))[0]
        x11.XFree(prop_data)
        return proxy_val
    if prop_data:
        x11.XFree(prop_data)
    return 0

def find_dnd_target(w):
    if w == 0 or w == root:
        return 0
    if check_dnd_aware(w):
        proxy = get_dnd_proxy(w)
        return proxy if proxy else w

    root_p = Window()
    parent_p = Window()
    children_p = ctypes.POINTER(Window)()
    nchildren = c_uint()

    if x11.XQueryTree(dpy, w, ctypes.byref(root_p), ctypes.byref(parent_p), ctypes.byref(children_p), ctypes.byref(nchildren)) != 0:
        if children_p and nchildren.value > 0:
            for i in range(nchildren.value - 1, -1, -1):
                cw = children_p[i]
                res = find_dnd_target(cw)
                if res != 0:
                    x11.XFree(children_p)
                    return res
            x11.XFree(children_p)
    return 0

target_win = find_dnd_target(child_ret.value)

app_class = "unknown"
if target_win != 0 or child_ret.value != 0:
    tw = target_win if target_win != 0 else child_ret.value
    actual_type = Atom()
    actual_format = c_int()
    nitems = c_ulong()
    bytes_after = c_ulong()
    prop_data = ctypes.POINTER(ctypes.c_ubyte)()
    res = x11.XGetWindowProperty(
        dpy, tw, WM_CLASS, 0, 100, False, x11.XInternAtom(dpy, b"STRING", False),
        ctypes.byref(actual_type), ctypes.byref(actual_format),
        ctypes.byref(nitems), ctypes.byref(bytes_after), ctypes.byref(prop_data)
    )
    if res == 0 and prop_data and nitems.value > 0:
        raw_s = bytes(ctypes.string_at(prop_data, nitems.value))
        parts = [p.decode('latin1') for p in raw_s.split(b'\x00') if p]
        if parts:
            app_class = parts[-1]
        x11.XFree(prop_data)

if target_win == 0:
    print(f"RESULT:NO_TARGET:0:{app_class}")
    x11.XCloseDisplay(dpy)
    sys.exit(0)

# Create lightweight source window to execute XDND handshake [2.4.1]
src_win = x11.XCreateSimpleWindow(dpy, root, 0, 0, 1, 1, 0, 0, 0)
version = (c_long * 1)(5)
x11.XChangeProperty(dpy, src_win, XdndAware, XA_ATOM, 32, PropModeReplace, ctypes.cast(version, c_void_p), 1)

type_list = (Atom * 4)(text_uri_list, text_plain, text_plain_utf8, utf8_string)
x11.XChangeProperty(dpy, src_win, XdndTypeList, XA_ATOM, 32, PropModeReplace, ctypes.cast(type_list, c_void_p), 4)

x11.XSetSelectionOwner(dpy, XdndSelection, src_win, CurrentTime)
x11.XFlush(dpy)

# 1. Send XdndEnter
ev_enter = XEvent()
ev_enter.type = ClientMessage
ev_enter.xclient.window = target_win
ev_enter.xclient.message_type = XdndEnter
ev_enter.xclient.format = 32
ev_enter.xclient.data.l[0] = src_win
ev_enter.xclient.data.l[1] = (5 << 24) | 1
ev_enter.xclient.data.l[2] = text_uri_list
ev_enter.xclient.data.l[3] = text_plain
ev_enter.xclient.data.l[4] = utf8_string
x11.XSendEvent(dpy, target_win, False, 0, ctypes.byref(ev_enter))

# 2. Send XdndPosition
ev_pos = XEvent()
ev_pos.type = ClientMessage
ev_pos.xclient.window = target_win
ev_pos.xclient.message_type = XdndPosition
ev_pos.xclient.format = 32
ev_pos.xclient.data.l[0] = src_win
ev_pos.xclient.data.l[1] = 0
ev_pos.xclient.data.l[2] = ((rx.value & 0xFFFF) << 16) | (ry.value & 0xFFFF)
ev_pos.xclient.data.l[3] = CurrentTime
ev_pos.xclient.data.l[4] = XdndActionCopy
x11.XSendEvent(dpy, target_win, False, 0, ctypes.byref(ev_pos))

# 3. Send XdndDrop [2.4.1]
ev_drop = XEvent()
ev_drop.type = ClientMessage
ev_drop.xclient.window = target_win
ev_drop.xclient.message_type = XdndDrop
ev_drop.xclient.format = 32
ev_drop.xclient.data.l[0] = src_win
ev_drop.xclient.data.l[1] = 0
ev_drop.xclient.data.l[2] = CurrentTime
x11.XSendEvent(dpy, target_win, False, 0, ctypes.byref(ev_drop))
x11.XFlush(dpy)

# 4. Respond to SelectionRequest from Target Window [2.4.5]
t_start = time.time()
drop_accepted = False

while time.time() - t_start < 4.0:
    if x11.XPending(dpy) > 0:
        ev = XEvent()
        x11.XNextEvent(dpy, ctypes.byref(ev))
        if ev.type == SelectionRequest:
            req = ev.xselectionrequest
            reply = XEvent()
            reply.type = SelectionNotify
            reply.xselection.serial = 0
            reply.xselection.send_event = 1
            reply.xselection.display = dpy
            reply.xselection.requestor = req.requestor
            reply.xselection.selection = req.selection
            reply.xselection.target = req.target
            reply.xselection.property = req.property
            reply.xselection.time = req.time

            if req.target == targets_atom:
                t_list = (Atom * 4)(targets_atom, text_uri_list, text_plain, utf8_string)
                x11.XChangeProperty(dpy, req.requestor, req.property, XA_ATOM, 32, PropModeReplace, ctypes.cast(t_list, c_void_p), 4)
            elif req.target == text_uri_list:
                x11.XChangeProperty(dpy, req.requestor, req.property, text_uri_list, 8, PropModeReplace, uri_list_bytes, len(uri_list_bytes))
                drop_accepted = True
            elif req.target in (text_plain, text_plain_utf8, utf8_string):
                x11.XChangeProperty(dpy, req.requestor, req.property, req.target, 8, PropModeReplace, plain_text_bytes, len(plain_text_bytes))
                drop_accepted = True
            else:
                reply.xselection.property = 0

            x11.XSendEvent(dpy, req.requestor, False, 0, ctypes.byref(reply))
            x11.XFlush(dpy)
        elif ev.type == ClientMessage and ev.xclient.message_type == XdndFinished:
            drop_accepted = True
            break
    time.sleep(0.01)

x11.XDestroyWindow(dpy, src_win)
x11.XCloseDisplay(dpy)

print(f"RESULT:{'SUCCESS' if drop_accepted else 'SENT'}:{target_win}:{app_class}")
"#;

        let output = Command::new("python3")
            .arg("-c")
            .arg(python_script)
            .args(paths)
            .stdin(Stdio::null())
            .output();

        if let Ok(out) = output {
            let res_str = String::from_utf8_lossy(&out.stdout);
            for line in res_str.lines() {
                if let Some(rest) = line.strip_prefix("RESULT:") {
                    let parts: Vec<&str> = rest.split(':').collect();
                    if parts.len() >= 3 {
                        let status = parts[0];
                        let win_id = parts[1].parse::<u64>().unwrap_or(0);
                        let app_class = parts[2].to_string();
                        let success = status == "SUCCESS" || status == "SENT";
                        return (success, win_id, app_class);
                    }
                }
            }
        }

        (false, 0, "unknown".to_string())
    }
}
