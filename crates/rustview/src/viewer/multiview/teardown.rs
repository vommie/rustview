//! Ordered destruction of video tiles.

use super::tile::VideoTile;
use crate::viewer::install_nonfatal_error_handler;
use crate::viewer::mpv_engine::MpvPlayer;
use crate::viewer::x11::X11VideoSubwindow;
use parking_lot::{Condvar, Mutex};
use rustview_core::settings::schema::PLAYER_TEARDOWN_WAIT_MS;
use std::sync::Arc;
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

/// Shuts one player down completely (its video output gone) and only then destroys the window it rendered into. Must be
/// called with the last reference to the player.
pub fn teardown_video(player: Arc<MpvPlayer>, window: Option<X11VideoSubwindow>) {
    player.shutdown_blocking();
    drop(window);
}

/// A closed video tab: its player is shut down and its window destroyed on a worker thread, in that order. The thread first waits
/// until nothing else holds the player (the UI may still use a short-lived reference). If something still does after the wait,
/// the window is left alone (the X server removes it with the connection) rather than destroyed under a running video output,
/// which would end the application.
pub fn teardown_video_deferred(player: Arc<MpvPlayer>, window: Option<X11VideoSubwindow>) {
    let guard = RunningGuard::enter();
    let spawned = std::thread::Builder::new()
        .name("video-tab-teardown".into())
        .spawn(move || {
            let _guard = guard;
            let deadline = Instant::now() + Duration::from_millis(PLAYER_TEARDOWN_WAIT_MS);
            while Arc::strong_count(&player) > 1 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            if Arc::strong_count(&player) > 1 {
                tracing::warn!(
                    "[Multiview: Teardown] A closed video tab's player is still referenced; its window is left alone"
                );
                std::mem::forget(window);
                return;
            }
            teardown_video(player, window);
            install_nonfatal_error_handler();
            tracing::debug!("[Multiview: Teardown] Video tab player shut down before its window was destroyed");
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "[Multiview: Teardown] Failed to spawn the video tab teardown thread");
    }
}

/// Destroys video tiles on a worker thread. Every player is shut down completely (its video output gone) BEFORE its X11
/// window is destroyed; destroying the window first lets the still-running video output hit `BadWindow`, and libmpv resets
/// the process-wide Xlib error handler to one that terminates the application.
pub fn teardown_videos(videos: Vec<VideoTile>) {
    if videos.is_empty() {
        return;
    }
    let count = videos.len();
    let guard = RunningGuard::enter();
    let spawned = std::thread::Builder::new()
        .name("multiview-teardown".into())
        .spawn(move || {
            let _guard = guard;
            teardown_all(videos);
            tracing::debug!(
                count,
                "[Multiview: Teardown] Video players shut down before their windows were destroyed"
            );
        });
    if let Err(e) = spawned {
        tracing::error!(error = %e, "[Multiview: Teardown] Failed to spawn the teardown thread; dropping tiles inline");
    }
}

/// The same destruction on the calling thread, for the application's exit: nothing is left to a thread that the end of the
/// process would cut short.
pub fn teardown_videos_now(videos: Vec<VideoTile>) {
    if videos.is_empty() {
        return;
    }
    let count = videos.len();
    teardown_all(videos);
    tracing::debug!(
        count,
        "[Multiview: Teardown] Video players shut down before their windows were destroyed (blocking)"
    );
}

fn teardown_all(videos: Vec<VideoTile>) {
    for VideoTile { player, window, .. } in videos {
        teardown_video(player, Some(window));
    }
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
