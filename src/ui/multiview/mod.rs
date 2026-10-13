//! Multiview presenters: the glue between the Multiview core (`crate::viewer::multiview`) and the UI.
//!
//! `eligibility` decides from in-memory library state what is shown, `session*` present a Multiview in its own frameless
//! Slint window, and `handle` is the thread-safe face used by the rest of the application. The pure modules live in
//! `crate::viewer::multiview` and are re-exported here for the sibling modules.

pub mod eligibility;
pub mod handle;
mod image_tile;
mod session;
mod session_hover;
mod session_layout;
mod session_sound;
mod session_tiles;
mod tiles;
mod views;

pub use crate::viewer::multiview::{audio, layout, model, reconcile};
pub use eligibility::OpenSummary;
pub use handle::MultiviewHandle;
