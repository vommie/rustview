//! Ordered destruction of video tiles.

use super::tile::VideoTile;
use crate::viewer::install_nonfatal_error_handler;

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
