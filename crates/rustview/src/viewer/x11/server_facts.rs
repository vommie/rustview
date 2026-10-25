//! What the X server offers `RustView`, for the dependency checks of Settings > Debug and `rustview check`.

use super::session::Session;
use rustview_core::diagnostics::dependencies::X11Facts;
use x11rb::connection::RequestConnection;

/// The extensions the video windows and the bars need and whether a compositor runs; `None` without an X display.
pub fn x11_server_facts() -> Option<X11Facts> {
    let session = Session::open("the dependency check").ok()?;
    let present = |name: &'static str| session.conn.extension_information(name).ok().flatten().is_some();
    Some(X11Facts {
        shape: present(x11rb::protocol::shape::X11_EXTENSION_NAME),
        xfixes: present(x11rb::protocol::xfixes::X11_EXTENSION_NAME),
        compositor: session.compositor_running(),
    })
}
