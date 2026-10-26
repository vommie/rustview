//! Videos as Slint images: mpv's OpenGL render API draws every video on a render thread of its own into textures that share a
//! GL share group with the Slint window showing the video; the window shows the newest texture as an ordinary `Image`
//! (`video-frames` of the window, the row of the tile). So everything Slint draws (menus, tooltips, popups, bars) lies over the
//! video without windows of its own, on X11 and on Wayland, with any OpenGL renderer of Slint (femtovg uses GLX on X11, skia
//! and Wayland use EGL).
//!
//! - [`VideoSurface`]: one per player (owned by its video tile): the render thread, the window it draws into, its row.
//! - [`hosts`]: the windows that show videos, driven by their rendering notifiers (UI thread).
//! - `render_thread`, `exchange`, `target`, `share_group`, `gl_api`: the GL plumbing (lab approach 03, `docs/video_in_slint/`).

mod exchange;
pub mod gl_api;
pub mod hosts;
mod render_thread;
pub mod share_group;
mod surface;
pub mod target;

pub use exchange::ReadyAction;
pub use hosts::{FULLSCREEN_WINDOW, HostWindowId, MAIN_WINDOW};
pub use surface::VideoSurface;
