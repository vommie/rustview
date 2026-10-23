//! Events that mpv observer threads and decode workers send to a Multiview instance.
//!
//! Callbacks run on arbitrary threads and only post an event together with a weak reference to their instance; the sink
//! (implemented by the UI layer) hops to the UI thread and decides what the event means in the instance's current host.
//! A dead weak reference means the instance is gone, so the event is stale and dropped.

use super::follow::TileKey;
use super::instance::WeakInstance;
use crate::viewer::zoom::render::SourceImage;
use crate::viewer::zoom::state::RenderDone;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::sync::Arc;

/// Original pixel size and the (possibly downscaled) pixels of a decoded image tile, or a user-facing error text.
pub type DecodedImage = Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), String>;

pub enum PlayerEvent {
    ToggleFullscreen,
    CloseTab,
    /// F2 in the window of this tile's player.
    RenameRequested(TileKey),
    /// DEL was pressed in a video's window.
    DeleteRequested,
    /// A rating key (Ctrl+0..5) was pressed in a video's window.
    RatingRequested(u8),
    /// The mute state of the player of this tile changed (a hotkey or the on-screen controller of mpv, or us).
    MuteChanged {
        tile: TileKey,
        muted: bool,
    },
    /// The user's pause state of the player of this tile changed.
    PauseChanged {
        tile: TileKey,
        paused: bool,
    },
    CtrlClick,
    GridLeftClick,
    GridMiddleClick,
    GridWheel {
        up: bool,
    },
    ImageDecoded {
        tile: TileKey,
        result: DecodedImage,
    },
    /// The original of an image tile that is zoomed in was decoded (or could not be).
    ZoomSource {
        tile: TileKey,
        result: Result<Arc<SourceImage>, String>,
    },
    /// A zoomed picture was resampled.
    ZoomRendered {
        tile: TileKey,
        done: RenderDone,
    },
}

pub trait MultiviewEventSink: Send + Sync {
    /// Called on any thread.
    fn post(&self, instance: WeakInstance, event: PlayerEvent);
}
