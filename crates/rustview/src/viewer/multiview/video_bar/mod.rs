//! The control bar of a video in the Multiview (pure): what it shows, where it goes and when, and what its clicks mean.
//!
//! Every video tile of the fullscreen and the tab view has a bar of its own at its bottom (play/pause, the current time, the
//! progress bar, the remaining time and the volume). The bar is a window of its own above the native mpv window, like the
//! action bars (`ui/multiview/video_bar`); `RustView` takes the pointer over videos, so mpv's own on-screen controller is off.
//! Settings > Fullscreen > Player (`prefs`) choose its style, opacity, height, when it appears and whether it fades.

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
