//! The video surface of one player: its render thread and where it is shown.

use super::exchange::{Channel, ReadyAction};
use super::hosts::{self, HostWindowId};
use super::render_thread;
use crate::viewer::mpv_engine::MpvPlayer;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

static NEXT_SURFACE: AtomicU64 = AtomicU64::new(1);

/// How long a move to another window waits for the render thread to keep the last picture: mpv may hold the thread up to one
/// frame's display time; without the still the new window shows black until its first frame.
const STILL_WAIT: Duration = Duration::from_millis(150);

/// Draws one player into textures of the window it is attached to. Attach, detach and the actions run on the UI thread; the
/// surface may be dropped anywhere (its render thread is ended and joined), but the host should detach it on the UI thread first
/// so that its row stops showing the textures before they are deleted.
pub struct VideoSurface {
    id: u64,
    channel: Arc<Channel>,
    thread: Option<JoinHandle<()>>,
    host: Option<HostWindowId>,
    row: Option<usize>,
}

impl VideoSurface {
    /// Starts the render thread of `player`; nothing is drawn until the surface is attached to a window.
    pub fn new(player: &MpvPlayer) -> Result<Self, String> {
        let id = NEXT_SURFACE.fetch_add(1, Ordering::Relaxed);
        let channel = Arc::new(Channel::default());
        let core = player.render_core();
        let thread_channel = Arc::clone(&channel);
        let thread = std::thread::Builder::new()
            .name(format!("video-render-{id}"))
            .spawn(move || render_thread::run(id, &core, &thread_channel))
            .map_err(|e| format!("render thread: {e}"))?;
        Ok(Self {
            id,
            channel,
            thread: Some(thread),
            host: None,
            row: None,
        })
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    /// The window the surface draws into.
    pub fn host(&self) -> Option<HostWindowId> {
        self.host
    }

    /// The row of the window's `video-frames` that shows the video, if it is attached to `host`.
    pub fn row_in(&self, host: HostWindowId) -> Option<usize> {
        (self.host == Some(host)).then_some(self.row).flatten()
    }

    /// Draws into `host` from now on (UI thread). A surface that moves from another window gets a new render context there; mpv
    /// disables its video track when the old one goes, so the track is selected again once the new one exists. Position, pause,
    /// volume and everything else of the player stay as they are. Until the first frame of the new render context the new window
    /// shows the last picture of the old one (the render thread reads it back before it leaves; this waits for that, at most
    /// `STILL_WAIT`).
    pub fn attach(&mut self, host: HostWindowId, player: &Arc<MpvPlayer>) {
        if self.host == Some(host) {
            return;
        }
        let track = self.host.is_some().then(|| player.video_track());
        if let Some(old) = self.host.take() {
            self.channel.lock().keep_still = true;
            let epoch = hosts::detach(old, self.id);
            let started = std::time::Instant::now();
            let in_time = epoch.is_none_or(|epoch| self.channel.wait_left(epoch, STILL_WAIT));
            tracing::debug!(
                surface = self.id,
                in_time,
                waited_us = started.elapsed().as_micros(),
                "[VideoSurface: Still] Old window left"
            );
        }
        self.row = hosts::attach(host, self.id, &self.channel);
        self.host = Some(host);
        tracing::info!(
            surface = self.id,
            window = host,
            moved = track.is_some(),
            "[VideoSurface: Attach] Video drawn in a window"
        );
        if let Some(track) = track {
            let player = Arc::clone(player);
            self.when_ready(Box::new(move || player.restore_video_track(&track)));
        }
    }

    /// Draws nowhere (UI thread): the render context goes, mpv disables the video track until the surface has a render context
    /// again, wherever that will be.
    pub fn detach(&mut self, player: &Arc<MpvPlayer>) {
        let Some(old) = self.host.take() else {
            return;
        };
        let track = player.video_track();
        hosts::detach(old, self.id);
        self.row = None;
        let player = Arc::clone(player);
        self.channel
            .lock()
            .on_ready
            .push(Box::new(move || player.restore_video_track(&track)));
        tracing::info!(
            surface = self.id,
            window = old,
            "[VideoSurface: Detach] Video drawn nowhere"
        );
    }

    /// Runs `action` on the UI thread once the surface has a render context (at once when it has one now; call it on the UI
    /// thread). mpv cannot start a video before (it would disable the video track), so loading a file waits for it.
    pub fn when_ready(&self, action: ReadyAction) {
        let mut state = self.channel.lock();
        if state.ready_epoch != 0 && state.ready_epoch == state.epoch {
            drop(state);
            action();
        } else {
            state.on_ready.push(action);
        }
    }

    /// The size of the textures in physical pixels (the frame the video is drawn into); 0 draws nothing.
    pub fn set_size(&self, width: u32, height: u32) {
        let mut state = self.channel.lock();
        if state.size == (width, height) {
            return;
        }
        state.size = (width, height);
        self.channel.notify_locked(state);
    }

    /// Why the last render context could not be created, if it could not.
    pub fn error(&self) -> Option<String> {
        self.channel.lock().error.clone()
    }
}

impl Drop for VideoSurface {
    fn drop(&mut self) {
        if let Some(host) = self.host.take() {
            // Normally detached by the host on the UI thread already; this only helps when it was not.
            hosts::detach(host, self.id);
        }
        {
            let mut state = self.channel.lock();
            state.quit = true;
            state.on_ready.clear();
            self.channel.notify_locked(state);
        }
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!(surface = self.id, "[VideoSurface: Drop] The render thread panicked");
        }
        tracing::debug!(surface = self.id, "[VideoSurface: Drop] Surface released");
    }
}
