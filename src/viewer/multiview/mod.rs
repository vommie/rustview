//! Multiview core, independent of the UI layer so that viewer tabs can own a running Multiview.
//!
//! The pure modules (`layout`, `audio`, `reconcile`, `model`, `container`, `click`, `route`, `target_pick`) have no
//! Slint/X11/mpv dependencies and are unit tested.

pub mod audio;
pub mod click;
pub mod container;
pub mod layout;
pub mod model;
pub mod reconcile;
pub mod route;
pub mod target_pick;
