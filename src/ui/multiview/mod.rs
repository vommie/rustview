//! Multiview presenters: the glue between the Multiview core (`crate::viewer::multiview`) and the UI.
//!
//! `eligibility` decides from in-memory library state what is shown, `session*` present a Multiview in its own frameless
//! Slint window, `router` turns player events into UI actions and `handle` is the thread-safe face used by the rest of
//! the application. The state and logic of a running Multiview live in `crate::viewer::multiview::instance`.

pub mod eligibility;
pub mod handle;
mod router;
mod session;
mod session_input;
mod views;

pub use eligibility::OpenSummary;
pub use handle::MultiviewHandle;
