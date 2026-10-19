//! Cut, copy and paste of media through the system clipboard (Ctrl+X / Ctrl+C / Ctrl+V and the context menus).
//!
//! The clipboard is shared with the file managers (`system::file_clipboard`): what RustView cuts or copies can be pasted in Dolphin, and
//! what Dolphin cuts or copies can be pasted here. `summary` follows the clipboard for the menus (memory only), `guard` decides where
//! Ctrl+V may paste, `entry` is the menu entry, `copy` fills the clipboard and `run` pastes (planner and executor in `library::paste`,
//! conflicts through the existing conflict dialog).
//!
//! The paste needs the same handles as a drop (library, thumbnails, dialogs), so the UI bridge installs them once as
//! [`ClipboardBridge`]; menus and shortcuts reach it through [`ClipboardBridge::current`].

mod copy;
mod entry;
mod guard;
mod report;
mod run;
mod summary;

pub use copy::CopySelection;
pub use entry::{PasteEntry, PasteTarget};
pub use guard::{visible_folder, PasteBlock, ViewFacts};
pub use summary::ClipboardSummary;

use super::bridge_media::BridgeMedia;
use super::bridge_notice_dialog::NoticeDialogController;
use super::bridge_state::UiState;
use super::{FileOpsConflictDialogController, MainWindow};
use crate::library::models::FolderId;
use crate::library::Library;
use crate::settings::SettingsManager;
use crate::system::file_clipboard::FileClipboard;
use crate::thumbnails::ThumbnailManager;
use parking_lot::RwLock;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::sync::Arc;

#[derive(Clone)]
pub struct ClipboardBridge {
    pub(super) window: slint::Weak<MainWindow>,
    pub(super) library: Arc<Library>,
    pub(super) thumbnails: Arc<ThumbnailManager>,
    pub(super) settings: Arc<SettingsManager>,
    pub(super) state: Arc<RwLock<UiState>>,
    pub(super) conflict_dialog: FileOpsConflictDialogController,
    pub(super) notice_dialog: NoticeDialogController,
}

thread_local! {
    static INSTANCE: RefCell<Option<ClipboardBridge>> = const { RefCell::new(None) };
}

impl ClipboardBridge {
    pub fn new(
        window: &MainWindow,
        library: Arc<Library>,
        thumbnails: Arc<ThumbnailManager>,
        settings: Arc<SettingsManager>,
        state: Arc<RwLock<UiState>>,
        conflict_dialog: FileOpsConflictDialogController,
        notice_dialog: NoticeDialogController,
    ) -> Self {
        Self { window: window.as_weak(), library, thumbnails, settings, state, conflict_dialog, notice_dialog }
    }

    /// Makes this the bridge menus and shortcuts reach, wires the shortcuts and starts following the clipboard. UI thread only.
    pub fn install(self, window: &MainWindow) {
        let bridge = self.clone();
        window.on_cut_selection_requested({
            let bridge = bridge.clone();
            move || bridge.cut_selected()
        });
        window.on_copy_selection_requested({
            let bridge = bridge.clone();
            move || bridge.copy_selected()
        });
        window.on_paste_requested(move || bridge.paste_into_visible_folder());
        INSTANCE.with(|slot| *slot.borrow_mut() = Some(self));
        Self::start_watching();
    }

    /// The installed bridge (UI thread only).
    pub fn current() -> Option<Self> {
        INSTANCE.with(|slot| slot.borrow().clone())
    }

    /// Keeps `ClipboardSummary` current without polling: the clipboard reports its changes and is read once per change.
    fn start_watching() {
        let watching = FileClipboard::watch(|files| ClipboardSummary::of(files.as_ref()).publish());
        if !watching {
            tracing::info!("[Clipboard: Watch] The clipboard cannot be followed; the Paste entries stay enabled and the paste decides");
        }
    }

    /// What the Paste entry of a folder's menu shows.
    pub fn folder_entry(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>, folder_missing: bool) -> PasteEntry {
        let target = if !FileClipboard::is_supported() {
            PasteTarget::Unsupported
        } else if folder_missing {
            PasteTarget::MissingFolder
        } else if BridgeMedia::is_system_busy(window, thumbnails, state) {
            PasteTarget::Busy
        } else {
            PasteTarget::Ready
        };
        PasteEntry::new(&ClipboardSummary::current(), &target)
    }

    /// What the Paste entry of the menu of the Browse view's free space shows (it pastes into the open folder).
    pub fn background_entry(window: &MainWindow, thumbnails: &ThumbnailManager, state: &RwLock<UiState>) -> PasteEntry {
        let view = Self::view_facts(state);
        let target = match visible_folder(&view) {
            _ if !FileClipboard::is_supported() => PasteTarget::Unsupported,
            Err(block) => PasteTarget::NotAFolder(block.short()),
            Ok(_) if BridgeMedia::is_system_busy(window, thumbnails, state) => PasteTarget::Busy,
            Ok(_) => PasteTarget::Ready,
        };
        PasteEntry::new(&ClipboardSummary::current(), &target)
    }

    /// What the Browse view shows, from memory only.
    pub fn view_facts(state: &RwLock<UiState>) -> ViewFacts {
        let st = state.read();
        let node = st
            .current_folder_id
            .and_then(|id| st.folder_hierarchy.as_ref().and_then(|snapshot| snapshot.get_node(id)));
        ViewFacts {
            folder_id: st.current_folder_id,
            folder_name: node.map(|n| n.name.clone()).unwrap_or_default(),
            is_query: st.is_query_active,
            is_recursive: st.is_virtual_folder_mode,
            folder_missing: node.is_some_and(|n| n.is_deleted),
        }
    }

    /// Ctrl+V and the Paste entry of the free space.
    pub fn paste_into_visible_folder(&self) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        match visible_folder(&Self::view_facts(&self.state)) {
            Ok(folder_id) => self.paste_into(&window, folder_id),
            Err(block) => {
                tracing::info!(reason = block.short(), "[Clipboard: Paste] Not pasted: the open view is no folder to paste into");
                self.explain_block(&window, &block)
            }
        }
    }

    /// The Paste entry of a folder's menu.
    pub fn paste_into_folder(&self, folder_id: FolderId) {
        if let Some(window) = self.window.upgrade() {
            self.paste_into(&window, folder_id);
        }
    }

    fn cut_selected(&self) {
        if let Some(window) = self.window.upgrade() {
            CopySelection::from_selection(&window, &self.state, crate::system::file_clipboard::ClipboardMode::Cut);
        }
    }

    fn copy_selected(&self) {
        if let Some(window) = self.window.upgrade() {
            CopySelection::from_selection(&window, &self.state, crate::system::file_clipboard::ClipboardMode::Copy);
        }
    }

    /// Cut or copy of the media a menu was opened for.
    pub fn put_ids_on_clipboard(
        window: &MainWindow,
        library: &Library,
        state: &RwLock<UiState>,
        ids: &[crate::library::models::MediaId],
        mode: crate::system::file_clipboard::ClipboardMode,
    ) {
        CopySelection::from_ids(window, library, state, ids, mode);
    }
}
