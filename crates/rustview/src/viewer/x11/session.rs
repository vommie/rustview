//! The connection to the X server (x11rb, a pure Rust implementation of the X11 protocol) and the small helpers every part shares.

use crate::viewer::ViewerError;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, ConnectionExt as _, Window};
use x11rb::rust_connection::RustConnection;

pub(super) fn x11_error(error: impl std::fmt::Display) -> ViewerError {
    ViewerError::X11(error.to_string())
}

/// Opens one connection to the X server that `DISPLAY` names and closes it again: whether an X11 program can run here (in a
/// Wayland session: whether `XWayland` answers).
pub fn x11_display_answers() -> Result<(), ViewerError> {
    Session::open("the window system check").map(drop)
}

/// A private connection to the X server and the root window of its default screen. Windows created on it vanish with it.
pub(super) struct Session {
    pub(super) conn: RustConnection,
    pub(super) root: Window,
    pub(super) screen: usize,
}

impl Session {
    pub(super) fn open(purpose: &str) -> Result<Self, ViewerError> {
        let (conn, screen) = RustConnection::connect(None)
            .map_err(|e| ViewerError::X11(format!("Failed to open the X11 display for {purpose}: {e}")))?;
        let root = conn
            .setup()
            .roots
            .get(screen)
            .map(|s| s.root)
            .ok_or_else(|| ViewerError::X11(format!("The X11 server has no screen {screen} ({purpose})")))?;
        Ok(Self { conn, root, screen })
    }

    /// The atom `name`; with `only_if_exists` `None` when no client ever used it.
    pub(super) fn atom(&self, name: &str, only_if_exists: bool) -> Option<Atom> {
        let atom = self
            .conn
            .intern_atom(only_if_exists, name.as_bytes())
            .ok()?
            .reply()
            .ok()?
            .atom;
        (atom != 0).then_some(atom)
    }

    /// Several atoms in one round trip (all requests go out before the first reply is read).
    pub(super) fn atoms<const N: usize>(&self, names: [&str; N]) -> Option<[Atom; N]> {
        let cookies = names
            .iter()
            .map(|name| self.conn.intern_atom(false, name.as_bytes()))
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        let mut atoms = [0; N];
        for (slot, cookie) in atoms.iter_mut().zip(cookies) {
            *slot = cookie.reply().ok()?.atom;
        }
        Some(atoms)
    }

    /// True while a compositing manager owns the `_NET_WM_CM_S<screen>` selection: only then are the alpha channels of windows blended
    /// with what is below them. Without one a window with an alpha channel shows its raw color bits.
    pub(super) fn compositor_running(&self) -> bool {
        let Some(atom) = self.atom(&format!("_NET_WM_CM_S{}", self.screen), true) else {
            return false;
        };
        self.conn
            .get_selection_owner(atom)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.owner != x11rb::NONE)
    }

    /// Sends the queued requests. Events and errors that arrived meanwhile are dropped: nothing here ever reads them, and they must
    /// not pile up for the whole life of the connection.
    pub(super) fn flush(&self) {
        let _ = self.conn.flush();
        self.drain_events();
    }

    /// Waits until the X server has processed everything sent so far.
    pub(super) fn sync(&self) {
        let _ = self.conn.get_input_focus().map(x11rb::cookie::Cookie::reply);
        self.drain_events();
    }

    /// Drops what the server sent unasked; protocol errors of requests that nobody waits for are logged.
    fn drain_events(&self) {
        while let Ok(Some(event)) = self.conn.poll_for_event() {
            if let x11rb::protocol::Event::Error(error) = event {
                tracing::debug!(?error, "[X11: Error] Ignored non-fatal X protocol error");
            }
        }
    }
}
