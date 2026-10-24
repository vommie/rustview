//! Multiview presenters: the glue between the Multiview core (`crate::viewer::multiview`) and the UI.
//!
//! `eligibility` decides from in-memory library state what is shown, `session*` present a Multiview in its own frameless
//! Slint window, `tab_host` shows it in a viewer tab (every tab is a Multiview: one tile for a tab opened on one medium), `router` turns player events into UI actions, `hint` shows the short hint over the view and `handle` is the
//! thread-safe face used by the rest of the application. The state and logic of a running Multiview live in `crate::viewer::multiview::instance`.

mod bar_hosting;
pub mod bar_overlay;
mod commands;
pub mod cursor;
pub mod eligibility;
mod feedback;
pub mod grid_host;
pub mod handle;
mod handle_edit;
pub mod hint;
mod navigation;
mod pointer_lock;
mod rating;
mod router;
mod session;
mod session_convert;
mod session_input;
pub mod tab_host;
mod views;

pub use eligibility::OpenSummary;

/// Where the Multiviews of tabs report their events (decoded images, player events): the UI thread, through `handle`.
pub fn event_sink(handle: MultiviewHandle) -> std::sync::Arc<dyn crate::viewer::multiview::events::MultiviewEventSink> {
    std::sync::Arc::new(router::UiEventSink::new(handle))
}
pub use handle::MultiviewHandle;

/// Applies changed settings to a fullscreen that is open (its background). UI thread only.
pub fn refresh_settings(settings: &rustview_core::settings::SettingsManager) {
    session::refresh_settings(settings);
}
