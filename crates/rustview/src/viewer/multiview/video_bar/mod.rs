//! The control bar of a video in the Multiview (pure): what it shows, where it goes and when, and what its clicks mean.
//!
//! Every video tile of the fullscreen and the tab view has a bar of its own at its bottom (play/pause, the current time, the
//! progress bar, the remaining time and the volume). The canvas draws it over the video (`ui/multiview_video_bar.slint`, fed by
//! `ui/multiview/video_bar`); `RustView` takes the pointer over videos, so mpv's own on-screen controller is off.
//! Settings > Multiview > Player (`prefs`) choose its style, opacity, height, when it appears and whether it fades.
//!
//! A narrow bar collapses step by step (`collapse`): the volume slider becomes a speaker with a vertical volume bar, the
//! remaining time goes, the A-B group becomes one button with a bubble, then the sound and the A-B repeat move into a "..."
//! menu (`more_menu`), last the current time. The progress bar keeps its minimum width throughout (`popups` holds the
//! popups this adds).

pub mod ab_menu;
pub mod ab_state;
pub mod ab_view;
pub mod bar_menu;
pub mod collapse;
pub mod more_menu;
pub mod placement;
pub mod popups;
pub mod prefs;
pub mod progress;
pub mod reveal;
pub mod state;
pub mod time_text;
pub mod view;
pub mod volume;
