//! Ordered destruction of video tiles.

use super::tile::VideoTile;
use crate::viewer::install_nonfatal_error_handler;
use parking_lot::{Condvar, Mutex};
use std::time::{Duration, Instant};

/// Number of teardown threads that are running, so that the application can wait for them before it exits.
static RUNNING: Mutex<usize> = Mutex::new(0);
static IDLE: Condvar = Condvar::new();

/// Counts a teardown thread for as long as it lives.
struct RunningGuard;

impl RunningGuard {
    fn enter() -> Self {
        *RUNNING.lock() += 1;
        Self
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        let mut running = RUNNING.lock();
        *running = running.saturating_sub(1);
        if *running == 0 {
            IDLE.notify_all();
        }
    }
}

/// Ends one video: its surface first (the render thread frees mpv's render context, which libmpv requires before the core is
/// destroyed, and deletes the textures), then the player.
fn teardown_video(video: VideoTile) {
    let VideoTile { player, surface, .. } = video;
    drop(surface);
    player.shutdown_blocking();
}

/// Lets the surfaces of `videos` stop drawing into their windows (UI thread): the rows of the canvases show nothing from now on, so
/// no window draws a texture after the render thread deleted it.
fn detach_all(videos: &mut [VideoTile]) {
    for video in videos {
        video.surface.detach(&video.player);
    }
}

/// Destroys video tiles on a worker thread (call it on the UI thread: the surfaces are detached from their windows here first).
pub fn teardown_videos(mut videos: Vec<VideoTile>) {
    if videos.is_empty() {
        return;
    }
    detach_all(&mut videos);
    let count = videos.len();
    let guard = RunningGuard::enter();
    let spawned = std::thread::Builder::new()
        .name("multiview-teardown".into())
        .spawn(move || {
            let _guard = guard;
            teardown_all(videos);
            tracing::debug!(
                count,
                "[Multiview: Teardown] Video surfaces released, then their players shut down"
            );
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "[Multiview: Teardown] Failed to spawn the teardown thread; dropping tiles inline");
    }
}

/// The same destruction on the calling thread, for the application's exit: nothing is left to a thread that the end of the
/// process would cut short.
pub fn teardown_videos_now(mut videos: Vec<VideoTile>) {
    if videos.is_empty() {
        return;
    }
    detach_all(&mut videos);
    let count = videos.len();
    teardown_all(videos);
    tracing::debug!(
        count,
        "[Multiview: Teardown] Video surfaces released, then their players shut down (blocking)"
    );
}

fn teardown_all(videos: Vec<VideoTile>) {
    for video in videos {
        teardown_video(video);
    }
    // libmpv may install its process-killing Xlib error handler when a hardware decoder's interop used Xlib.
    install_nonfatal_error_handler();
}

/// Waits until every teardown thread has finished, at most `limit`. Returns whether none is left.
pub fn wait_for_teardowns(limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    let mut running = RUNNING.lock();
    while *running > 0 {
        if IDLE.wait_until(&mut running, deadline).timed_out() {
            break;
        }
    }
    let left = *running;
    if left > 0 {
        tracing::warn!(
            left,
            "[Multiview: Teardown] Teardown threads were still running at the deadline"
        );
    }
    left == 0
}
