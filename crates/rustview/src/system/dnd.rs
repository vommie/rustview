use super::error::SystemHandlerError;
use crate::icons::Icon;
use rustview_core::settings::LogObscurer;
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;
use std::time::Instant;

/// Edge length of the ghost window's thumbnail/icon slot (matches the scaling in the helper script below).
const GHOST_ICON_SIZE_PX: u32 = 24;
/// Color of the ghost fallback icon; equals `Theme.text` in `ui/styles.slint` (the helper window cannot read the theme).
const GHOST_ICON_COLOR: &str = "#cdd6f4";

static ACTIVE_GHOST_CHILD: LazyLock<Mutex<Option<Child>>> = LazyLock::new(|| Mutex::new(None));

/// Detailed telemetry metrics for external Drag & Drop export sessions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DndExportMetrics {
    pub files_count: usize,
    pub total_bytes: u64,
    pub session_type: &'static str,
    pub target_window_id: u64,
    pub target_app_class: String,
    pub elapsed_us: u128,
    pub success: bool,
}

/// Centralized Linux Drag and Drop export subsystem (supporting modern Wayland and X11 protocols).
/// Handles seamless dragging and dropping of files into 3rd-party applications
/// (web browsers, media players, chat apps, image editors, and file managers) via direct XDND transactions.
pub struct SystemDndHandler;

impl SystemDndHandler {
    /// Detects whether the current Linux session is running on Wayland or X11.
    #[inline]
    pub fn detect_session_type() -> &'static str {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE")
                .map(|v| v.eq_ignore_ascii_case("wayland"))
                .unwrap_or(false)
        {
            "Wayland"
        } else {
            "X11"
        }
    }

    /// Converts a local filesystem path into an RFC 2483 compliant `file://` URI string.
    pub fn path_to_file_uri(path: &Path) -> String {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        };

        url::Url::from_file_path(&abs_path).map_or_else(|()| format!("file://{}", abs_path.to_string_lossy()), String::from)
    }

    /// Formats a slice of filesystem paths into a standard CRLF-delimited `text/uri-list` payload.
    pub fn format_uri_list<P: AsRef<Path>>(paths: &[P]) -> String {
        let mut out = String::new();
        for p in paths {
            let uri = Self::path_to_file_uri(p.as_ref());
            out.push_str(&uri);
            out.push_str("\r\n");
        }
        out
    }

    /// Formats a slice of filesystem paths into a newline-delimited plain path list.
    pub fn format_plain_paths<P: AsRef<Path>>(paths: &[P]) -> String {
        paths
            .iter()
            .map(|p| p.as_ref().to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Renders (once) and returns the generic file icon PNG used by the ghost window when no thumbnail exists.
    fn fallback_icon_path() -> Option<PathBuf> {
        let dest = std::env::temp_dir()
            .join("rustview")
            .join("icons")
            .join(format!("ghost-file-{GHOST_ICON_SIZE_PX}.png"));
        if !dest.is_file()
            && let Err(e) = crate::icons::raster::render_png(
                Icon::File,
                GHOST_ICON_SIZE_PX,
                GHOST_ICON_COLOR,
                &dest,
            )
        {
            tracing::warn!(error = %e, "[SystemDnd] Cannot render ghost fallback icon");
            return None;
        }
        Some(dest)
    }

    /// Starts the standalone, click-through OS-wide floating ghost window styled with RustView's theme palette and instant cursor tracking.
    pub fn start_ghost_overlay(title: &str, count: usize, thumb_path: Option<&Path>) {
        Self::cancel_ghost_overlay();

        let title_owned = title.to_string();
        // Without a real thumbnail the ghost shows the generic file icon (rendered from the icon library),
        // so the native helper window never needs an emoji or font glyph as icon.
        let thumb_arg = thumb_path
            .map(Path::to_path_buf)
            .or_else(Self::fallback_icon_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        std::thread::Builder::new()
            .name("sys-dnd-ghost".into())
            .spawn(move || {
                let python_script = r#"
import sys, os, time

title = sys.argv[1] if len(sys.argv) > 1 else "Files"
count = int(sys.argv[2]) if len(sys.argv) > 2 else 1
thumb_path = sys.argv[3] if len(sys.argv) > 3 and os.path.exists(sys.argv[3]) else ""

badge_text = f"{count} Files" if count > 1 else "1 File"

# Tier 1: GTK 3 with RGBA Visual, Cairo rounded-rectangle drawing, and GdkPixbuf
try:
    import gi
    gi.require_version('Gtk', '3.0')
    from gi.repository import Gtk, Gdk, GdkPixbuf, GLib, Pango, cairo

    class GhostWindow(Gtk.Window):
        def __init__(self):
            super().__init__(type=Gtk.WindowType.POPUP)
            self.set_app_paintable(True)
            self.set_decorated(False)
            self.set_keep_above(True)
            self.set_skip_taskbar_hint(True)
            self.set_skip_pager_hint(True)
            self.set_type_hint(Gdk.WindowTypeHint.DND)

            # Enable 32-bit RGBA visual for true anti-aliased rounded transparency
            screen = self.get_screen()
            visual = screen.get_rgba_visual()
            if visual and screen.is_composited():
                self.set_visual(visual)

            # RustView Catppuccin / Breeze Dark Theme Typography & Badge Styling
            css = Gtk.CssProvider()
            css.load_from_data(b"""
                window {
                    background-color: transparent;
                }
                label.title {
                    color: #cdd6f4;
                    font-size: 11px;
                    font-weight: bold;
                }
                label.badge {
                    background-color: #89b4fa;
                    color: #11111b;
                    font-size: 10px;
                    font-weight: bold;
                    border-radius: 4px;
                    padding: 2px 6px;
                }
                image.thumb {
                    border-radius: 3px;
                }
            """)
            Gtk.StyleContext.add_provider_for_screen(
                Gdk.Screen.get_default(),
                css,
                Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION
            )

            box = Gtk.Box(orientation=Gtk.Orientation.HORIZONTAL, spacing=8)
            box.set_margin_start(10)
            box.set_margin_end(12)
            box.set_margin_top(6)
            box.set_margin_bottom(6)

            # 1. Real Image Thumbnail or Document Icon
            pixbuf = None
            if thumb_path:
                try:
                    pixbuf = GdkPixbuf.Pixbuf.new_from_file_at_scale(thumb_path, 24, 24, True)
                except Exception:
                    pixbuf = None

            if pixbuf:
                img = Gtk.Image.new_from_pixbuf(pixbuf)
                img.get_style_context().add_class("thumb")
                box.pack_start(img, False, False, 0)

            # 2. File Title
            lbl = Gtk.Label(label=title)
            lbl.get_style_context().add_class("title")
            lbl.set_ellipsize(Pango.EllipsizeMode.END)
            lbl.set_max_width_chars(28)
            box.pack_start(lbl, True, True, 0)

            # 3. Count Badge
            if count > 1:
                badge = Gtk.Label(label=badge_text)
                badge.get_style_context().add_class("badge")
                box.pack_start(badge, False, False, 0)

            self.add(box)
            self.connect("draw", self.on_draw)
            self.connect("realize", self.on_realize)

            # CRUCIAL: Position window at exact cursor position BEFORE mapping/showing to avoid (0, 0) flash
            self.track_pointer()
            self.show_all()

            # 60 FPS smooth cursor tracking
            GLib.timeout_add(16, self.track_pointer)

        def on_draw(self, widget, cr):
            # Clear background with full alpha transparency
            cr.set_operator(cairo.Operator.CLEAR)
            cr.paint()
            cr.set_operator(cairo.Operator.OVER)

            w = float(self.get_allocated_width())
            h = float(self.get_allocated_height())
            r = 8.0 # 8px border radius
            deg = 3.141592653589793 / 180.0

            # Draw rounded rectangle path
            cr.new_sub_path()
            cr.arc(w - r - 1.0, r + 1.0, r, -90.0 * deg, 0.0 * deg)
            cr.arc(w - r - 1.0, h - r - 1.0, r, 0.0 * deg, 90.0 * deg)
            cr.arc(r + 1.0, h - r - 1.0, r, 90.0 * deg, 180.0 * deg)
            cr.arc(r + 1.0, r + 1.0, r, 180.0 * deg, 270.0 * deg)
            cr.close_path()

            # Fill Surface background (#1e1e2e at 96% opacity)
            cr.set_source_rgba(30.0 / 255.0, 30.0 / 255.0, 46.0 / 255.0, 0.96)
            cr.fill_preserve()

            # Stroke Primary border (#89b4fa)
            cr.set_source_rgba(137.0 / 255.0, 180.0 / 255.0, 250.0 / 255.0, 1.0)
            cr.set_line_width(1.5)
            cr.stroke()
            return False

        def on_realize(self, widget):
            window = self.get_window()
            if window:
                # Make window 100% click-through so it never intercepts drops underneath it
                empty_region = cairo.Region()
                window.input_shape_combine_region(empty_region, 0, 0)
            self.track_pointer()

        def track_pointer(self):
            display = Gdk.Display.get_default()
            if display:
                seat = display.get_default_seat()
                if seat:
                    pointer = seat.get_pointer()
                    if pointer:
                        screen, x, y = pointer.get_position()
                        self.move(x + 12, y + 12)
            return True

    win = GhostWindow()
    GLib.timeout_add_seconds(60, Gtk.main_quit)
    Gtk.main()
    sys.exit(0)
except Exception:
    pass

# Tier 2: PyQt6 / PySide6 / PyQt5 fallback
try:
    from PyQt6.QtWidgets import QApplication, QWidget, QLabel, QHBoxLayout
    from PyQt6.QtCore import Qt, QTimer, QPoint
    from PyQt6.QtGui import QPixmap, QColor, QFont, QCursor

    app = QApplication.instance() or QApplication(['rustview-ghost'])
    w = QWidget()
    w.setWindowFlags(Qt.WindowType.ToolTip | Qt.WindowType.FramelessWindowHint | Qt.WindowType.WindowStaysOnTopHint | Qt.WindowType.WindowTransparentForInput | Qt.WindowType.WindowDoesNotAcceptFocus)
    w.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground, True)
    w.setAttribute(Qt.WidgetAttribute.WA_TransparentForMouseEvents, True)
    w.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating, True)
    w.setStyleSheet("""
        QWidget#GhostBox {
            background-color: rgba(30, 30, 46, 245);
            border: 1.5px solid #89b4fa;
            border-radius: 8px;
        }
        QLabel {
            color: #cdd6f4;
            font-size: 11px;
            font-weight: bold;
        }
        QLabel#Badge {
            background-color: #89b4fa;
            color: #11111b;
            font-size: 10px;
            font-weight: bold;
            border-radius: 4px;
            padding: 2px 6px;
        }
    """)

    container = QWidget(w)
    container.setObjectName("GhostBox")
    layout = QHBoxLayout(container)
    layout.setContentsMargins(10, 6, 12, 6)
    layout.setSpacing(8)

    if thumb_path and os.path.exists(thumb_path):
        thumb_lbl = QLabel()
        pm = QPixmap(thumb_path).scaled(24, 24, Qt.AspectRatioMode.KeepAspectRatio, Qt.TransformationMode.SmoothTransformation)
        thumb_lbl.setPixmap(pm)
        layout.addWidget(thumb_lbl)

    title_lbl = QLabel(title)
    layout.addWidget(title_lbl)

    if count > 1:
        badge_lbl = QLabel(badge_text)
        badge_lbl.setObjectName("Badge")
        layout.addWidget(badge_lbl)

    main_layout = QHBoxLayout(w)
    main_layout.setContentsMargins(0, 0, 0, 0)
    main_layout.addWidget(container)

    def update_pos():
        pos = QCursor.pos()
        w.move(pos.x() + 12, pos.y() + 12)

    # Move to initial position BEFORE showing
    update_pos()
    w.show()

    timer = QTimer()
    timer.timeout.connect(update_pos)
    timer.start(16)

    QTimer.single_shot(60000, app.quit)
    app.exec()
    sys.exit(0)
except Exception:
    sys.exit(1)
"#;

                if let Ok(child) = Command::new("python3")
                    .arg("-c")
                    .arg(python_script)
                    .arg(&title_owned)
                    .arg(count.to_string())
                    .arg(&thumb_arg)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    *ACTIVE_GHOST_CHILD.lock() = Some(child);
                }
            })
            .ok();
    }

    /// Instantly terminates and dismisses the floating drag ghost overlay (0ms latency).
    pub fn cancel_ghost_overlay() {
        let mut guard = ACTIVE_GHOST_CHILD.lock();
        if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

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
    pub fn execute_external_drop_sync(
        paths: &[PathBuf],
    ) -> Result<DndExportMetrics, SystemHandlerError> {
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
            total_kb = format!("{:.2} KB", total_bytes as f64 / 1024.0),
            first_file = %LogObscurer::path(&canonical_paths[0]),
            "[SystemDndHandler: Drop] Executing external XDND drop transaction on window under cursor..."
        );

        let (success, target_window_id, target_app_class) =
            Self::perform_xdnd_drop_transaction(&canonical_paths);

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
            elapsed_ms = metrics.elapsed_us as f64 / 1000.0,
            "[Benchmark: ExternalDrop] XDND Drop transaction completed"
        );

        Ok(metrics)
    }

    /// Performs the direct XDND Drop handshake with the target application window using Python ctypes and standard libX11 [2.1.1, 2.4.1].
    fn perform_xdnd_drop_transaction(paths: &[PathBuf]) -> (bool, u64, String) {
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

#[cfg(test)]
mod tests {
    use super::SystemDndHandler;
    use std::path::Path;

    #[test]
    fn file_uri_escapes_spaces_hashes_and_unicode() {
        assert_eq!(
            SystemDndHandler::path_to_file_uri(Path::new("/home/u/My Photos/a #1 \u{fc}.jpg")),
            "file:///home/u/My%20Photos/a%20%231%20%C3%BC.jpg"
        );
    }

    #[test]
    fn uri_list_is_crlf_terminated() {
        let list = SystemDndHandler::format_uri_list(&["/a/b.jpg", "/c d.png"]);
        assert_eq!(list, "file:///a/b.jpg\r\nfile:///c%20d.png\r\n");
    }
}
