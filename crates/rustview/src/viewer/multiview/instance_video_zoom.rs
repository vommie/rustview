//! The zoom of a video tile: the same model as a picture's (`TileState::zoom`, `instance_zoom`: wheel at the pointer, dragging,
//! minimap, slider, keys). mpv renders the whole picture into the video's texture and the canvas draws it where the model puts the
//! picture, clipped to the room of the tile (`video_placement`); the minimap shows the same texture in small. Here every video
//! gets the texture size its placement needs.

use super::instance::MultiviewInstance;
use super::model::TileKind;
use super::route::HostMode;
use super::video_placement::{self, VideoPlacement};
use crate::viewer::zoom::state::ZoomState;
use rustview_core::settings::schema::MULTIVIEW_MIN_VIDEO_WINDOW_PX;
use rustview_core::util::cast::Az;

impl MultiviewInstance {
    /// Gives video tile `index` its zoom once mpv knows the size of the picture (`width` x `height` as displayed). A zoom mpv did
    /// itself before (`MpvPlayer::zoom_video_by`, while the size was unknown) is undone: from now on the canvas zooms.
    pub(super) fn ensure_video_zoom(&mut self, index: usize, size: (u32, u32)) {
        let Some(tile) = self.tiles.get_mut(index) else { return };
        if tile.kind != TileKind::Video || tile.zoom.as_ref().is_some_and(|z| z.image() == size) {
            return;
        }
        if tile.zoom.is_none()
            && let Some(video) = tile.video.as_ref()
        {
            video.player.reset_video_zoom();
        }
        tile.zoom = Some(ZoomState::new_video(size));
        tracing::debug!(media_id = tile.media_id, ?size, "[Zoom: Video] Zoom ready");
        self.sync_zoom_frames();
    }

    /// Where the video of tile `index` is drawn and what of it shows (host pixels): in the fullscreen and the tab view a zoomed
    /// video is drawn where its zoom puts the picture and shows in the room of its tile (like a picture), else at the medium's
    /// place.
    pub(super) fn video_placement(&self, index: usize) -> Option<VideoPlacement> {
        let tile = self.tiles.get(index)?;
        let content = self.host_content(index)?;
        let interactive = matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView);
        let zoomed = tile
            .zoom
            .as_ref()
            .filter(|z| interactive && z.is_active())
            .and_then(ZoomState::image_rect)
            .zip(self.host_room(index));
        Some(match zoomed {
            Some((image, room)) => VideoPlacement::zoomed(room, image),
            None => VideoPlacement::plain(content, video_aspect(tile)),
        })
    }

    /// Gives every video's textures the size its placement needs (`VideoPlacement::texture_size`; a video that would show less
    /// than `MULTIVIEW_MIN_VIDEO_WINDOW_PX` draws nothing). While a zoom animates a texture keeps its size until it falls far
    /// behind. Called after the layout changed and on every poll of the zoom.
    pub(super) fn size_video_textures(&mut self) {
        for index in 0..self.tiles.len() {
            let placement = self.video_placement(index);
            let tile = &mut self.tiles[index];
            let native = tile.zoom.as_ref().map(ZoomState::image);
            let animating = tile.zoom.as_ref().is_some_and(ZoomState::animating);
            let Some(video) = tile.video.as_mut() else {
                continue;
            };
            let Some(placement) = placement else {
                continue;
            };
            let shows = placement
                .visible()
                .is_some_and(|v| v.w >= MULTIVIEW_MIN_VIDEO_WINDOW_PX && v.h >= MULTIVIEW_MIN_VIDEO_WINDOW_PX);
            let wanted = if shows { placement.texture_size(native) } else { (0, 0) };
            if video_placement::keeps_texture(video.texture, wanted, animating && shows) {
                continue;
            }
            video.surface.set_size(wanted.0, wanted.1);
            video.texture = wanted;
        }
    }
}

/// The aspect of the video of `tile` (width / height as displayed): exact once mpv reported its size, else what the tile knows.
fn video_aspect(tile: &super::tile::TileState) -> Option<f32> {
    match tile.zoom.as_ref().map(ZoomState::image) {
        Some((w, h)) if w > 0 && h > 0 => Some(w.az::<f32>() / h.az::<f32>()),
        _ => (tile.aspect.is_finite() && tile.aspect > 0.0).then_some(tile.aspect),
    }
}
