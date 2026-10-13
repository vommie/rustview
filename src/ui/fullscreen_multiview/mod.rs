//! Multiview: several media side by side in one quick-fullscreen window.
//!
//! The pure parts (`layout`, `audio`, `reconcile`, `model`) have no Slint/X11/mpv dependencies and are unit tested. The
//! rest is glue: `eligibility` decides from in-memory library state what is shown, `session*` own the Slint window and
//! the tiles (each video tile is an `X11VideoSubwindow` with an `MpvPlayer`, exactly as in the single quick fullscreen),
//! and `handle` is the thread-safe face used by the rest of the application.

pub mod audio;
pub mod eligibility;
pub mod handle;
mod image_tile;
pub mod layout;
pub mod model;
pub mod reconcile;
mod session;
mod session_hover;
mod session_layout;
mod session_sound;
mod session_tiles;
mod tiles;
mod views;

pub use eligibility::OpenSummary;
pub use handle::MultiviewHandle;
