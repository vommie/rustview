//! Joins the type-ahead search to the Browse list: names from memory, the jump through the cursor move of the arrow keys.

use super::session::{Cursor, NameSource, Reaction, TypeAhead, classify};
use crate::library::Library;
use crate::settings::SettingsManager;
use crate::thumbnails::ThumbnailManager;
use crate::ui::MainWindow;
use crate::ui::bridge_state::UiState;
use crate::ui::browse_folders::{FolderRowRef, Slot};
use crate::ui::keyboard_navigation::{CursorMove, Reveal, move_browse_cursor};
use crate::ui::multiview::handle::Pace;
use parking_lot::RwLock;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

/// The search of the window; lives on the UI thread only.
pub type TypeAheadState = Rc<RefCell<TypeAhead>>;

pub struct TypeAheadParams<'a> {
    /// `KeyEvent.text` of the key press.
    pub text: &'a str,
    /// Ctrl, Alt or Meta is held.
    pub command: bool,
    pub search: &'a TypeAheadState,
    pub window: &'a MainWindow,
    pub library: &'a Arc<Library>,
    pub thumbnails: &'a Arc<ThumbnailManager>,
    pub settings: &'a Arc<SettingsManager>,
    pub state: &'a Arc<RwLock<UiState>>,
}

/// The names of the list positions as the Browse view shows them: media by their lower-cased file name (the key of the text
/// filter, built when the record was loaded), folders by their name in the folder snapshot; "..." has none.
struct ListNames<'a>(&'a UiState);

impl NameSource for ListNames<'_> {
    fn count(&self) -> usize {
        self.0.view_len()
    }

    fn starts_with(&self, position: usize, needle: &str) -> bool {
        match self.0.slot_at(position) {
            Slot::Media(row) => self.0.item_at(row).is_some_and(|item| item.filename_lower.starts_with(needle)),
            Slot::Folder(FolderRowRef::Folder(id)) => self
                .0
                .folder_hierarchy
                .as_ref()
                .and_then(|snapshot| snapshot.get_node(id))
                .is_some_and(|node| lower_starts_with(&node.name, needle)),
            Slot::Folder(FolderRowRef::Up) | Slot::Empty => false,
        }
    }
}

/// `name.to_lowercase().starts_with(needle)` without building the lower-case name.
fn lower_starts_with(name: &str, needle: &str) -> bool {
    let mut lower = name.chars().flat_map(char::to_lowercase);
    needle.chars().all(|expected| lower.next() == Some(expected))
}

/// A key press of the Browse view. Returns whether the search took the key (the caller then does not handle it any further).
pub fn handle_type_ahead_key(params: TypeAheadParams<'_>) -> bool {
    let started = Instant::now();
    let class = classify(params.text, params.command);

    let (reaction, in_place, landed) = {
        let st = params.state.read();
        let item = st.browse_cursor();
        let cursor = Cursor { item, position: item.and_then(|item| st.browse_position_of(item)) };
        let reaction = params.search.borrow_mut().key(class, started, cursor, &ListNames(&st));
        let landed = match reaction {
            Reaction::Jump(position) => st.browse_item_at(position),
            _ => item,
        };
        // A jump onto the item the cursor is on changes nothing, unless more than that one item is selected.
        let in_place = matches!(reaction, Reaction::Jump(position) if Some(position) == cursor.position)
            && matches!(st.browse_selection_counts(), (0, 1) | (1, 0));
        (reaction, in_place, landed)
    };

    match reaction {
        Reaction::NotHandled => return false,
        Reaction::Consumed => {}
        Reaction::Jump(_) if in_place => {}
        Reaction::Jump(position) => {
            move_browse_cursor(
                &CursorMove {
                    window: params.window,
                    library: params.library,
                    thumbnails: params.thumbnails,
                    settings: params.settings,
                    state: params.state,
                },
                position,
                false,
                Reveal::Centered,
                Pace::Settled,
            );
        }
    }
    params.search.borrow_mut().settle(landed);

    tracing::debug!(
        reaction = ?reaction,
        query_len = params.search.borrow().len(),
        elapsed_us = started.elapsed().as_micros(),
        "[TypeAhead: Key] Type-ahead find handled a key"
    );
    true
}
