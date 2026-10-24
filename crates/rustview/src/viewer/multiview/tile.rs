//! Runtime state of one Multiview tile and the creation of its native video window.
//!
//! A video tile is an `X11VideoSubwindow` below the host window with an `MpvPlayer` attached to it, the usual callbacks,
//! `load_file` + `play`. mpv does whatever it does inside its rectangle; the layout only decides where that rectangle is.

use super::audio::TileAudio;
use super::events::{MultiviewEventSink, PlayerEvent};
use super::feedback_overlay::OverlayState;
use super::focus_overlay::CornerMarks;
use super::follow::TileKey;
use super::instance::WeakInstance;
use super::layout::Rect;
use super::media_actions::MediaActionId;
use super::model::{AspectSource, TileKind};
use crate::ui::button_feedback::ButtonFeedback;
use crate::viewer::edit::save_badge::SaveBadge;
use crate::viewer::edit::tile_edit::{EditSource, TileEdit};
use crate::viewer::zoom::state::ZoomState;
use crate::viewer::{MpvPlayer, X11VideoSubwindow, install_nonfatal_error_handler};
use parking_lot::RwLock;
use rustview_core::library::models::MediaId;
use rustview_core::util::cast::SaturatingAs;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// The Multiview that the events of a player go to. The callbacks are installed once when the player is created and read this at
/// every call, so a tile that moves to another Multiview (a merge of tabs) takes its running player along and only points the
/// cell at its new owner.
pub type EventOwner = Arc<RwLock<WeakInstance>>;

pub struct VideoTile {
    pub player: Arc<MpvPlayer>,
    pub window: X11VideoSubwindow,
    /// Mute state mpv started with (user config); `Native` audio restores it.
    pub native_mute: bool,
    /// Last mute state known, to tell our own changes from changes made inside mpv (`m`, OSC).
    pub last_mute: bool,
    /// The tile the player currently serves (`TileKey`). A recycled player moves on to another tile, and the callbacks that
    /// were installed once at creation read this instead of the key they were created for.
    pub current_tile: Arc<AtomicU64>,
    /// The Multiview whose tile this is; the events of the player are posted to it.
    pub owner: EventOwner,
    /// The rating icon that mpv draws over the picture right now, if any.
    pub feedback_overlay: Option<OverlayState>,
    /// The corner marks of the selected tile that mpv draws over the picture right now, if any.
    pub focus_overlay: Option<CornerMarks>,
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
    /// Already decoded pixels of an image (a duplicate of a tile that has them shares the buffer instead of decoding again).
    pub pixels: Option<SharedPixelBuffer<Rgb8Pixel>>,
    /// Pixel size of the original of an image whose `pixels` are given (the pixels may be a downscaled copy): what zooming needs.
    pub orig_size: Option<(u32, u32)>,
    /// What is known about the file of a decoded image (a duplicate takes it over from its source tile).
    pub source: Option<EditSource>,
    pub sticky: bool,
}

impl TileSeed {
    /// A medium that is not shown yet: nothing decoded, not sticky.
    pub fn new(media_id: MediaId, kind: TileKind, aspect: f32, path: PathBuf, filename: String) -> Self {
        Self {
            media_id,
            kind,
            aspect,
            path,
            filename,
            pixels: None,
            orig_size: None,
            source: None,
            sticky: false,
        }
    }
}

static NEXT_TILE_KEY: AtomicU64 = AtomicU64::new(1);

/// A key no other tile has had in this process.
pub fn next_tile_key() -> TileKey {
    NEXT_TILE_KEY.fetch_add(1, Ordering::Relaxed)
}

pub struct TileState {
    /// Identity of the tile. The same medium can be shown by several tiles; only the key tells them apart.
    pub key: TileKey,
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
    /// Stays on screen when the Browse selection changes (only Multiviews that follow the selection use it).
    pub sticky: bool,
    /// Width the user gave this tile by dragging a gutter, as a share of its row; `None` = automatic.
    pub pin: Option<f64>,
    /// Feedback of the action buttons of this tile.
    pub feedback: HashMap<MediaActionId, ButtonFeedback>,
    /// Display parameters mpv reported for the file a recycled player showed before: they are stale until they change.
    pub stale_probe: Option<StaleProbe>,
    /// The zoom of an image that is shown (`None` for a video, an image that is still loading or one that failed).
    pub zoom: Option<ZoomState>,
    /// The minimap was shown the last time the zoom was looked at (a change of it makes the view redraw).
    pub zoom_minimap_shown: bool,
    /// What the decoder learned about the file of an image (for editing and saving); `None` until it is decoded.
    pub source: Option<EditSource>,
    /// The edits of the picture while it is edited (the crop tool, unsaved changes, a save in progress).
    pub edit: Option<TileEdit>,
    /// The Save button of the tile (it outlives the edit for a moment after saving, fading out).
    pub save_badge: SaveBadge,
}

/// Parameters that must not be taken for the new file, and how many ticks to ignore them at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleProbe {
    pub params: (i64, i64, i64),
    pub ticks_left: u32,
}

impl TileState {
    pub fn from_seed(seed: TileSeed) -> Self {
        let is_video = seed.kind == TileKind::Video;
        let decoded = !is_video && seed.pixels.is_some();
        Self {
            key: next_tile_key(),
            media_id: seed.media_id,
            kind: seed.kind,
            filename: seed.filename,
            path: seed.path,
            aspect: seed.aspect,
            aspect_source: if decoded {
                AspectSource::Probed
            } else {
                AspectSource::Library
            },
            pixels: seed.pixels,
            is_loading: !is_video && !decoded,
            error: None,
            video: None,
            audio: is_video.then(TileAudio::native),
            sticky: seed.sticky,
            pin: None,
            feedback: HashMap::new(),
            stale_probe: None,
            zoom: seed.orig_size.filter(|_| decoded).map(ZoomState::new),
            zoom_minimap_shown: false,
            source: seed.source,
            edit: None,
            save_badge: SaveBadge::Hidden,
        }
    }
}

/// A player callback that posts an event for its instance; the event is built per call so it never has to be `Clone`.
fn forward(
    sink: &Arc<dyn MultiviewEventSink>,
    owner: &EventOwner,
    make: impl Fn() -> PlayerEvent + Send + Sync + 'static,
) -> impl Fn() + Send + Sync + 'static {
    let (sink, owner) = (Arc::clone(sink), Arc::clone(owner));
    move || {
        let me = owner.read().clone();
        sink.post(me, make());
    }
}

/// Installs every player callback once, when the tile is created. What an event means is decided when it arrives (by the
/// current host of the instance), so the callbacks never have to be replaced.
pub(super) fn connect_callbacks(
    player: &MpvPlayer,
    sink: &Arc<dyn MultiviewEventSink>,
    owner: &EventOwner,
    current_tile: &Arc<AtomicU64>,
) {
    player.on_toggle_fullscreen(forward(sink, owner, || PlayerEvent::ToggleFullscreen));
    player.on_close_tab(forward(sink, owner, || PlayerEvent::CloseTab));
    player.on_rename_requested(forward(sink, owner, || PlayerEvent::RenameRequested));
    player.on_escape(forward(sink, owner, || PlayerEvent::EscapePressed));
    player.on_delete_requested(forward(sink, owner, || PlayerEvent::DeleteRequested));
    let (rating_sink, rating_owner) = (Arc::clone(sink), Arc::clone(owner));
    player.on_rating_requested(move |rating| {
        let me = rating_owner.read().clone();
        rating_sink.post(me, PlayerEvent::RatingRequested(rating));
    });
    let tile = current_tile.clone();
    let (mute_sink, mute_owner) = (Arc::clone(sink), Arc::clone(owner));
    player.on_mute_changed(move |muted| {
        let tile = tile.load(Ordering::SeqCst);
        let me = mute_owner.read().clone();
        mute_sink.post(me, PlayerEvent::MuteChanged { tile, muted });
    });
    let tile = current_tile.clone();
    let (pause_sink, pause_owner) = (Arc::clone(sink), Arc::clone(owner));
    player.on_user_pause_changed(move |paused| {
        let tile = tile.load(Ordering::SeqCst);
        let me = pause_owner.read().clone();
        pause_sink.post(me, PlayerEvent::PauseChanged { tile, paused });
    });
    let tile = Arc::clone(current_tile);
    player.on_clicked(forward(sink, owner, move || {
        PlayerEvent::Clicked(tile.load(Ordering::SeqCst))
    }));
    player.on_convert_to_background_tab(forward(sink, owner, || PlayerEvent::CtrlClick));
    player.on_grid_left_click(forward(sink, owner, || PlayerEvent::GridLeftClick));
    player.on_grid_middle_click(forward(sink, owner, || PlayerEvent::GridMiddleClick));
    player.on_grid_wheel_up(forward(sink, owner, || PlayerEvent::GridWheel { up: true }));
    player.on_grid_wheel_down(forward(sink, owner, || PlayerEvent::GridWheel { up: false }));
}

/// Creates the video window inside `rect` (coordinates of the parent window) and starts playback.
pub fn create_video(
    sink: &Arc<dyn MultiviewEventSink>,
    me: &WeakInstance,
    parent_xid: u64,
    rect: Rect,
    tile: TileKey,
    path: &Path,
) -> Result<VideoTile, String> {
    let mut window = X11VideoSubwindow::new(
        parent_xid,
        rect.x,
        rect.y,
        rect.w.max(16).saturating_as::<u32>(),
        rect.h.max(16).saturating_as::<u32>(),
    )
    .map_err(|e| e.to_string())?;
    let wid = window.xid();
    window.show();

    let player = Arc::new(MpvPlayer::new().map_err(|e| e.to_string())?);
    let current_tile = Arc::new(AtomicU64::new(tile));
    let owner: EventOwner = Arc::new(RwLock::new(me.clone()));
    connect_callbacks(&player, sink, &owner, &current_tile);

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
        current_tile,
        owner,
        feedback_overlay: None,
        focus_overlay: None,
    })
}

/// Plays another medium in the player of `video`, now serving `tile`: one player and one window, `loadfile replace`, so
/// volume, speed and loop settings carry over.
pub fn reload_video(video: &VideoTile, tile: TileKey, path: &Path) -> Result<(), String> {
    video.current_tile.store(tile, Ordering::SeqCst);
    video.player.load_file(path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::multiview::instance::MultiviewInstance;
    use parking_lot::Mutex;

    /// Remembers the id of the instance each event was posted for.
    struct Recorder(Mutex<Vec<Option<u64>>>);

    impl MultiviewEventSink for Recorder {
        fn post(&self, instance: WeakInstance, _event: PlayerEvent) {
            self.0.lock().push(instance.upgrade().map(|i| i.lock().id()));
        }
    }

    #[test]
    fn a_callback_follows_the_owner_cell_when_the_tile_moves_to_another_multiview() {
        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        let sink: Arc<dyn MultiviewEventSink> = recorder.clone();
        let first = MultiviewInstance::create(sink.clone(), Vec::new());
        let second = MultiviewInstance::create(sink.clone(), Vec::new());
        let (first_id, second_id) = (first.lock().id(), second.lock().id());

        let owner: EventOwner = Arc::new(RwLock::new(Arc::downgrade(&first)));
        let callback = forward(&sink, &owner, || PlayerEvent::CloseTab);
        callback();
        *owner.write() = Arc::downgrade(&second);
        callback();
        drop(first);
        *owner.write() = Arc::downgrade(&second);
        callback();

        assert_eq!(
            *recorder.0.lock(),
            vec![Some(first_id), Some(second_id), Some(second_id)]
        );
    }
}
