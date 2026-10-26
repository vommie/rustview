//! Events that mpv observer threads and decode workers send to a Multiview instance.
//!
//! Callbacks run on arbitrary threads and only post an event together with a weak reference to their instance; the sink
//! (implemented by the UI layer) hops to the UI thread and decides what the event means in the instance's current host.
//! A dead weak reference means the instance is gone, so the event is stale and dropped.

use super::follow::TileKey;
use super::instance::WeakInstance;
use crate::viewer::edit::tile_edit::EditSource;
use crate::viewer::zoom::render::SourceImage;
use crate::viewer::zoom::state::RenderDone;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::sync::Arc;

/// Original pixel size and the (possibly downscaled) pixels of a decoded image tile, or a user-facing error text.
pub type DecodedImage = Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), String>;

pub enum PlayerEvent {
    /// The mute state of the player of this tile changed (a key, the control bar or a button of `RustView`, or a script inside mpv).
    MuteChanged { tile: TileKey, muted: bool },
    /// The user's pause state of the player of this tile changed.
    PauseChanged { tile: TileKey, paused: bool },
    /// Pause, end of file, duration or volume of the player of this tile changed (its numbers: `MpvPlayer::playback`).
    Playback { tile: TileKey },
    /// mpv knows the display size of the video of this tile.
    VideoParams { tile: TileKey },
    /// mpv cannot play the video of this tile (or stopped by itself): the tile shows `message` where the video would be.
    Failed { tile: TileKey, message: String },
    /// The mpv script `ab_repeat.lua` in the player of this tile published a change (its support, A-B or saved repeats).
    AbRepeat {
        tile: TileKey,
        change: crate::viewer::mpv_engine::ab_repeat::AbChange,
    },
    ImageDecoded {
        tile: TileKey,
        result: DecodedImage,
        /// What the decoder learned about the file (for editing); `None` when the decode failed before it.
        source: Option<EditSource>,
    },
    /// The sharp picture of the edited state of an image (`revision` of its edits), decoded from the original.
    EditPreview {
        tile: TileKey,
        revision: u64,
        result: DecodedImage,
    },
    /// The original of an image tile that is zoomed in was decoded (or could not be).
    ZoomSource {
        tile: TileKey,
        result: Result<Arc<SourceImage>, String>,
    },
    /// A zoomed picture was resampled.
    ZoomRendered { tile: TileKey, done: RenderDone },
}

pub trait MultiviewEventSink: Send + Sync {
    /// Called on any thread.
    fn post(&self, instance: WeakInstance, event: PlayerEvent);
}
