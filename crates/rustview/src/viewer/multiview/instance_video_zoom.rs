//! The zoom of a video tile: the same model as a picture's (`TileState::zoom`, `instance_zoom`: wheel at the pointer, dragging,
//! minimap, slider, keys), drawn by mpv. Here the model's picture becomes mpv's view (`viewer::zoom::mpv_view`), the video window
//! grows into the room of its tile while zoomed (like a zoomed picture), the minimap gets stills of the video (mpv's
//! `screenshot-raw`, on a worker thread) and is cut out of the video window, which would cover it (the canvas draws it below).

use super::events::PlayerEvent;
use super::follow::TileKey;
use super::instance::MultiviewInstance;
use super::layout::Rect;
use super::model::TileKind;
use super::route::HostMode;
use crate::viewer::XRectangle;
use crate::viewer::multiview::bar_placement::xrectangle;
use crate::viewer::zoom::mpv_view::{self, MpvView};
use crate::viewer::zoom::prefs as zoom_prefs;
use crate::viewer::zoom::state::ZoomState;
use rustview_core::util::cast::FloatToInt;
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::time::{Duration, Instant};

/// While the minimap of a playing video shows, its still is taken again this often.
const STILL_REFRESH: Duration = Duration::from_secs(2);

/// The slider bubble on the minimap (`multiview_canvas.slint`): its size, the gap to the minimap and the room for the percentage
/// floating above it, in logical pixels.
const BUBBLE_WIDTH: f32 = 210.0;
const BUBBLE_HEIGHT: f32 = 43.0;
const BUBBLE_GAP: f32 = 4.0;
const BUBBLE_PILL: f32 = 26.0;

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

    /// Where the window of the video of tile `index` goes: the room of the tile while the video is zoomed (it uses the free space
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

    /// Brings every video in line with its zoom: the window in its room (or back on its place), mpv's view, the cutouts of the
    /// minimap and the stills it shows. Only what changed reaches mpv and the X server. Called after every change of a zoom and
    /// on every tick of the host.
    pub(super) fn sync_video_zoom(&mut self, now: Instant) {
        let interactive = matches!(self.host.surface.mode, HostMode::Fullscreen | HostMode::TabView);
        let mut recut: Vec<usize> = Vec::new();
        let mut stills: Vec<usize> = Vec::new();
        for index in 0..self.tiles.len() {
            if self.tiles[index].kind != TileKind::Video {
                continue;
            }
            let window = self.video_window_rect(index);
            let cut = if interactive {
                self.video_cutouts(index)
            } else {
                Vec::new()
            };
            let tile = &mut self.tiles[index];
            let (Some(video), zoom) = (tile.video.as_mut(), tile.zoom.as_ref()) else {
                continue;
            };
            let view = match (zoom, window) {
                (Some(zoom), Some(window)) if interactive && zoom.is_active() => zoom
                    .image_rect()
                    .and_then(|rect| mpv_view::view_for(zoom.image(), (window.w, window.h), rect))
                    .unwrap_or(MpvView::FIT),
                _ => MpvView::FIT,
            };
            if !video.zoom_view.unwrap_or(MpvView::FIT).same_as(&view) {
                video.player.set_view(&view);
                video.zoom_view = Some(view);
            }
            if let Some(window) = window
                && video.zoom_window != Some(window)
                && interactive
            {
                video.window.move_resize(
                    window.x,
                    window.y,
                    window.w.max(1).saturating_as_u32(),
                    window.h.max(1).saturating_as_u32(),
                );
                video.zoom_window = Some(window);
                recut.push(index);
            }
            if video.minimap_cut != cut {
                video.minimap_cut = cut;
                recut.push(index);
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
        recut.dedup();
        for index in recut {
            self.apply_cutouts(index);
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

    /// What has to show through the window of video tile `index` (host pixels): its minimap while it shows, and the slider bubble
    /// on it while that is open. The canvas draws them below the native window.
    pub(super) fn video_cutouts(&self, index: usize) -> Vec<XRectangle> {
        let prefs = zoom_prefs::current();
        let (Some(minimap), Some(content)) = (self.minimap_rects(index, &prefs), self.host_content(index)) else {
            return Vec::new();
        };
        let (mm, bubble) = minimap;
        let shift = |r: Rect| xrectangle(Rect::new(content.x + r.x, content.y + r.y, r.w, r.h));
        let mut cut = vec![shift(mm)];
        cut.extend(bubble.map(shift));
        cut
    }

    /// The minimap of tile `index` while it shows (relative to the tile's place, physical pixels), and the bubble on it if open.
    fn minimap_rects(&self, index: usize, prefs: &zoom_prefs::ZoomPrefs) -> Option<(Rect, Option<Rect>)> {
        let view = self.minimap_view_of(index, prefs)?;
        if !view.visible {
            return None;
        }
        let dpi = self.host.surface.dpi.max(0.5);
        let px = |logical: f32| (logical * dpi).round().to_int::<i32>();
        let bubble = view.bubble.as_ref().map(|_| {
            let (area, mm) = (view.area, view.rect);
            let w = px(BUBBLE_WIDTH);
            let x = (mm.x + mm.w / 2 - w / 2).min(area.x + area.w - w).max(area.x);
            let top = mm.y - px(BUBBLE_GAP + BUBBLE_HEIGHT + BUBBLE_PILL);
            Rect::new(x, top, w, px(BUBBLE_GAP + BUBBLE_HEIGHT + BUBBLE_PILL))
        });
        Some((view.rect, bubble))
    }
}

/// Window sizes are physical pixels and never negative.
trait SaturatingU32 {
    fn saturating_as_u32(self) -> u32;
}

impl SaturatingU32 for i32 {
    fn saturating_as_u32(self) -> u32 {
        u32::try_from(self.max(0)).unwrap_or(0)
    }
}
