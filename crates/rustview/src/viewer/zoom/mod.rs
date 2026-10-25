//! Zooming an image or a video inside a Multiview tile: the model, the resampling of pictures, mpv's view of videos and the state.
//!
//! The pure parts (`geometry`, `scale`, `render`, `prefs`, `state`) know nothing about Slint windows or X11, so they are unit
//! tested; the Multiview instance and the plain image tab own a `ZoomState` per image and carry out what it asks for
//! (decoding the original, resampling on a worker thread).

pub mod geometry;
pub mod minimap;
pub mod mpv_view;
pub mod prefs;
pub mod render;
pub mod scale;
pub mod state;
