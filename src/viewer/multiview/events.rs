//! Events that mpv observer threads and decode workers send to a Multiview instance.
//!
//! Callbacks run on arbitrary threads and only post an event together with a weak reference to their instance; the sink
//! (implemented by the UI layer) hops to the UI thread and decides what the event means in the instance's current host.
//! A dead weak reference means the instance is gone, so the event is stale and dropped.

use super::instance::WeakInstance;
use crate::library::models::MediaId;
use slint::{Rgb8Pixel, SharedPixelBuffer};

/// Original pixel size and the (possibly downscaled) pixels of a decoded image tile, or a user-facing error text.
pub type DecodedImage = Result<(u32, u32, SharedPixelBuffer<Rgb8Pixel>), String>;

pub enum PlayerEvent {
    ToggleFullscreen,
    CloseTab,
    RenameRequested(MediaId),
    /// The mute state of the player of this medium changed (a hotkey or the on-screen controller of mpv, or us).
    MuteChanged {
        media_id: MediaId,
        muted: bool,
    },
    /// The user's pause state of the player of this medium changed.
    PauseChanged {
        media_id: MediaId,
        paused: bool,
    },
    CtrlClick,
    GridLeftClick,
    GridMiddleClick,
    GridWheel {
        up: bool,
    },
    ImageDecoded {
        media_id: MediaId,
        result: DecodedImage,
    },
}

pub trait MultiviewEventSink: Send + Sync {
    /// Called on any thread.
    fn post(&self, instance: WeakInstance, event: PlayerEvent);
}
