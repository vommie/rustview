//! The control bar of a video in the Multiview (pure): what it shows, where it goes and when, and what its clicks mean.
//!
//! Every video tile of the fullscreen and the tab view has a bar of its own at its bottom (play/pause, the current time, the
//! progress bar, the remaining time and the volume). The canvas draws it over the video (`ui/multiview_video_bar.slint`, fed by
//! `ui/multiview/video_bar`); `RustView` takes the pointer over videos, so mpv's own on-screen controller is off.
//! Settings > Multiview > Player (`prefs`) choose its style, opacity, height, when it appears and whether it fades.

pub mod ab_menu;
pub mod ab_state;
pub mod ab_view;
pub mod placement;
pub mod prefs;
pub mod progress;
pub mod reveal;
pub mod state;
pub mod time_text;
pub mod view;
pub mod volume;
