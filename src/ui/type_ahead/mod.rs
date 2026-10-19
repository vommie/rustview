//! Type-ahead find in the Browse view (grid and table): typing without any visible field jumps to the first item whose name
//! starts with what was typed, the way Dolphin, Windows Explorer and the GTK/Qt item views do it.
//!
//! * `session` (pure) is the state machine: key classification, the timed search buffer, which item to start from, repeated keys
//!   that cycle through the items of one letter, a pause that starts a new search, Backspace and Escape while a search runs.
//! * `handler` joins it to the Browse list: the names come from memory (`filename_lower` of the records, the folder snapshot), the
//!   jump goes through the same cursor move as the arrow keys.
//!
//! Nothing here touches the drive or runs a timer: a search expires lazily, when the next key arrives.

mod handler;
pub mod session;

pub use handler::{TypeAheadParams, TypeAheadState, handle_type_ahead_key};
pub use session::{KeyClass, NameSource, Reaction, TypeAhead};
