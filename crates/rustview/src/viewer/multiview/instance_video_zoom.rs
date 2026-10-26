//! The zoom of a video tile: the same model as a picture's (`TileState::zoom`, `instance_zoom`: wheel at the pointer, dragging,
//! minimap, slider, keys), drawn by mpv. Here the model's picture becomes mpv's view (`viewer::zoom::mpv_view`), the visible part
//! of the video grows into the room of its tile while zoomed (like a zoomed picture; mpv's frame, the texture, always covers the
//! room), and the minimap gets stills of the video (mpv's `screenshot-raw`, on a worker thread). The canvas draws the minimap
//! over the video.

use super::events::PlayerEvent;
use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::model::TileKind;
use super::route::HostMode;
use super::tile::VideoTile;
use super::video_placement::VideoPlacement;
use crate::viewer::zoom::mpv_view::{self, MpvView};
use crate::viewer::zoom::state::ZoomState;
use rustview_core::settings::schema::MULTIVIEW_MIN_VIDEO_WINDOW_PX;
use rustview_core::util::cast::SaturatingAs;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::time::{Duration, Instant};

/// While the minimap of a playing video shows, its still is taken again this often.
const STILL_REFRESH: Duration = Duration::from_secs(2);

impl MultiviewInstance {
    /// Gives video tile `index` its zoom once mpv knows the size of the picture (`width` x `height` as displayed).
    pub(super) fn ensure_video_zoom(&mut self, index: usize, size: (u32, u32)) {
        let Some(tile) = self.tiles.get_mut(index) else { return };
        if tile.kind != TileKind::Video || tile.zoom.as_ref().is_some_and(|z| z.image() == size) {
            return;
        }
        tile.zoom = Some(ZoomState::new_video(size));
        tracing::debug!(media_id = tile.media_id, ?size, "[Zoom: Video] Zoom ready");
        self.sync_zoom_frames();
    }

    /// Where the video of tile `index` shows: the room of the tile while the video is zoomed (it uses the free space
    /// around it, like a picture), else the place of the medium.
    pub(super) fn video_window_rect(&self, index: usize) -> Option<Rect> {
        let tile = self.tiles.get(index)?;
        let zoomed = matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView)
            && tile.zoom.as_ref().is_some_and(ZoomState::is_active);
        if zoomed {
            self.host_room(index)
        } else {
            self.host_content(index)
        }
    }

    /// Where the video of tile `index` goes (`video_placement`): in the fullscreen and the tab view mpv's frame (the texture)
    /// always covers the room of the tile and the canvas clips it to `video_window_rect`, so zooming never resizes it; elsewhere
    /// both are the place of the medium.
    pub(super) fn video_placement(&self, index: usize) -> Option<VideoPlacement> {
        let window = self.video_window_rect(index)?;
        let frame = if matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView) {
            self.host_room(index)?
        } else {
            window
        };
        Some(VideoPlacement::new(frame, window))
    }

    /// Brings every video in line with its zoom: what of its frame shows (the room or its place), mpv's view and the stills of the
    /// minimap. Only what changed reaches mpv. Called after every change of a zoom and on every tick of the host.
    pub(super) fn sync_video_zoom(&mut self, now: Instant) {
        let interactive = matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView);
        let mut moved_any = false;
        let mut stills: Vec<usize> = Vec::new();
        for index in 0..self.tiles.len() {
            if self.tiles[index].kind != TileKind::Video {
                continue;
            }
            let placement = self.video_placement(index);
            let home = self.host_content(index);
            let tile = &mut self.tiles[index];
            let (Some(video), zoom) = (tile.video.as_mut(), tile.zoom.as_ref()) else {
                continue;
            };
            let view = match (placement, home) {
                (Some(placement), Some(home)) if interactive => view_in(zoom, &placement, home),
                _ => MpvView::FIT,
            };
            let moved = placement
                .filter(|p| interactive && video.placement != Some(*p))
                .filter(|p| p.window.w >= MULTIVIEW_MIN_VIDEO_WINDOW_PX && p.window.h >= MULTIVIEW_MIN_VIDEO_WINDOW_PX);
            // mpv draws its next frame on its own thread: a clip that grows waits for the new view, one that shrinks goes first,
            // so the moment in between shows a little less of the new picture instead of mpv's background. (The frame keeps its
            // size, so the texture is never stretched.)
            let view_first = moved.is_some_and(|p| p.grows_from(video.placement.as_ref()));
            if view_first {
                apply_view(video, &view);
            }
            if let Some(placement) = moved {
                place_video(video, placement);
                moved_any = true;
            }
            if !view_first {
                apply_view(video, &view);
            }
            let wants_still = zoom.is_some_and(|z| z.is_active() || z.minimap_pinned())
                && !video.still_pending
                && (zoom.is_some_and(|z| z.thumb().is_none())
                    || (tile.zoom_minimap_shown
                        && !video.player.playback().paused
                        && video
                            .still_at
                            .is_none_or(|at| now.saturating_duration_since(at) >= STILL_REFRESH)));
            if wants_still && interactive {
                video.still_pending = true;
                video.still_at = Some(now);
                stills.push(index);
            }
        }
        if moved_any {
            self.bump();
        }
        for index in stills {
            self.request_still(index);
        }
    }

    /// Takes a still of the video of tile `index` on a worker thread; it comes back as `PlayerEvent::VideoStill`.
    fn request_still(&self, index: usize) {
        let Some((key, player)) = self
            .tiles
            .get(index)
            .and_then(|t| Some((t.key, t.video.as_ref()?.player.clone())))
        else {
            return;
        };
        let (sink, me) = (self.sink.clone(), self.me.clone());
        let spawned = std::thread::Builder::new().name("video-still".into()).spawn(move || {
            let result = player.still();
            sink.post(me, PlayerEvent::VideoStill { tile: key, result });
        });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[Zoom: Video] The still thread could not be started");
        }
    }

    /// A still of the video of tile `key` arrived (or could not be taken): it becomes the picture of its minimap.
    pub fn on_video_still(&mut self, key: TileKey, result: Result<SharedPixelBuffer<Rgb8Pixel>, String>) {
        let Some(index) = self.tile_index(key) else { return };
        let tile = &mut self.tiles[index];
        if let Some(video) = tile.video.as_mut() {
            video.still_pending = false;
        }
        match result {
            Ok(still) => {
                if let Some(zoom) = tile.zoom.as_mut() {
                    zoom.replace_thumb(&still);
                }
            }
            Err(e) => tracing::debug!(error = %e, "[Zoom: Video] No still for the minimap"),
        }
        self.bump();
    }
}

/// The view that puts the picture where the zoom wants it inside the frame of `placement` (at 100 % its fitted place `home`,
/// the medium's place in host pixels, which is also the answer while mpv has not reported the size yet).
fn view_in(zoom: Option<&ZoomState>, placement: &VideoPlacement, home: Rect) -> MpvView {
    let frame = (placement.frame.w, placement.frame.h);
    zoom.and_then(|z| mpv_view::view_for(z.image(), frame, z.image_rect()?))
        .unwrap_or_else(|| placement.home_view(home))
}

/// Hands `view` to mpv unless it shows that already.
fn apply_view(video: &mut VideoTile, view: &MpvView) {
    if !video.zoom_view.unwrap_or(MpvView::FIT).same_as(view) {
        video.player.set_view(view);
        video.zoom_view = Some(*view);
    }
}

/// Gives the texture of `video` the size of the frame of `placement` and records where it shows.
pub(super) fn place_video(video: &mut VideoTile, placement: VideoPlacement) {
    let frame = placement.frame;
    video.surface.set_size(
        frame.w.max(1).saturating_as::<u32>(),
        frame.h.max(1).saturating_as::<u32>(),
    );
    note_placement(video, placement);
}

/// Records where `video` shows now, and lets subtitles use the black borders only while the whole frame shows.
pub(super) fn note_placement(video: &mut VideoTile, placement: VideoPlacement) {
    let whole = placement.shows_all();
    if video.placement.is_none_or(|old| old.shows_all() != whole) {
        video.player.set_subtitle_margins(whole);
    }
    video.placement = Some(placement);
}
