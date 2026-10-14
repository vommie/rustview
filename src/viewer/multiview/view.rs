//! What a host has to draw for a Multiview (Slint-free, physical pixels relative to the host rectangle).
//!
//! The instance builds this description from its single state; the presenters of the hosts only convert it into their
//! own view structs, so no host keeps a second copy of the arrangement.

use super::actions::ActionId;
use super::layout::Rect;
use super::model::TileKind;
use crate::button_feedback::ButtonFeedback;
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
    /// The buttons of the hover bar of this tile.
    pub actions: Vec<ActionView>,
}

/// One button of the hover bar of a tile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActionView {
    pub id: ActionId,
    pub icon: Icon,
    pub tooltip: &'static str,
    pub active: Option<bool>,
    pub feedback: ButtonFeedback,
}

pub struct GutterView {
    pub rect: Rect,
    pub vertical: bool,
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
    /// The tile whose hover bar is shown right now (the pointer is on it and has been active recently).
    pub bar_tile: Option<usize>,
    pub navigation: Navigation,
}
