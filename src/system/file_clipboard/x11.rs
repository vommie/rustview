//! The X11 side of the file clipboard: an owner thread that serves our files to other applications and a reader for what they offer.
//!
//! X11 has no clipboard storage: the application that copied something owns the `CLIPBOARD` selection and hands the data to whoever
//! asks (ICCCM, section 2). So to make Dolphin able to paste what RustView copied, RustView must stay the selection owner and answer
//! `SelectionRequest` events with the formats of `format.rs`; and to paste what Dolphin copied, RustView asks the owner for `TARGETS`
//! and then for the file formats. Large answers travel in `INCR` chunks, which the reader supports; the owner only serves what fits into
//! one request (a selection that does not is refused when it is set). On exit the data is handed to the clipboard manager
//! (`SAVE_TARGETS`) so that it survives the process, like every well-behaved X11 application does.
//!
//! The owner thread blocks in `wait_for_event` (no polling); the rest of the application wakes it with a client message.

use super::format::{self, ClipboardFiles, ClipboardMode, FilePayloads, MAX_PAYLOAD_BYTES};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{
    AtomEnum, ClientMessageEvent, ConnectionExt, CreateWindowAux, EventMask, PropMode, Property, SelectionNotifyEvent, SelectionRequestEvent,
    WindowClass, SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME, NONE};

x11rb::atom_manager! {
    Atoms: AtomsCookie {
        CLIPBOARD,
        TARGETS,
        INCR,
        UTF8_STRING,
        CLIPBOARD_MANAGER,
        SAVE_TARGETS,
        RV_CLIPBOARD_WAKE,
        RV_CLIPBOARD_DATA,
        TEXT_URI_LIST: b"text/uri-list",
        GNOME_COPIED_FILES: b"x-special/gnome-copied-files",
        KDE_CUT_SELECTION: b"application/x-kde-cutselection",
        TEXT_PLAIN: b"text/plain",
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
    }
}

/// How long a read waits for the owner of the clipboard to answer one request.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(1500);
/// How long the hand-over to the clipboard manager may take at most.
const HANDOVER_TIMEOUT: Duration = Duration::from_millis(500);
const POLL_PAUSE: Duration = Duration::from_millis(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardError(pub String);

impl std::fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! clipboard_error_from {
    ($($source:ty),* $(,)?) => {
        $(impl From<$source> for ClipboardError {
            fn from(e: $source) -> Self {
                Self(e.to_string())
            }
        })*
    };
}

clipboard_error_from!(
    x11rb::errors::ConnectError,
    x11rb::errors::ConnectionError,
    x11rb::errors::ReplyError,
    x11rb::errors::ReplyOrIdError,
    std::io::Error,
    &str,
    String,
);

type Result<T> = std::result::Result<T, ClipboardError>;

/// What we serve, ready to be written into a property.
struct OwnedContent {
    files: Vec<PathBuf>,
    mode: ClipboardMode,
    uri_list: Vec<u8>,
    gnome: Vec<u8>,
    kde_cut: Vec<u8>,
    text: Vec<u8>,
}

impl OwnedContent {
    fn new(files: &[PathBuf], mode: ClipboardMode) -> Self {
        Self {
            files: files.to_vec(),
            mode,
            uri_list: format::encode_uri_list(files).into_bytes(),
            gnome: format::encode_gnome_copied_files(files, mode).into_bytes(),
            kde_cut: format::encode_kde_cut_selection(mode).as_bytes().to_vec(),
            text: format::encode_plain_text(files),
        }
    }

    fn largest(&self) -> usize {
        self.uri_list.len().max(self.gnome.len()).max(self.text.len())
    }
}

enum Command {
    Set(Arc<OwnedContent>, Sender<Result<()>>),
    Clear(Sender<()>),
    Handover(Sender<()>),
}

/// The owner thread's connection and what it serves.
struct Owner {
    conn: Arc<RustConnection>,
    window: u32,
    atoms: Atoms,
    owned: Mutex<Option<Arc<OwnedContent>>>,
    commands: Mutex<Sender<Command>>,
    alive: AtomicBool,
}

static OWNER: Mutex<Option<Arc<Owner>>> = Mutex::new(None);

fn connect() -> Result<(Arc<RustConnection>, u32, Atoms)> {
    let (conn, screen) = RustConnection::connect(None)?;
    let root = conn.setup().roots.get(screen).ok_or("no X screen")?.root;
    let window = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        COPY_FROM_PARENT,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    let atoms = Atoms::new(&conn)?.reply()?;
    conn.flush()?;
    Ok((Arc::new(conn), window, atoms))
}

fn owner() -> Result<Arc<Owner>> {
    let mut slot = OWNER.lock();
    if let Some(existing) = slot.as_ref().filter(|o| o.alive.load(Ordering::Acquire)) {
        return Ok(existing.clone());
    }
    let (conn, window, atoms) = connect()?;
    let (tx, rx) = mpsc::channel();
    let owner = Arc::new(Owner { conn, window, atoms, owned: Mutex::new(None), commands: Mutex::new(tx), alive: AtomicBool::new(true) });
    let for_thread = owner.clone();
    std::thread::Builder::new()
        .name("file-clipboard".into())
        .spawn(move || {
            if let Err(e) = for_thread.run(rx) {
                tracing::warn!(error = %e, "[FileClipboard: Owner] The clipboard thread ended");
            }
            for_thread.alive.store(false, Ordering::Release);
            *for_thread.owned.lock() = None;
        })?;
    *slot = Some(owner.clone());
    Ok(owner)
}

impl Owner {
    fn send_command(&self, command: Command) -> Result<()> {
        self.commands.lock().send(command).map_err(|_| ClipboardError("the clipboard thread is gone".into()))?;
        let wake = ClientMessageEvent::new(32, self.window, self.atoms.RV_CLIPBOARD_WAKE, [0u32; 5]);
        self.conn.send_event(false, self.window, EventMask::NO_EVENT, wake)?;
        self.conn.flush()?;
        Ok(())
    }

    fn run(&self, commands: Receiver<Command>) -> Result<()> {
        loop {
            match self.conn.wait_for_event()? {
                Event::SelectionRequest(request) => self.answer(&request)?,
                Event::SelectionClear(clear) if clear.selection == self.atoms.CLIPBOARD && clear.owner == self.window => {
                    // Another application copied something: what we served is no longer on the clipboard.
                    *self.owned.lock() = None;
                }
                Event::ClientMessage(message) if message.type_ == self.atoms.RV_CLIPBOARD_WAKE => {
                    while let Ok(command) = commands.try_recv() {
                        self.execute(command)?;
                    }
                }
                _ => {}
            }
        }
    }

    fn execute(&self, command: Command) -> Result<()> {
        match command {
            Command::Set(content, reply) => {
                let result = self.take_ownership(content);
                let _ = reply.send(result);
            }
            Command::Clear(reply) => {
                if self.owned.lock().take().is_some() && self.is_selection_owner() {
                    self.conn.set_selection_owner(NONE, self.atoms.CLIPBOARD, CURRENT_TIME)?;
                    self.conn.flush()?;
                }
                let _ = reply.send(());
            }
            Command::Handover(reply) => {
                self.hand_over_to_manager();
                let _ = reply.send(());
            }
        }
        Ok(())
    }

    fn is_selection_owner(&self) -> bool {
        self.conn
            .get_selection_owner(self.atoms.CLIPBOARD)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.owner == self.window)
    }

    fn take_ownership(&self, content: Arc<OwnedContent>) -> Result<()> {
        let limit = self.conn.maximum_request_bytes().saturating_sub(1024).min(MAX_PAYLOAD_BYTES);
        if content.largest() > limit || content.files.len() > format::MAX_FILES {
            return Err(ClipboardError(format!(
                "the list of {} files is too long for the clipboard of this display",
                content.files.len()
            )));
        }
        *self.owned.lock() = Some(content);
        self.conn.set_selection_owner(self.window, self.atoms.CLIPBOARD, CURRENT_TIME)?;
        self.conn.flush()?;
        if !self.is_selection_owner() {
            *self.owned.lock() = None;
            return Err(ClipboardError("the clipboard could not be taken over".into()));
        }
        Ok(())
    }

    fn targets(&self) -> Vec<u32> {
        let a = &self.atoms;
        vec![
            a.TARGETS,
            a.TEXT_URI_LIST,
            a.GNOME_COPIED_FILES,
            a.KDE_CUT_SELECTION,
            a.TEXT_PLAIN,
            a.TEXT_PLAIN_UTF8,
            a.UTF8_STRING,
            AtomEnum::STRING.into(),
            // Tells the clipboard manager that the data may be handed over when we exit.
            a.SAVE_TARGETS,
        ]
    }

    /// Answers one request of another application for our data.
    fn answer(&self, request: &SelectionRequestEvent) -> Result<()> {
        let a = &self.atoms;
        let property = if request.property == NONE { request.target } else { request.property };
        let content = self.owned.lock().clone();
        let served = match (request.selection == a.CLIPBOARD, content) {
            (true, Some(content)) => self.write_target(request, property, &content)?,
            _ => false,
        };
        let notify = SelectionNotifyEvent {
            response_type: SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: request.time,
            requestor: request.requestor,
            selection: request.selection,
            target: request.target,
            property: if served { property } else { NONE },
        };
        self.conn.send_event(false, request.requestor, EventMask::NO_EVENT, notify)?;
        self.conn.flush()?;
        Ok(())
    }

    fn write_target(&self, request: &SelectionRequestEvent, property: u32, content: &OwnedContent) -> Result<bool> {
        let a = &self.atoms;
        let target = request.target;
        let (kind, data): (u32, &[u8]) = if target == a.TEXT_URI_LIST {
            (target, &content.uri_list)
        } else if target == a.GNOME_COPIED_FILES {
            (target, &content.gnome)
        } else if target == a.KDE_CUT_SELECTION {
            (target, &content.kde_cut)
        } else if target == a.TEXT_PLAIN || target == a.TEXT_PLAIN_UTF8 || target == a.UTF8_STRING || target == u32::from(AtomEnum::STRING) {
            (target, &content.text)
        } else if target == a.TARGETS {
            self.conn.change_property32(PropMode::REPLACE, request.requestor, property, AtomEnum::ATOM, &self.targets())?;
            return Ok(true);
        } else {
            return Ok(false);
        };
        if data.len() > self.conn.maximum_request_bytes().saturating_sub(1024) {
            return Ok(false);
        }
        self.conn.change_property8(PropMode::REPLACE, request.requestor, property, kind, data)?;
        Ok(true)
    }

    /// Asks the clipboard manager to take over our data (`SAVE_TARGETS`) while we can still serve it.
    fn hand_over_to_manager(&self) {
        let a = &self.atoms;
        if self.owned.lock().is_none() || !self.is_selection_owner() {
            return;
        }
        let has_manager = self
            .conn
            .get_selection_owner(a.CLIPBOARD_MANAGER)
            .ok()
            .and_then(|cookie| cookie.reply().ok())
            .is_some_and(|reply| reply.owner != NONE);
        if !has_manager {
            return;
        }
        if self.conn.convert_selection(self.window, a.CLIPBOARD_MANAGER, a.SAVE_TARGETS, a.RV_CLIPBOARD_DATA, CURRENT_TIME).is_err()
            || self.conn.flush().is_err()
        {
            return;
        }
        let deadline = Instant::now() + HANDOVER_TIMEOUT;
        // The manager asks for our data meanwhile; those requests are answered here.
        while Instant::now() < deadline {
            match self.conn.poll_for_event() {
                Ok(Some(Event::SelectionRequest(request))) => {
                    if self.answer(&request).is_err() {
                        return;
                    }
                }
                Ok(Some(Event::SelectionNotify(notify))) if notify.selection == a.CLIPBOARD_MANAGER => return,
                Ok(Some(_)) => {}
                Ok(None) => std::thread::sleep(POLL_PAUSE),
                Err(_) => return,
            }
        }
        tracing::debug!("[FileClipboard: Handover] The clipboard manager did not answer in time");
    }
}

/// Puts files on the clipboard and keeps serving them until another application copies something.
pub fn set(files: &[PathBuf], mode: ClipboardMode) -> Result<()> {
    let owner = owner()?;
    let (tx, rx) = mpsc::channel();
    owner.send_command(Command::Set(Arc::new(OwnedContent::new(files, mode)), tx))?;
    rx.recv_timeout(REQUEST_TIMEOUT).map_err(|_| ClipboardError("the clipboard did not answer".into()))?
}

/// Empties the clipboard if it still holds what we put there (after a cut was pasted); never touches the clipboard of others.
pub fn clear_if_ours() {
    let Some(owner) = OWNER.lock().clone().filter(|o| o.alive.load(Ordering::Acquire)) else {
        return;
    };
    let (tx, rx) = mpsc::channel();
    if owner.send_command(Command::Clear(tx)).is_ok() {
        let _ = rx.recv_timeout(REQUEST_TIMEOUT);
    }
}

/// Hands the clipboard to the clipboard manager (on exit), so that it outlives the process.
pub fn hand_over_to_manager() {
    let Some(owner) = OWNER.lock().clone().filter(|o| o.alive.load(Ordering::Acquire)) else {
        return;
    };
    if owner.owned.lock().is_none() {
        return;
    }
    let (tx, rx) = mpsc::channel();
    if owner.send_command(Command::Handover(tx)).is_ok() {
        let _ = rx.recv_timeout(HANDOVER_TIMEOUT + Duration::from_millis(200));
    }
}

/// The files we put on the clipboard, while they are still there (no round trip to the server).
pub fn own_files() -> Option<(Vec<PathBuf>, ClipboardMode)> {
    let owner = OWNER.lock().clone().filter(|o| o.alive.load(Ordering::Acquire))?;
    let content = owner.owned.lock().clone()?;
    Some((content.files.clone(), content.mode))
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Reading what another application offers.
// ---------------------------------------------------------------------------------------------------------------------------------

struct Reader {
    conn: RustConnection,
    window: u32,
    atoms: Atoms,
}

/// Waits until `matches` accepts an event or `deadline` passes; other events are dropped (the window is ours alone).
fn wait_event(conn: &RustConnection, deadline: Instant, mut matches: impl FnMut(&Event) -> bool) -> Result<Option<Event>> {
    loop {
        match conn.poll_for_event()? {
            Some(event) if matches(&event) => return Ok(Some(event)),
            Some(_) => {}
            None if Instant::now() >= deadline => return Ok(None),
            None => std::thread::sleep(POLL_PAUSE),
        }
    }
}

impl Reader {
    fn new() -> Result<Self> {
        let (conn, screen) = RustConnection::connect(None)?;
        let root = conn.setup().roots.get(screen).ok_or("no X screen")?.root;
        let window = conn.generate_id()?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            COPY_FROM_PARENT,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )?;
        let atoms = Atoms::new(&conn)?.reply()?;
        conn.flush()?;
        Ok(Self { conn, window, atoms })
    }

    /// Asks the owner to convert the clipboard to `target`; the bytes of the answer, or `None` when it refuses or does not answer.
    fn fetch(&self, target: u32) -> Result<Option<(u32, Vec<u8>)>> {
        let a = &self.atoms;
        self.conn.delete_property(self.window, a.RV_CLIPBOARD_DATA)?;
        self.conn.convert_selection(self.window, a.CLIPBOARD, target, a.RV_CLIPBOARD_DATA, CURRENT_TIME)?;
        self.conn.flush()?;
        let deadline = Instant::now() + REQUEST_TIMEOUT;
        let Some(Event::SelectionNotify(notify)) =
            wait_event(&self.conn, deadline, |e| matches!(e, Event::SelectionNotify(n) if n.selection == a.CLIPBOARD && n.target == target))?
        else {
            return Ok(None);
        };
        if notify.property == NONE {
            return Ok(None);
        }
        let first = self.conn.get_property(true, self.window, a.RV_CLIPBOARD_DATA, AtomEnum::ANY, 0, u32::MAX / 4)?.reply()?;
        if first.type_ != a.INCR {
            return Ok(Some((first.type_, first.value)));
        }
        // INCR: the owner sends the data in pieces; each piece is announced by a change of the property, an empty one ends it.
        let mut data = Vec::new();
        loop {
            let deadline = Instant::now() + REQUEST_TIMEOUT;
            let changed = wait_event(&self.conn, deadline, |e| {
                matches!(e, Event::PropertyNotify(p) if p.window == self.window && p.atom == a.RV_CLIPBOARD_DATA && p.state == Property::NEW_VALUE)
            })?;
            if changed.is_none() {
                return Ok(None);
            }
            let chunk = self.conn.get_property(true, self.window, a.RV_CLIPBOARD_DATA, AtomEnum::ANY, 0, u32::MAX / 4)?.reply()?;
            if chunk.value.is_empty() {
                return Ok(Some((target, data)));
            }
            data.extend_from_slice(&chunk.value);
            if data.len() > MAX_PAYLOAD_BYTES {
                return Ok(None);
            }
        }
    }

    fn offered_targets(&self) -> Result<Vec<u32>> {
        let Some((_, bytes)) = self.fetch(self.atoms.TARGETS)? else {
            return Ok(Vec::new());
        };
        Ok(bytes.chunks_exact(4).map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]])).collect())
    }

    fn files(&self) -> Result<Option<ClipboardFiles>> {
        let a = &self.atoms;
        let owner = self.conn.get_selection_owner(a.CLIPBOARD)?.reply()?.owner;
        if owner == NONE {
            return Ok(None);
        }
        let targets = self.offered_targets()?;
        let offers = |atom: u32| targets.contains(&atom);
        if !offers(a.TEXT_URI_LIST) && !offers(a.GNOME_COPIED_FILES) {
            return Ok(None);
        }
        let get = |atom: u32| -> Result<Option<Vec<u8>>> { Ok(if offers(atom) { self.fetch(atom)?.map(|(_, bytes)| bytes) } else { None }) };
        let payloads = FilePayloads {
            kde_cut_selection: get(a.KDE_CUT_SELECTION)?,
            uri_list: get(a.TEXT_URI_LIST)?,
            gnome_copied_files: get(a.GNOME_COPIED_FILES)?,
        };
        Ok(format::files_from_payloads(&payloads))
    }
}

/// The files on the clipboard (ours or another application's); `None` when it holds none. Blocks until the owner answers, so call it
/// from a worker thread.
pub fn read_files() -> Result<Option<ClipboardFiles>> {
    if let Some(own) = OWNER.lock().clone().filter(|o| o.alive.load(Ordering::Acquire))
        && own.is_selection_owner()
        && let Some(content) = own.owned.lock().clone()
    {
        return Ok(Some(ClipboardFiles { paths: content.files.clone(), mode: content.mode, rejected: 0 }));
    }
    Reader::new()?.files()
}

/// Whether an X display is reachable.
pub fn available() -> bool {
    std::env::var_os("DISPLAY").is_some_and(|d| !d.is_empty())
}

#[cfg(test)]
mod tests {
    //! These tests talk to the real X server of the session (skipped without a display) and share one clipboard, so they run one at a time.
    use super::*;

    static SERIAL: Mutex<()> = Mutex::new(());

    fn guard() -> Option<parking_lot::MutexGuard<'static, ()>> {
        available().then(|| SERIAL.lock())
    }

    /// Reads through a second connection, like another application would (no shortcut to our own data).
    fn read_as_other_application() -> Option<ClipboardFiles> {
        Reader::new().ok()?.files().ok()?
    }

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| PathBuf::from(format!("/tmp/rv-clip/{n}"))).collect()
    }

    #[test]
    fn copy_and_cut_are_served_to_other_applications() {
        let Some(_g) = guard() else { return };
        let files = paths(&["a.jpg", "with space.png", "umlaut-äöü.mp4", "100%.gif", "hash#1.jpg", "q?.jpg"]);
        set(&files, ClipboardMode::Copy).unwrap();
        let read = read_as_other_application().unwrap();
        assert_eq!(read.paths, files);
        assert_eq!(read.mode, ClipboardMode::Copy);
        set(&files, ClipboardMode::Cut).unwrap();
        let read = read_as_other_application().unwrap();
        assert_eq!(read.paths, files);
        assert_eq!(read.mode, ClipboardMode::Cut);
        assert_eq!(own_files().unwrap().1, ClipboardMode::Cut);
    }

    #[test]
    fn a_long_list_travels_and_a_too_long_one_is_refused() {
        let Some(_g) = guard() else { return };
        let many: Vec<PathBuf> = (0..3000).map(|i| PathBuf::from(format!("/tmp/rv-clip/some/folder/file-number-{i:05}.jpg"))).collect();
        set(&many, ClipboardMode::Copy).unwrap();
        assert_eq!(read_as_other_application().unwrap().paths, many);
        let huge: Vec<PathBuf> = (0..1_000_000).map(|i| PathBuf::from(format!("/tmp/rv-clip/some/folder/file-number-{i:07}.jpg"))).collect();
        let error = set(&huge, ClipboardMode::Copy).unwrap_err();
        assert!(error.0.contains("too long"), "{error}");
        // The refusal leaves what was there before.
        assert_eq!(read_as_other_application().unwrap().paths, many);
    }

    #[test]
    fn clearing_only_empties_what_we_put_there() {
        let Some(_g) = guard() else { return };
        set(&paths(&["x.jpg"]), ClipboardMode::Cut).unwrap();
        clear_if_ours();
        assert!(own_files().is_none());
        assert!(read_as_other_application().is_none());
        clear_if_ours();
    }

    #[test]
    fn another_application_taking_the_clipboard_ends_our_ownership() {
        let Some(_g) = guard() else { return };
        set(&paths(&["x.jpg"]), ClipboardMode::Copy).unwrap();
        let (conn, window, atoms) = connect().unwrap();
        conn.set_selection_owner(window, atoms.CLIPBOARD, CURRENT_TIME).unwrap();
        conn.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while own_files().is_some() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(own_files().is_none(), "the SelectionClear was not noticed");
        // The new owner offers nothing we know: no files.
        assert!(read_files().unwrap().is_none());
        // Clearing must not take the clipboard away from the other owner.
        clear_if_ours();
        let owner = conn.get_selection_owner(atoms.CLIPBOARD).unwrap().reply().unwrap().owner;
        assert_eq!(owner, window);
        conn.set_selection_owner(NONE, atoms.CLIPBOARD, CURRENT_TIME).unwrap();
        conn.flush().unwrap();
    }

    #[test]
    fn unknown_targets_are_refused() {
        let Some(_g) = guard() else { return };
        set(&paths(&["x.jpg"]), ClipboardMode::Copy).unwrap();
        let reader = Reader::new().unwrap();
        let weird = reader.conn.intern_atom(false, b"application/x-nothing").unwrap().reply().unwrap().atom;
        assert!(reader.fetch(weird).unwrap().is_none());
        let targets = reader.offered_targets().unwrap();
        assert!(targets.contains(&reader.atoms.TEXT_URI_LIST) && targets.contains(&reader.atoms.GNOME_COPIED_FILES));
        clear_if_ours();
    }
}
