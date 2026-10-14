//! Multiview presenters: the glue between the Multiview core (`crate::viewer::multiview`) and the UI.
//!
//! `eligibility` decides from in-memory library state what is shown, `session*` present a Multiview in its own frameless
//! Slint window, `tab_host` shows it in a viewer tab, `router` turns player events into UI actions and `handle` is the thread-safe face used by the rest of
//! the application. The state and logic of a running Multiview live in `crate::viewer::multiview::instance`.

pub mod bar_overlay;
pub mod eligibility;
pub mod grid_host;
pub mod handle;
mod navigation;
mod router;
#[cfg(test)]
mod screenshot_lab;
mod session;
mod session_convert;
mod session_input;
mod tab_single;
pub mod tab_host;
mod views;

pub use eligibility::OpenSummary;
pub use handle::MultiviewHandle;
