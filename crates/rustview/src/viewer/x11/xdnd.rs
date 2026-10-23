//! The source side of the XDND protocol (drag and drop between X11 clients): hands files to the window under the pointer when the
//! user releases a drag outside of `RustView`. Slint and winit cannot do this, so it is spoken here directly with x11rb.
//!
//! The exchange: find the XDND aware window under the pointer, announce the drag (`XdndEnter`) and the position (`XdndPosition`),
//! send `XdndDrop`, then serve the selection requests of the target (`TARGETS`, `text/uri-list` and plain text) until it says
//! `XdndFinished`.

use super::session::{Session, x11_error};
use crate::viewer::ViewerError;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ClientMessageEvent, ConnectionExt as _, CreateWindowAux, EventMask, PropMode,
    SELECTION_NOTIFY_EVENT, SelectionNotifyEvent, SelectionRequestEvent, Window, WindowClass,
};
use x11rb::wrapper::ConnectionExt as _;

/// The protocol version this side speaks.
const XDND_VERSION: u32 = 5;
/// How long a drop may take until the target has fetched the data and answered.
const DROP_TIMEOUT: Duration = Duration::from_secs(4);
/// How long to wait for the target's answer to the position before the drop is sent anyway.
const STATUS_WAIT: Duration = Duration::from_millis(300);
/// The deepest window nesting searched for an XDND aware window.
const MAX_SEARCH_DEPTH: u32 = 16;
/// The depth value of `CreateWindow` that means "the depth of the parent".
const COPY_DEPTH_FROM_PARENT: u8 = 0;
/// `CurrentTime`: the server picks the time stamp.
const CURRENT_TIME: u32 = 0;

/// What an event of the target means for the drop.
#[derive(PartialEq, Eq)]
enum Progress {
    Nothing,
    /// The target fetched the data (`text/uri-list` or plain text).
    Fetched,
    /// The target sent `XdndFinished`.
    Finished,
}

/// What happened to a drop.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XdndOutcome {
    /// The window that received the drop; `None` when no XDND aware window was under the pointer.
    pub target_window: Option<u64>,
    /// The application class (`WM_CLASS`) of the window under the pointer, `unknown` when it has none.
    pub app_class: String,
    /// The target confirmed with `XdndFinished` (or fetched the data); without it the drop was sent and not contradicted.
    pub finished: bool,
}

struct Atoms {
    aware: Atom,
    proxy: Atom,
    selection: Atom,
    enter: Atom,
    position: Atom,
    status: Atom,
    drop: Atom,
    finished: Atom,
    action_copy: Atom,
    type_list: Atom,
    wm_class: Atom,
    uri_list: Atom,
    plain: Atom,
    plain_utf8: Atom,
    utf8_string: Atom,
    targets: Atom,
}

impl Atoms {
    fn intern(session: &Session) -> Option<Self> {
        let [
            aware,
            proxy,
            selection,
            enter,
            position,
            status,
            drop,
            finished,
            action_copy,
            type_list,
            wm_class,
            uri_list,
            plain,
            plain_utf8,
            utf8_string,
            targets,
        ] = session.atoms([
            "XdndAware",
            "XdndProxy",
            "XdndSelection",
            "XdndEnter",
            "XdndPosition",
            "XdndStatus",
            "XdndDrop",
            "XdndFinished",
            "XdndActionCopy",
            "XdndTypeList",
            "WM_CLASS",
            "text/uri-list",
            "text/plain",
            "text/plain;charset=utf-8",
            "UTF8_STRING",
            "TARGETS",
        ])?;
        Some(Self {
            aware,
            proxy,
            selection,
            enter,
            position,
            status,
            drop,
            finished,
            action_copy,
            type_list,
            wm_class,
            uri_list,
            plain,
            plain_utf8,
            utf8_string,
            targets,
        })
    }
}

/// Drops the data on the XDND aware window under the pointer: `uri_list` is the `text/uri-list` body (CRLF separated URIs),
/// `plain_text` the same files as paths for targets that only take text.
pub fn drop_on_window_under_pointer(uri_list: &[u8], plain_text: &[u8]) -> Result<XdndOutcome, ViewerError> {
    let session = Session::open("the XDND drop")?;
    let atoms = Atoms::intern(&session).ok_or_else(|| ViewerError::X11("cannot intern the XDND atoms".to_owned()))?;
    let pointer = session
        .conn
        .query_pointer(session.root)
        .map_err(x11_error)?
        .reply()
        .map_err(x11_error)?;
    drop_on(
        &session,
        &atoms,
        pointer.child,
        (pointer.root_x, pointer.root_y),
        uri_list,
        plain_text,
    )
}

/// The drop on `under` (a window, or the first XDND aware window inside it) at the root coordinates `position`.
fn drop_on(
    session: &Session,
    atoms: &Atoms,
    under: Window,
    position: (i16, i16),
    uri_list: &[u8],
    plain_text: &[u8],
) -> Result<XdndOutcome, ViewerError> {
    let conn = &session.conn;
    let under_pointer = under;
    let target = find_target(session, atoms, under_pointer, 0);
    let app_class = [target, Some(under_pointer).filter(|window| *window != x11rb::NONE)]
        .into_iter()
        .flatten()
        .find_map(|window| window_class(session, atoms, window))
        .unwrap_or_else(|| "unknown".to_owned());
    let Some(target) = target else {
        return Ok(XdndOutcome {
            target_window: None,
            app_class,
            finished: false,
        });
    };

    // A 1x1 window that is the source of the drag: it owns the selection and receives the answers of the target.
    let source = conn.generate_id().map_err(x11_error)?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        source,
        session.root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )
    .map_err(x11_error)?;
    let result = run_drop(session, atoms, source, target, position, uri_list, plain_text);
    let _ = conn.destroy_window(source);
    session.sync();
    result.map(|finished| XdndOutcome {
        target_window: Some(u64::from(target)),
        app_class,
        finished,
    })
}

fn run_drop(
    session: &Session,
    atoms: &Atoms,
    source: Window,
    target: Window,
    position: (i16, i16),
    uri_list: &[u8],
    plain_text: &[u8],
) -> Result<bool, ViewerError> {
    let conn = &session.conn;
    conn.change_property32(PropMode::REPLACE, source, atoms.aware, AtomEnum::ATOM, &[XDND_VERSION])
        .map_err(x11_error)?;
    let types = [atoms.uri_list, atoms.plain, atoms.plain_utf8, atoms.utf8_string];
    conn.change_property32(PropMode::REPLACE, source, atoms.type_list, AtomEnum::ATOM, &types)
        .map_err(x11_error)?;
    conn.set_selection_owner(source, atoms.selection, CURRENT_TIME)
        .map_err(x11_error)?;

    let send = |message_type: Atom, data: [u32; 5]| -> Result<(), ViewerError> {
        let event = ClientMessageEvent::new(32, target, message_type, data);
        conn.send_event(false, target, EventMask::NO_EVENT, event)
            .map_err(x11_error)?;
        Ok(())
    };
    // The packed pointer position of XdndPosition: x in the high, y in the low 16 bits (root coordinates).
    let packed_position = (u32::from(position.0.cast_unsigned()) << 16) | u32::from(position.1.cast_unsigned());

    send(
        atoms.enter,
        [
            source,
            (XDND_VERSION << 24) | 1,
            atoms.uri_list,
            atoms.plain,
            atoms.utf8_string,
        ],
    )?;
    send(
        atoms.position,
        [source, 0, packed_position, CURRENT_TIME, atoms.action_copy],
    )?;
    session.flush();

    // Targets answer the position with XdndStatus; the drop waits for it for a moment, but is sent in any case.
    let status_deadline = Instant::now() + STATUS_WAIT;
    let mut fetched = false;
    'status: while Instant::now() < status_deadline {
        while let Some(event) = conn.poll_for_event().map_err(x11_error)? {
            if let Event::ClientMessage(message) = &event
                && message.type_ == atoms.status
            {
                break 'status;
            }
            fetched |= serve_event(session, atoms, uri_list, plain_text, &event) == Progress::Fetched;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    send(atoms.drop, [source, 0, CURRENT_TIME, 0, 0])?;
    session.flush();

    // The target fetches the data with selection requests and ends with XdndFinished; give it time for both.
    let deadline = Instant::now() + DROP_TIMEOUT;
    'drop: while Instant::now() < deadline {
        while let Some(event) = conn.poll_for_event().map_err(x11_error)? {
            match serve_event(session, atoms, uri_list, plain_text, &event) {
                Progress::Finished => {
                    fetched = true;
                    break 'drop;
                }
                Progress::Fetched => fetched = true,
                Progress::Nothing => {}
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(fetched)
}

/// Answers a selection request of the target with the data and reports what the event means for the drop.
fn serve_event(session: &Session, atoms: &Atoms, uri_list: &[u8], plain_text: &[u8], event: &Event) -> Progress {
    match event {
        Event::ClientMessage(message) if message.type_ == atoms.finished => Progress::Finished,
        Event::SelectionRequest(request) => {
            if serve_request(session, atoms, uri_list, plain_text, request) {
                Progress::Fetched
            } else {
                Progress::Nothing
            }
        }
        _ => Progress::Nothing,
    }
}

fn serve_request(
    session: &Session,
    atoms: &Atoms,
    uri_list: &[u8],
    plain_text: &[u8],
    request: &SelectionRequestEvent,
) -> bool {
    let conn = &session.conn;
    let mut property = request.property;
    let mut fetched = false;
    if request.target == atoms.targets {
        let targets = [atoms.targets, atoms.uri_list, atoms.plain, atoms.utf8_string];
        let _ = conn.change_property32(
            PropMode::REPLACE,
            request.requestor,
            request.property,
            AtomEnum::ATOM,
            &targets,
        );
    } else if request.target == atoms.uri_list {
        let _ = conn.change_property8(
            PropMode::REPLACE,
            request.requestor,
            request.property,
            request.target,
            uri_list,
        );
        fetched = true;
    } else if [atoms.plain, atoms.plain_utf8, atoms.utf8_string].contains(&request.target) {
        let _ = conn.change_property8(
            PropMode::REPLACE,
            request.requestor,
            request.property,
            request.target,
            plain_text,
        );
        fetched = true;
    } else {
        property = x11rb::NONE;
    }
    let reply = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: 0,
        time: request.time,
        requestor: request.requestor,
        selection: request.selection,
        target: request.target,
        property,
    };
    let _ = conn.send_event(false, request.requestor, EventMask::NO_EVENT, reply);
    session.flush();
    fetched
}

/// The first XDND aware window at or below `window` (searching the children from the topmost), or its `XdndProxy`.
fn find_target(session: &Session, atoms: &Atoms, window: Window, depth: u32) -> Option<Window> {
    if window == x11rb::NONE || window == session.root || depth > MAX_SEARCH_DEPTH {
        return None;
    }
    if is_aware(session, atoms, window) {
        return Some(proxy_of(session, atoms, window).unwrap_or(window));
    }
    let tree = session.conn.query_tree(window).ok()?.reply().ok()?;
    tree.children
        .iter()
        .rev()
        .find_map(|child| find_target(session, atoms, *child, depth + 1))
}

fn is_aware(session: &Session, atoms: &Atoms, window: Window) -> bool {
    session
        .conn
        .get_property(false, window, atoms.aware, AtomEnum::ATOM, 0, 1)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.value_len > 0)
}

fn proxy_of(session: &Session, atoms: &Atoms, window: Window) -> Option<Window> {
    let reply = session
        .conn
        .get_property(false, window, atoms.proxy, AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next().filter(|proxy| *proxy != x11rb::NONE)
}

/// The class part of `WM_CLASS` (its last string).
fn window_class(session: &Session, atoms: &Atoms, window: Window) -> Option<String> {
    let reply = session
        .conn
        .get_property(false, window, atoms.wm_class, AtomEnum::STRING, 0, 100)
        .ok()?
        .reply()
        .ok()?;
    reply
        .value
        .split(|byte| *byte == 0)
        .rfind(|part| !part.is_empty())
        .map(|part| part.iter().map(|byte| char::from(*byte)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use x11rb::protocol::xproto::EventMask as Mask;

    const URI_LIST: &[u8] = b"file:///tmp/a%20b.jpg\r\nfile:///tmp/c.jpg\r\n";

    /// A minimal XDND target on its own connection: answers `XdndPosition` with `XdndStatus`, fetches `text/uri-list` after `XdndDrop`
    /// and ends with `XdndFinished`. Returns what it received.
    fn spawn_target(window_out: std::sync::mpsc::Sender<Window>) -> std::thread::JoinHandle<Option<Vec<u8>>> {
        std::thread::spawn(move || {
            let session = Session::open("the XDND test target").ok()?;
            let atoms = Atoms::intern(&session)?;
            let conn = &session.conn;
            let window = conn.generate_id().ok()?;
            conn.create_window(
                COPY_DEPTH_FROM_PARENT,
                window,
                session.root,
                0,
                0,
                1,
                1,
                0,
                WindowClass::INPUT_OUTPUT,
                x11rb::COPY_FROM_PARENT,
                &CreateWindowAux::new(),
            )
            .ok()?;
            conn.change_property32(PropMode::REPLACE, window, atoms.aware, AtomEnum::ATOM, &[XDND_VERSION])
                .ok()?;
            let property = session.atom("RUSTVIEW_XDND_TEST", false)?;
            session.sync();
            window_out.send(window).ok()?;
            let mut source = 0;
            let deadline = Instant::now() + Duration::from_secs(10);
            let mut received = None;
            while Instant::now() < deadline {
                let Some(event) = conn.poll_for_event().ok()? else {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                };
                match event {
                    Event::ClientMessage(message) if message.type_ == atoms.position => {
                        source = message.data.as_data32()[0];
                        let reply =
                            ClientMessageEvent::new(32, source, atoms.status, [window, 1, 0, 0, atoms.action_copy]);
                        conn.send_event(false, source, Mask::NO_EVENT, reply).ok()?;
                        session.flush();
                    }
                    Event::ClientMessage(message) if message.type_ == atoms.drop => {
                        conn.convert_selection(window, atoms.selection, atoms.uri_list, property, CURRENT_TIME)
                            .ok()?;
                        session.flush();
                    }
                    Event::SelectionNotify(notify) if notify.property != x11rb::NONE => {
                        let reply = conn
                            .get_property(true, window, property, AtomEnum::ANY, 0, 1024)
                            .ok()?
                            .reply()
                            .ok()?;
                        received = Some(reply.value);
                        let finished =
                            ClientMessageEvent::new(32, source, atoms.finished, [window, 1, atoms.action_copy, 0, 0]);
                        conn.send_event(false, source, Mask::NO_EVENT, finished).ok()?;
                        session.flush();
                        break;
                    }
                    _ => {}
                }
            }
            let _ = conn.destroy_window(window);
            session.sync();
            received
        })
    }

    #[test]
    fn a_target_receives_the_uri_list_and_confirms_the_drop() {
        let (window_tx, window_rx) = std::sync::mpsc::channel();
        let target = spawn_target(window_tx);
        let Ok(window) = window_rx.recv_timeout(Duration::from_secs(5)) else {
            // No X display (or no XDND atoms): nothing to test here.
            assert!(target.join().is_ok());
            return;
        };
        let session = Session::open("the XDND test source").expect("the target connected, so the display exists");
        let atoms = Atoms::intern(&session).expect("atoms");
        let outcome = drop_on(
            &session,
            &atoms,
            window,
            (10, 10),
            URI_LIST,
            b"/tmp/a b.jpg\n/tmp/c.jpg\n",
        )
        .expect("a drop");
        assert_eq!(outcome.target_window, Some(u64::from(window)));
        assert!(outcome.finished, "the target should confirm the drop");
        assert_eq!(target.join().expect("target thread"), Some(URI_LIST.to_vec()));
    }

    #[test]
    fn a_window_without_xdnd_support_is_no_target() {
        let Ok(session) = Session::open("the XDND test") else {
            return;
        };
        let atoms = Atoms::intern(&session).expect("atoms");
        let outcome = drop_on(&session, &atoms, session.root, (0, 0), URI_LIST, b"").expect("a result");
        assert_eq!(outcome.target_window, None);
        assert!(!outcome.finished);
    }
}
