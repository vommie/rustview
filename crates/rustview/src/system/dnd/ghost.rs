//! The drag ghost window shown while dragging files out of `RustView`.

use super::{ACTIVE_GHOST_CHILD, SystemDndHandler};
use std::path::Path;
use std::process::{Command, Stdio};

impl SystemDndHandler {
    /// Starts the standalone, click-through OS-wide floating ghost window styled with `RustView`'s theme palette and instant cursor tracking.
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
}
