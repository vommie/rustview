//! Support for the mpv script `ab_repeat.lua` (A-B repeats, `docs/mpv_ab_repeat.md`).
//!
//! The script runs inside every player (`RustView` loads the user's mpv scripts) and does all the work: it sets A and B, loops,
//! and saves, loads and deletes named ranges in its own JSON file. `RustView` only shows its state and sends its commands, so
//! a repeat behaves exactly as in mpv itself. The script publishes `user-data/ab_repeat/{info,state,ranges}`, which the player
//! observes (`mpv_engine/ab_repeat.rs`); commands are `script-message-to <client> ...` (`protocol::Command`).
//! The modules are pure (the protocol, which script counts as compatible, the stable colors of saved ranges, the preferences
//! of Settings > Viewer > Videos) except `moves`, which hands renamed and moved files to the script on a worker thread.

pub mod colors;
pub mod moves;
pub mod prefs;
pub mod protocol;
pub mod support;
