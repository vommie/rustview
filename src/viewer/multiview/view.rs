//! What a host has to draw for a Multiview (Slint-free, physical pixels relative to the host rectangle).
//!
//! The instance builds this description from its single state; the presenters of the hosts only convert it into their
//! own view structs, so no host keeps a second copy of the arrangement.

use super::layout::Rect;
use super::model::TileKind;
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

pub struct GutterView {
    pub rect: Rect,
    pub vertical: bool,
}

pub struct BadgeView {
    pub rect: Rect,
    pub muted: bool,
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
    pub badge: Option<BadgeView>,
}
