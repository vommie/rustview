//! Runtime state of one Multiview tile and the creation of its native video window.
//!
//! A video tile is set up exactly like the standalone video of the single quick fullscreen: an `X11VideoSubwindow` below the
//! host window, an `MpvPlayer` attached to it, the same callbacks, `load_file` + `play`. mpv does whatever it does
//! inside its rectangle; the layout only decides where that rectangle is.

use super::audio::TileAudio;
use super::events::{MultiviewEventSink, PlayerEvent};
use super::instance::WeakInstance;
use super::layout::Rect;
use super::model::{AspectSource, TileKind};
use crate::library::models::MediaId;
use crate::viewer::{MpvPlayer, X11VideoSubwindow, install_nonfatal_error_handler};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct VideoTile {
    pub player: Arc<MpvPlayer>,
    pub window: X11VideoSubwindow,
    /// Mute state mpv started with (user config); `Native` audio restores it.
    pub native_mute: bool,
    /// Last mute state known, to tell our own changes from changes made inside mpv (`m`, OSC).
    pub last_mute: bool,
}

/// What is known about a medium before its tile exists (from the in-memory library state, never from the drive).
#[derive(Debug, Clone)]
pub struct TileSeed {
    pub media_id: MediaId,
    pub kind: TileKind,
    /// Aspect ratio from the library record; corrected from mpv's `video-params` once a video is playing.
    pub aspect: f32,
    pub path: PathBuf,
    pub filename: String,
}

pub struct TileState {
    pub media_id: MediaId,
    pub kind: TileKind,
    pub filename: String,
    pub path: PathBuf,
    pub aspect: f32,
    pub aspect_source: AspectSource,
    pub pixels: Option<SharedPixelBuffer<Rgb8Pixel>>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub video: Option<VideoTile>,
    /// `None` for images (no sound).
    pub audio: Option<TileAudio>,
}

impl TileState {
    pub fn from_seed(seed: TileSeed) -> Self {
        let is_video = seed.kind == TileKind::Video;
        Self {
            media_id: seed.media_id,
            kind: seed.kind,
            filename: seed.filename,
            path: seed.path,
            aspect: seed.aspect,
            aspect_source: AspectSource::Library,
            pixels: None,
            is_loading: !is_video,
            error: None,
            video: None,
            audio: is_video.then(TileAudio::native),
        }
    }
}

/// A player callback that posts an event for its instance; the event is built per call so it never has to be `Clone`.
fn forward(
    sink: &Arc<dyn MultiviewEventSink>,
    me: &WeakInstance,
    make: impl Fn() -> PlayerEvent + Send + Sync + 'static,
) -> impl Fn() + Send + Sync + 'static {
    let (sink, me) = (Arc::clone(sink), me.clone());
    move || sink.post(me.clone(), make())
}

/// Installs every player callback once, when the tile is created. What an event means is decided when it arrives (by the
/// current host of the instance), so the callbacks never have to be replaced.
fn connect_callbacks(
    player: &MpvPlayer,
    sink: &Arc<dyn MultiviewEventSink>,
    me: &WeakInstance,
    media_id: MediaId,
) {
    player.on_toggle_fullscreen(forward(sink, me, || PlayerEvent::ToggleFullscreen));
    player.on_close_tab(forward(sink, me, || PlayerEvent::CloseTab));
    player.on_rename_requested(forward(sink, me, move || {
        PlayerEvent::RenameRequested(media_id)
    }));
    player.on_convert_to_background_tab(forward(sink, me, || PlayerEvent::CtrlClick));
    player.on_grid_left_click(forward(sink, me, || PlayerEvent::GridLeftClick));
    player.on_grid_middle_click(forward(sink, me, || PlayerEvent::GridMiddleClick));
    player.on_grid_wheel_up(forward(sink, me, || PlayerEvent::GridWheel { up: true }));
    player.on_grid_wheel_down(forward(sink, me, || PlayerEvent::GridWheel { up: false }));
}

/// Creates the video window inside `rect` (coordinates of the parent window) and starts playback.
pub fn create_video(
    sink: &Arc<dyn MultiviewEventSink>,
    me: &WeakInstance,
    parent_xid: u64,
    rect: Rect,
    media_id: MediaId,
    path: &Path,
) -> Result<VideoTile, String> {
    let mut window = X11VideoSubwindow::new(
        parent_xid,
        rect.x,
        rect.y,
        rect.w.max(16) as u32,
        rect.h.max(16) as u32,
    )
    .map_err(|e| e.to_string())?;
    let wid = window.xid();
    window.show();

    let player = Arc::new(MpvPlayer::new().map_err(|e| e.to_string())?);
    connect_callbacks(&player, sink, me, media_id);

    let started = player.attach_to_window(wid).and_then(|()| {
        player.set_tab_grid_mode(false);
        player.set_view_active(true);
        player.set_user_paused(false);
        player.load_file(path)?;
        player.play()
    });
    if let Err(e) = started {
        player.shutdown_blocking();
        install_nonfatal_error_handler();
        return Err(e.to_string());
    }

    let native_mute = player.get_mute();
    Ok(VideoTile {
        player,
        window,
        native_mute,
        last_mute: native_mute,
    })
}
