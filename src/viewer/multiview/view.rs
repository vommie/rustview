//! What a host has to draw for a Multiview (Slint-free, physical pixels relative to the host rectangle).
//!
//! The instance builds this description from its single state; the presenters of the hosts only convert it into their
//! own view structs, so no host keeps a second copy of the arrangement.

use super::media_actions::MediaActionId;
use super::layout::Rect;
use super::model::TileKind;
use crate::ui::button_feedback::ButtonFeedback;
use crate::icons::Icon;
use crate::library::models::MediaId;
use slint::{Rgb8Pixel, SharedPixelBuffer};

pub struct TileView {
    pub media_id: MediaId,
    pub rect: Rect,
    pub kind: TileKind,
    pub pixels: Option<SharedPixelBuffer<Rgb8Pixel>>,
    pub is_loading: bool,
    pub error: Option<String>,
}

/// One button of the hover bar of a tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaActionView {
    pub id: MediaActionId,
    pub icon: Icon,
    pub tooltip: &'static str,
    pub active: Option<bool>,
    pub feedback: ButtonFeedback,
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

/// The hover bar a host may show: the tile the pointer is on, where that tile is in the host window (physical pixels),
/// the pointer (host window coordinates) and the buttons the tile offers. The bar only appears while the pointer is
/// within `reveal_distance_px` (logical pixels) of the place where it would be.
pub struct MediaBarTarget {
    pub tile: usize,
    pub rect: Rect,
    pub actions: Vec<MediaActionView>,
    pub pointer: (i32, i32),
    pub reveal_distance_px: u32,
}
