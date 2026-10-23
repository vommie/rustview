//! A running Multiview: the single source of truth for its tiles, arrangement, sound and interaction state.
//!
//! The instance knows nothing about Slint or tabs. A host (the fullscreen window, later a viewer tab or a tab-grid tile)
//! shows it by calling `set_host` and drawing `view()`; asynchronous events (decoded images, mpv callbacks) reach it through
//! the [`MultiviewEventSink`]. All methods run on the UI thread; the mutex only makes the handle `Send`, so callbacks on
//! other threads can hold a `Weak` reference and drop their event once the instance is gone. Dropping the last reference
//! tears the players down in the safe order, so nobody has to remember an explicit teardown.
//!
//! The behavior is split over `instance_host` (placement and projection), `instance_layout` (arrangement, gutters),
//! `instance_tiles` (tile lifecycle), `instance_follow` (every change of the tile set: following the Browse selection,
//! duplicating a tile, `instance_clone`), `instance_actions` (the action buttons), `instance_sound` (mute state) and `instance_hover` (pointer, mute badge, aspect probing).

use super::click::{ClickDetector, ClickKind};
use super::container::Container;
use super::events::MultiviewEventSink;
use super::follow::TileKey;
use super::instance_host::HostState;
use super::instance_layout::{DragState, layout_params};
use super::layout::{Layout, LayoutParams, Rect};
use super::route::{HostMode, OriginKind};
use super::teardown::{teardown_videos, teardown_videos_now};
use super::tile::{TileSeed, TileState, VideoTile};
use parking_lot::Mutex;
use rustview_core::library::models::MediaId;
use rustview_core::settings::schema::MULTIVIEW_DOUBLE_CLICK_MS;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

pub type SharedInstance = Arc<Mutex<MultiviewInstance>>;
pub type WeakInstance = Weak<Mutex<MultiviewInstance>>;

static NEXT_INSTANCE_ID: AtomicU64 = AtomicU64::new(1);

/// Size and scale of the virtual display used until a fullscreen host sets the real one.
const PLACEHOLDER_CONTAINER: Container = Container {
    width: 1920,
    height: 1080,
    dpi: 1.0,
};

pub struct MultiviewInstance {
    pub(super) me: WeakInstance,
    pub(super) id: u64,
    pub(super) sink: Arc<dyn MultiviewEventSink>,
    pub(super) origin: OriginKind,
    /// The tiles have been started (their players and decoders exist). A Multiview that was never shown is started by the first
    /// host that shows it.
    pub(super) started: bool,
    pub(super) tiles: Vec<TileState>,
    /// Heights of the two rows of a 2x2 grid after the user dragged the horizontal gutter; `None` = automatic. The widths
    /// the user set belong to the tiles (`TileState::pin`).
    pub(super) row_pins: Option<[f64; 2]>,
    pub(super) container: Container,
    pub(super) params: LayoutParams,
    /// Always solved in container pixels; hosts project it into their own rectangle.
    pub(super) layout: Layout,
    pub(super) host: HostState,
    pub(super) drag: Option<DragState>,
    pub(super) hovered: Option<usize>,
    pub(super) last_hovered: Option<MediaId>,
    /// Last pointer position in the coordinates of the host window (physical pixels).
    pub(super) pointer: Option<(i32, i32)>,
    /// When the pointer last moved; the mouse cursor hides once it rested long enough.
    pub(super) last_move: Option<Instant>,
    /// The fullscreen cannot be closed (runtime state of this Multiview, cleared when it leaves the fullscreen).
    pub(super) pinned: bool,
    /// The mouse cursor is hidden over the video windows right now.
    pub(super) cursor_hidden: bool,
    /// A refused close made the primary bar show itself and its pin button blink until this moment.
    pub(super) pin_pulse_until: Option<Instant>,
    /// The color the fullscreen paints behind the media (also around a single video, whose window covers the display).
    pub(super) video_background: Option<[u8; 3]>,
    pub(super) ticks: u32,
    pub(super) rename_target: Option<MediaId>,
    /// The rename target was named by a player (not picked by the pointer) and is used by the next rename request.
    pub(super) rename_pinned: bool,
    pub(super) clicks: ClickDetector,
    pub(super) revision: u64,
    /// The tiles whose zoom slider hangs below their media bar (every tile has its own; any number can be open).
    pub(super) zoom_slider: Vec<TileKey>,
    /// The tiles whose minimap carries a zoom slider bubble (right click on the minimap; every tile has its own).
    pub(super) zoom_minimap_bubble: Vec<TileKey>,
    /// Wheel movement over an image that has not made a whole step yet.
    pub(super) zoom_wheel_accum: f32,
}

impl MultiviewInstance {
    /// Creates an instance with one (not yet started) tile per seed.
    pub fn create(sink: Arc<dyn MultiviewEventSink>, seeds: Vec<TileSeed>) -> SharedInstance {
        let id = NEXT_INSTANCE_ID.fetch_add(1, Ordering::SeqCst);
        Arc::new_cyclic(|me| {
            Mutex::new(Self {
                me: me.clone(),
                id,
                sink,
                origin: OriginKind::Browse,
                started: false,
                tiles: seeds.into_iter().map(TileState::from_seed).collect(),
                row_pins: None,
                container: PLACEHOLDER_CONTAINER,
                params: layout_params(&PLACEHOLDER_CONTAINER, 0),
                layout: Layout::default(),
                host: HostState::hidden(&PLACEHOLDER_CONTAINER),
                drag: None,
                hovered: None,
                last_hovered: None,
                pointer: None,
                last_move: None,
                pinned: false,
                cursor_hidden: false,
                pin_pulse_until: None,
                video_background: None,
                ticks: 0,
                rename_target: None,
                rename_pinned: false,
                clicks: ClickDetector::default(),
                revision: 0,
                zoom_slider: Vec::new(),
                zoom_minimap_bubble: Vec::new(),
                zoom_wheel_accum: 0.0,
            })
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn origin(&self) -> OriginKind {
        self.origin
    }

    pub fn set_origin(&mut self, origin: OriginKind) {
        self.origin = origin;
    }

    /// Whether the tiles have been started.
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// The media the Multiview shows, each once and in slot order (a medium shown by several tiles is one member).
    pub fn member_ids(&self) -> Vec<MediaId> {
        let mut ids: Vec<MediaId> = Vec::with_capacity(self.tiles.len());
        for tile in &self.tiles {
            if !ids.contains(&tile.media_id) {
                ids.push(tile.media_id);
            }
        }
        ids
    }

    /// The medium of every tile in slot order (a medium shown twice appears twice).
    pub(super) fn slot_media_ids(&self) -> Vec<MediaId> {
        self.tiles.iter().map(|t| t.media_id).collect()
    }

    /// The key of the tile in slot `index` (the bar and the layout address tiles by slot, which changes whenever tiles come
    /// and go; the key does not).
    pub fn tile_key_at(&self, index: usize) -> Option<TileKey> {
        self.tiles.get(index).map(|t| t.key)
    }

    /// The first tile that shows `media_id`.
    pub fn tile_key_of(&self, media_id: MediaId) -> Option<TileKey> {
        self.tiles.iter().find(|t| t.media_id == media_id).map(|t| t.key)
    }

    pub fn has_member(&self, id: MediaId) -> bool {
        self.tiles.iter().any(|t| t.media_id == id)
    }

    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    pub fn container(&self) -> Container {
        self.container
    }

    pub fn host_mode(&self) -> HostMode {
        self.host.surface.mode
    }

    pub fn has_video(&self) -> bool {
        self.tiles.iter().any(|t| t.video.is_some())
    }

    /// True while an image tile waits for its decoder.
    pub fn is_loading(&self) -> bool {
        self.tiles.iter().any(|t| t.is_loading)
    }

    /// The rectangle of tile `index` in the host rectangle (physical pixels).
    pub fn tile_rect(&self, index: usize) -> Rect {
        self.host_content(index).unwrap_or_default()
    }

    /// Number of rows of the current arrangement (two for a 2x2 grid).
    pub fn row_count(&self) -> usize {
        self.layout.rows.len()
    }

    /// The media an F2 rename acts on (the tile the user pressed F2 over, or the one reported by its player).
    pub fn rename_target(&self) -> Option<MediaId> {
        self.rename_target
    }

    /// A player reported F2: the next rename request acts on that tile's medium.
    pub fn note_rename_target_of_tile(&mut self, key: TileKey) {
        if let Some(tile) = self.tiles.iter().find(|t| t.key == key) {
            self.rename_target = Some(tile.media_id);
            self.rename_pinned = true;
        }
    }

    /// The tile a rename request acts on: the one a player named, else the one under the pointer (see `rename_hovered`).
    pub fn resolve_rename_target(&mut self, pointer: Option<(i32, i32)>) -> Option<MediaId> {
        if std::mem::take(&mut self.rename_pinned) {
            return self.rename_target;
        }
        self.rename_hovered(pointer)
    }

    /// One click inside the Multiview (Slint tile or background, or a Ctrl+click reported by an mpv player).
    pub fn register_click(&mut self, is_ctrl: bool) -> ClickKind {
        self.clicks.register(
            Instant::now(),
            is_ctrl,
            Duration::from_millis(MULTIVIEW_DOUBLE_CLICK_MS),
        )
    }

    /// Changes whenever something a host draws changed; presenters compare it to skip identical syncs.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub(super) fn bump(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

impl MultiviewInstance {
    fn take_videos(&mut self) -> Vec<VideoTile> {
        self.tiles.drain(..).filter_map(|t| t.video).collect()
    }

    /// The application exits: the players are shut down and their windows destroyed right now, on the calling thread, instead
    /// of on the worker thread that `Drop` uses (the end of the process would cut that short). The instance is empty
    /// afterwards.
    pub fn shutdown_now(&mut self) {
        let videos = self.take_videos();
        tracing::info!(
            instance = self.id,
            players = videos.len(),
            "[Multiview: Instance] Shutting the players down for the exit of the application"
        );
        teardown_videos_now(videos);
        self.bump();
    }
}

impl Drop for MultiviewInstance {
    fn drop(&mut self) {
        let videos = self.take_videos();
        tracing::debug!(
            instance = self.id,
            players = videos.len(),
            "[Multiview: Instance] Released; tearing down the players"
        );
        teardown_videos(videos);
    }
}
