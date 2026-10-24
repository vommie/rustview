//! Multiview core, independent of the UI layer so that viewer tabs can own a running Multiview.
//!
//! The pure modules (`layout`, `layout_pins`, `audio`, `follow`, `model`, `navigation`, `actions`,
//! `container`, `click`, `route`, `target_pick`) have no Slint/X11/mpv dependencies and are unit tested. `instance*` is the running Multiview (single source of truth for its
//! tiles, arrangement, sound and interaction state), `tile`/`teardown`/`image_decode` its native parts, `events` the
//! channel from player threads back to the UI and `view` the Slint-free description a host draws.

pub mod audio;
pub mod bar_menu;
pub mod bar_placement;
pub mod click;
pub mod container;
pub mod events;
pub mod feedback_overlay;
pub mod follow;
pub mod image_decode;
pub mod instance;
mod instance_actions;
mod instance_clone;
pub mod instance_edit;
pub mod instance_edit_view;
mod instance_feedback;
mod instance_follow;
mod instance_host;
mod instance_hover;
mod instance_layout;
mod instance_members;
mod instance_merge;
mod instance_primary;
mod instance_rating;
mod instance_sound;
mod instance_tiles;
mod instance_zoom;
pub mod layout;
pub mod layout_pins;
pub mod media_actions;
pub mod media_feedback;
pub mod model;
pub mod navigation;
pub mod prefs;
pub mod primary_actions;
pub mod rating_keys;
pub mod route;
pub mod target_pick;
pub mod teardown;
pub mod tile;
pub mod view;

pub use instance_host::HostSurface;
pub use instance_merge::ReleasedTiles;
