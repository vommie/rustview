//! What a host has to draw for a Multiview (Slint-free, physical pixels relative to the host rectangle).
//!
//! The instance builds this description from its single state; the presenters of the hosts only convert it into their
//! own view structs, so no host keeps a second copy of the arrangement.

use super::follow::TileKey;
use super::layout::Rect;
use super::media_actions::{ButtonLook, MediaActionId};
use super::media_feedback::PulseView;
use super::model::TileKind;
use super::primary_actions::PrimaryActionId;
use crate::icons::Icon;
use crate::ui::button_feedback::ButtonFeedback;
use rustview_core::library::models::MediaId;
use slint::{Rgb8Pixel, SharedPixelBuffer};

pub struct TileView {
    pub key: TileKey,
    pub media_id: MediaId,
    pub rect: Rect,
    pub kind: TileKind,
    pub pixels: Option<SharedPixelBuffer<Rgb8Pixel>>,
    pub is_loading: bool,
    pub error: Option<String>,
    /// An image that is zoomed: what is drawn instead of the fitted `pixels`.
    pub zoom: Option<ZoomPictureView>,
    /// The minimap of a zoomed image, while it is shown.
    pub minimap: Option<MinimapView>,
    /// Dragging the picture moves something (the image is larger than its tile).
    pub pannable: bool,
    /// The feedback icon of an interaction (a rating just given, a save), while it is shown.
    pub feedback: Option<PulseView>,
    /// The crop tool, while it is open on this tile.
    pub crop: Option<CropView>,
    /// The picture has unsaved edits.
    pub dirty: bool,
    /// The marks of the selected medium, while this tile is the selected one of several.
    pub focus: Option<FocusMarkView>,
    /// The picture of a video, once its surface draws into the host's window.
    pub video: Option<VideoFrameView>,
}

/// Where the canvas draws the texture of a video (physical pixels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoFrameView {
    /// The row of the window's `video-frames` that holds the video's newest texture.
    pub row: usize,
    /// The frame mpv draws into (the size of the texture), relative to the tile: the room of the tile in the fullscreen and the
    /// tab view (mpv letterboxes into it), else the tile.
    pub frame: Rect,
    /// The part of the frame that shows (the medium's place, or the whole room while the video is zoomed), relative to the tile.
    pub clip: Rect,
}

/// The marks of the selected medium (physical pixels). The host brightens the view and draws the corner marks in its corners;
/// where a video's native window covers a corner, mpv draws the part of the mark inside it (`focus_overlay`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusMarkView {
    /// The view of the tile (its canvas, `host_room`: the column at the full height of its row), relative to the tile. The marks
    /// frame it, whatever the medium's size or zoom; it is brightened where the medium leaves room.
    pub view: Rect,
    /// White over the view, 0 (off) to 1.
    pub brighten: f32,
    /// Length and thickness of the corner marks.
    pub corner: i32,
    pub thickness: i32,
}

/// The crop tool on a tile (physical pixels).
#[derive(Debug, Clone, PartialEq)]
pub struct CropView {
    /// The part of the host the overlay covers, relative to the tile: the room of the picture (the tile and the free space
    /// around it, where a zoomed picture is drawn).
    pub clip: Rect,
    /// The whole picture, relative to `clip` (it may reach out of it when zoomed in).
    pub picture: (f64, f64, f64, f64),
    /// The crop rectangle, relative to `clip`.
    pub rect: (f64, f64, f64, f64),
    /// "893 x 1116 px".
    pub size_label: String,
    /// "O", "4:5", "Free".
    pub ratio_label: String,
    /// The rule-of-thirds lines are drawn.
    pub grid: bool,
    /// A handle or the rectangle is being dragged.
    pub dragging: bool,
}

/// The picture of a zoomed image: where it goes in its room (physical pixels) and what it is.
pub struct ZoomPictureView {
    /// The room the picture may use: the tile and the free space around it (physical pixels, relative to the tile, so it starts
    /// at zero or below).
    pub area: Rect,
    /// Where the picture goes, relative to `area`.
    pub dest: Rect,
    pub content: ZoomContentView,
    /// Hard pixels instead of smoothing (the nearest-neighbour filter is chosen).
    pub pixelated: bool,
}

pub enum ZoomContentView {
    /// The sharp, resampled picture, larger than what is shown: `clip` (its pixels) is the part that goes to the picture's place.
    Rendered {
        pixels: SharedPixelBuffer<Rgb8Pixel>,
        clip: Rect,
    },
    /// This rectangle of the tile's own picture, scaled by the renderer, until the sharp picture is ready.
    Preview { clip: Rect },
}

/// The minimap of a tile: its place (physical pixels relative to the tile), the thumbnail of the whole image and the part
/// that is visible (shares of the image: x, y, width, height).
pub struct MinimapView {
    /// Whether it is shown now (it fades in and out, so it is described while it is not).
    pub visible: bool,
    /// The room of the tile (physical pixels, relative to the tile) the minimap is in.
    pub area: Rect,
    /// Relative to the tile (physical pixels).
    pub rect: Rect,
    pub thumb: SharedPixelBuffer<Rgb8Pixel>,
    pub viewport: (f64, f64, f64, f64),
    /// The zoom slider bubble above the minimap, while the user has it open.
    pub bubble: Option<ZoomBubbleView>,
}

/// The zoom slider that hangs below the media bar: which tile it zooms and the numbers it needs (tenths of a percent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZoomBubbleView {
    pub tile: usize,
    pub key: super::follow::TileKey,
    pub permille: i32,
    pub min_permille: i32,
    pub max_permille: i32,
    /// The marks of the slider (where the handle snaps to): tenths of a percent * 4 + the rank of the mark (0 = reset, 1 and 2 =
    /// lines, 3 = dot), see `scale::Tick`.
    pub snaps: Vec<i32>,
}

/// One button of the hover bar of a tile.
#[derive(Debug, Clone, PartialEq)]
pub struct MediaActionView {
    pub id: MediaActionId,
    pub icon: Icon,
    pub tooltip: String,
    pub active: Option<bool>,
    pub enabled: bool,
    pub feedback: ButtonFeedback,
    pub look: ButtonLook,
    /// The button has an arrow of its own that opens a drop-down.
    pub split: bool,
}

pub struct GutterView {
    pub rect: Rect,
    pub vertical: bool,
    /// Only a swap button (no divider, no drag).
    pub swap_only: bool,
}

/// Whether previous/next navigation (keys and mouse wheel) applies to what the Multiview shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Navigation {
    /// Not applicable (several media, a video that keeps all keys for mpv, or an isolated tab snapshot).
    Off,
    /// A single image that follows the Browse selection.
    Allowed,
    /// A single sticky image: the request is refused (the sticky button flashes).
    Blocked,
}

pub struct HostedView {
    /// Changes whenever anything visible changed, so presenters can skip identical syncs.
    pub revision: u64,
    /// Scale factor of the host (physical pixels per logical pixel).
    pub dpi: f32,
    /// The projected container inside the host rectangle.
    pub screen: Rect,
    pub tiles: Vec<TileView>,
    pub gutters: Vec<GutterView>,
    pub navigation: Navigation,
}

/// A media bar the canvas of the host draws: the tile it belongs to, its place relative to the canvas (physical pixels) and the
/// buttons of the tile.
pub struct MediaBarTarget {
    pub tile: usize,
    /// The tile's stable key: the bar belongs to the tile, not to its place in the row (which changes when media come and go).
    pub key: super::follow::TileKey,
    pub bar: Rect,
    pub actions: Vec<MediaActionView>,
    /// The bar stays whatever the pointer does (the tile has its zoom slider or a drop-down open), and follows its tile.
    pub forced: bool,
}

/// One button of the primary action bar.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimaryActionView {
    pub id: PrimaryActionId,
    pub icon: Icon,
    pub tooltip: String,
    pub enabled: bool,
    pub active: Option<bool>,
    /// The button blinks (a refused attempt to close a pinned fullscreen).
    pub attention: bool,
}

/// The primary bar the canvas of the host draws: its place in the top left corner of the Multiview relative to the canvas
/// (physical pixels) and its buttons.
pub struct PrimaryBarTarget {
    pub bar: Rect,
    pub actions: Vec<PrimaryActionView>,
}
