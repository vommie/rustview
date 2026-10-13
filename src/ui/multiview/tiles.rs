//! Runtime state of one Multiview tile and the creation/teardown of its native video window.
//!
//! A video tile is set up exactly like the standalone video of the single quick fullscreen: an `X11VideoSubwindow` below the
//! fullscreen window, an `MpvPlayer` attached to it, the same callbacks, `load_file` + `play`. mpv does whatever it does
//! inside its rectangle; the layout only decides where that rectangle is.

use super::audio::TileAudio;
use super::handle::MultiviewHandle;
use super::layout::Rect;
use super::model::{AspectSource, TileKind};
use crate::library::models::MediaId;
use crate::viewer::{MpvPlayer, X11VideoSubwindow, install_nonfatal_error_handler};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct VideoTile {
    pub player: Arc<MpvPlayer>,
    pub window: X11VideoSubwindow,
    /// Mute state mpv started with (user config); `Native` audio restores it.
    pub native_mute: bool,
    /// Last mute state known, to tell our own changes from changes made inside mpv (`m`, OSC).
    pub last_mute: bool,
}

pub struct TileRuntime {
    pub media_id: MediaId,
    pub kind: TileKind,
    pub filename: String,
    pub path: PathBuf,
    pub aspect: f32,
    pub aspect_source: AspectSource,
    pub image: Option<slint::Image>,
    pub is_loading: bool,
    pub error: Option<String>,
    pub video: Option<VideoTile>,
    /// `None` for images (no sound).
    pub audio: Option<TileAudio>,
}

fn connect_callbacks(player: &MpvPlayer, handle: &MultiviewHandle, media_id: MediaId) {
    let h = handle.clone();
    player.on_toggle_fullscreen(move || h.request_close());
    let h = handle.clone();
    player.on_close_tab(move || h.request_close());
    let h = handle.clone();
    player.on_rename_requested(move || h.request_rename(media_id));
    let h = handle.clone();
    player.on_convert_to_background_tab(move || h.video_ctrl_click());
}

/// Creates the video window inside `rect` (coordinates of the fullscreen window) and starts playback.
pub fn create_video(
    handle: &MultiviewHandle,
    parent_xid: u64,
    rect: Rect,
    media_id: MediaId,
    path: &Path,
) -> Result<VideoTile, String> {
    let mut window = X11VideoSubwindow::new(
        parent_xid,
        rect.x,
        rect.y,
        rect.w.max(16) as u32,
        rect.h.max(16) as u32,
    )
    .map_err(|e| e.to_string())?;
    let wid = window.xid();
    window.show();

    let player = Arc::new(MpvPlayer::new().map_err(|e| e.to_string())?);
    connect_callbacks(&player, handle, media_id);

    let started = player.attach_to_window(wid).and_then(|()| {
        player.set_tab_grid_mode(false);
        player.set_view_active(true);
        player.set_user_paused(false);
        player.load_file(path)?;
        player.play()
    });
    if let Err(e) = started {
        player.shutdown_blocking();
        install_nonfatal_error_handler();
        return Err(e.to_string());
    }

    let native_mute = player.get_mute();
    Ok(VideoTile {
        player,
        window,
        native_mute,
        last_mute: native_mute,
    })
}

/// Destroys video tiles on a worker thread. Every player is shut down completely (its video output gone) BEFORE its X11
/// window is destroyed; destroying the window first lets the still-running video output hit `BadWindow`, and libmpv resets
/// the process-wide Xlib error handler to one that terminates the application.
pub fn teardown_videos(videos: Vec<VideoTile>) {
    if videos.is_empty() {
        return;
    }
    let count = videos.len();
    let spawned = std::thread::Builder::new()
        .name("multiview-teardown".into())
        .spawn(move || {
            for VideoTile { player, window, .. } in videos {
                player.shutdown_blocking();
                drop(window);
            }
            install_nonfatal_error_handler();
            tracing::debug!(
                count,
                "[Multiview: Teardown] Video players shut down before their windows were destroyed"
            );
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "[Multiview: Teardown] Failed to spawn the teardown thread; dropping tiles inline");
    }
}
