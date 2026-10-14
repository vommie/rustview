//! A normal image or video tab shown in the fullscreen: it becomes a Multiview of one tile that stays isolated from the
//! Browse view (origin `Tab`: no following, no previous/next, the main window is locked while it is shown).
//!
//! An image tab hands over its decoded pixels (the decode of a tab that is still loading starts with the fullscreen). A video
//! tab lends its running player and window, so the position and the playback settings carry over; `return_loans` of the
//! instance gives them back when the fullscreen closes.

use super::handle::MultiviewHandle;
use super::router::UiEventSink;
use crate::settings::schema::MULTIVIEW_FALLBACK_ASPECT;
use crate::viewer::multiview::instance::{MultiviewInstance, SharedInstance};
use crate::viewer::multiview::model::{TileKind, aspect_from_size};
use crate::viewer::multiview::route::OriginKind;
use crate::viewer::multiview::tile::{TileSeed, VideoTile};
use crate::viewer::{MediaTabSession, TabPayload};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

/// The one-tile Multiview for `tab`, or `None` if the tab holds a Multiview itself (that one is shown as it is).
pub(super) fn instance_for_tab(
    handle: &MultiviewHandle,
    tab: &MediaTabSession,
) -> Option<SharedInstance> {
    let kind = match tab.payload {
        TabPayload::Image(_) => TileKind::Image,
        TabPayload::Video(_) => TileKind::Video,
        TabPayload::Multiview(_) => return None,
    };
    let media_id = tab.media_id;
    let seed = TileSeed {
        media_id,
        kind,
        aspect: aspect_from_size(tab.record.width, tab.record.height, MULTIVIEW_FALLBACK_ASPECT),
        path: tab.file_path.clone(),
        filename: tab.filename.clone(),
    };
    let instance = MultiviewInstance::create(Arc::new(UiEventSink::new(handle.clone())), vec![seed]);
    {
        let mut guard = instance.lock();
        guard.set_origin(OriginKind::Tab);
        guard.set_single_tab(true);
        match &tab.payload {
            TabPayload::Image(image) => {
                if !image.error_message.is_empty() {
                    guard.on_image_loaded(media_id, Err(image.error_message.clone()));
                } else if let Some(pixels) = &image.high_res_pixels {
                    guard.on_image_loaded(
                        media_id,
                        Ok((image.orig_width, image.orig_height, pixels.clone())),
                    );
                }
            }
            TabPayload::Video(video) => {
                let window = if video.error_message.is_empty() {
                    video.subwindow.lock().take()
                } else {
                    None
                };
                match window {
                    Some(window) => {
                        let native_mute = video.player.get_mute();
                        guard.lend_video(
                            media_id,
                            VideoTile {
                                player: video.player.clone(),
                                window,
                                native_mute,
                                last_mute: native_mute,
                                current_media: Arc::new(AtomicU64::new(media_id)),
                                loan: Some(video.subwindow.clone()),
                            },
                        );
                    }
                    None => guard.fail_tile(
                        media_id,
                        format!("This video could not be played\n{}", video.error_message),
                    ),
                }
            }
            TabPayload::Multiview(_) => {}
        }
    }
    Some(instance)
}
