use crate::viewer::{MediaTabSession, TabId};
use rustview_core::library::models::MediaId;
use rustview_core::util::cast::Az;
use slint::{Rgb8Pixel, SharedPixelBuffer};

/// High-level layout item for an individual tab inside the `TabGridView`.
#[derive(Debug, Clone)]
pub struct TabGridTile {
    pub tab_id: TabId,
    pub media_id: MediaId,
    pub title: String,
    pub format_str: String,
    pub is_video: bool,
    pub rating: u8,
    pub has_tags: bool,
    pub orig_width: u32,
    pub orig_height: u32,
    pub aspect_ratio: f32,
    pub high_res_image: Option<SharedPixelBuffer<Rgb8Pixel>>,
    pub is_loading: bool,
    pub error_message: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl TabGridTile {
    /// Aspect ratio of the content area of a tab: its medium, or the virtual display of a Multiview.
    pub fn aspect_of(tab: &MediaTabSession) -> f32 {
        let (width, height) = tab.display_size();
        let is_video = tab
            .multiview
            .instance
            .lock()
            .solo_tile()
            .is_some_and(|(_, kind, _)| kind == crate::viewer::multiview::model::TileKind::Video);
        Self::compute_aspect_ratio(width, height, is_video)
    }

    /// Computes safe aspect ratio (width / height) clamped to prevent extreme geometric distortion.
    pub fn compute_aspect_ratio(orig_w: u32, orig_h: u32, is_video: bool) -> f32 {
        if orig_w > 0 && orig_h > 0 {
            let ar = orig_w.az::<f32>() / orig_h.az::<f32>();
            ar.clamp(0.28, 3.5)
        } else if is_video {
            16.0 / 9.0
        } else {
            4.0 / 3.0
        }
    }
}
