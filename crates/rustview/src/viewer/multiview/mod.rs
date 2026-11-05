//! Multiview core, independent of the UI layer so that viewer tabs can own a running Multiview.
//!
//! The pure modules (`layout`, `layout_pins`, `audio`, `follow`, `model`, `navigation`, `actions`,
//! `container`, `click`, `route`, `focus`, `command`, `hint`) have no Slint/X11/mpv dependencies and are unit tested. `instance*` is the running Multiview (single source of truth for its
//! tiles, arrangement, sound and interaction state), `tile`/`teardown`/`image_decode` its native parts, `events` the
//! channel from player threads back to the UI and `view` the Slint-free description a host draws.

pub mod audio;
pub mod bar_placement;
pub mod click;
pub mod command;
pub mod container;
pub mod events;
pub mod focus;
pub mod follow;
pub mod hint;
pub mod image_decode;
pub mod instance;
mod instance_ab_repeat;
mod instance_actions;
mod instance_clone;
mod instance_commands;
pub mod instance_edit;
pub mod instance_edit_view;
mod instance_feedback;
mod instance_focus;
mod instance_follow;
pub mod instance_grid_bar;
mod instance_host;
mod instance_hover;
mod instance_layout;
mod instance_members;
mod instance_merge;
mod instance_player;
mod instance_player_events;
mod instance_primary;
mod instance_rating;
mod instance_sound;
pub mod instance_tick;
mod instance_tiles;
mod instance_video_bar;
mod instance_video_filter;
mod instance_video_zoom;
mod instance_volume_feedback;
mod instance_zoom;
pub mod key_labels;
pub mod layout;
pub mod layout_pins;
pub mod media_actions;
pub mod media_feedback;
pub mod model;
pub mod navigation;
pub mod player_prefs;
pub mod prefs;
pub mod primary_actions;
pub mod route;
pub mod teardown;
pub mod tile;
pub mod video_bar;
pub mod video_filter;
pub mod video_placement;
pub mod view;
pub mod volume_feedback;

pub use instance_host::HostSurface;
pub use instance_merge::ReleasedTiles;
