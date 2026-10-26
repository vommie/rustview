//! What the UI thread and the render thread of one video hand each other: the wanted size and window, finished frames (a
//! texture plus a GL fence), the textures the UI no longer shows (with the fence after which it no longer reads them) and the
//! still of a video that moves to another window.
//!
//! Three textures per video: one shown by Slint, one being rendered, one waiting. The UI never waits for the render thread: it
//! takes the newest frame if there is one, makes the GPU wait for its fence (`glWaitSync`, no CPU stall) and gives the texture it
//! showed before back.

use super::share_group::ShareGroup;
use crate::viewer::mpv_engine::NativeDisplay;
use parking_lot::{Condvar, Mutex, MutexGuard};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::sync::Arc;
use std::time::Duration;

/// Textures per video.
pub const SLOTS: usize = 3;

/// A GL sync object; it is valid in every context of the share group it was made in.
pub struct Fence(pub glow::NativeFence);

// SAFETY: a GLsync handle is a share-group object usable from any thread whose context is in that group; every fence is waited on
// and deleted exactly once, by the side that received it.
unsafe impl Send for Fence {}

/// A rendered texture handed to the UI.
pub struct Frame {
    pub slot: usize,
    pub texture: glow::NativeTexture,
    pub size: (u32, u32),
    pub fence: Fence,
    /// The binding it was rendered for; a frame of an older binding is never shown.
    pub epoch: u64,
}

// SAFETY: a texture name of the share group, read by the UI only while it holds the frame.
unsafe impl Send for Frame {}

/// Where the render thread draws: the share group of a host window and how to ask that window for a new frame.
#[derive(Clone)]
pub struct Binding {
    pub group: ShareGroup,
    pub display: NativeDisplay,
    pub redraw: Arc<dyn Fn() + Send + Sync>,
}

/// An action for the UI thread once the render context of the current binding exists (load the file, select the video track
/// again). It runs through `slint::invoke_from_event_loop`.
pub type ReadyAction = Box<dyn FnOnce() + Send>;

#[derive(Default)]
pub struct Exchange {
    pub wake: bool,
    pub quit: bool,
    /// Size of the texture in physical pixels; 0 = draw nothing.
    pub size: (u32, u32),
    /// Where to render; `None` = no render context (mpv has no video output then).
    pub binding: Option<Binding>,
    /// Counts the changes of `binding`.
    pub epoch: u64,
    /// The epoch whose render context exists (0 = none).
    pub ready_epoch: u64,
    /// UI actions waiting for the render context of `epoch`.
    pub on_ready: Vec<ReadyAction>,
    /// The newest frame, not taken by the UI yet.
    pub ready: Option<Frame>,
    /// Slots the UI gave back, each with the fence after which the UI no longer reads it (`None`: it never drew it).
    pub released: Vec<(usize, Option<Fence>)>,
    /// Why the last render context could not be created (shown in the log and Settings > Debug).
    pub error: Option<String>,
    /// Asks the render thread to read the last picture back when it leaves its current binding (the video moves to another
    /// window, whose share group cannot see the old textures).
    pub keep_still: bool,
    /// The last picture of the window the video left, in memory: the new window shows it until its first frame, so the move
    /// shows no black picture. Dropped with that frame.
    pub still: Option<SharedPixelBuffer<Rgba8Pixel>>,
    /// The newest epoch for which the render thread has left its old binding (a move waits for it to have the still).
    pub left_epoch: u64,
}

pub struct Channel {
    state: Mutex<Exchange>,
    wake: Condvar,
    /// Signals `left_epoch` to the UI thread.
    left: Condvar,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            state: Mutex::new(Exchange::default()),
            wake: Condvar::new(),
            left: Condvar::new(),
        }
    }
}

impl Channel {
    pub fn lock(&self) -> MutexGuard<'_, Exchange> {
        self.state.lock()
    }

    /// Wakes the render thread (mpv's update callback, a new size, a new binding, released slots).
    pub fn notify(&self) {
        self.lock().wake = true;
        self.wake.notify_one();
    }

    /// Wakes the render thread after the caller changed the state under `guard`.
    pub fn notify_locked(&self, mut guard: MutexGuard<'_, Exchange>) {
        guard.wake = true;
        drop(guard);
        self.wake.notify_one();
    }

    /// The render thread left its binding of an epoch before `epoch` (and kept the still if it was asked to).
    pub fn mark_left(&self, epoch: u64, still: Option<SharedPixelBuffer<Rgba8Pixel>>) {
        let mut state = self.lock();
        if still.is_some() {
            state.still = still;
        }
        state.left_epoch = state.left_epoch.max(epoch);
        drop(state);
        self.left.notify_all();
    }

    /// Waits (UI thread, at most `timeout`) until the render thread has left the binding it had before `epoch`. Returns whether
    /// it did in time; mpv may hold the thread up to one frame's display time.
    pub fn wait_left(&self, epoch: u64, timeout: Duration) -> bool {
        let mut state = self.lock();
        let deadline = std::time::Instant::now() + timeout;
        while state.left_epoch < epoch && !state.quit {
            if self.left.wait_until(&mut state, deadline).timed_out() {
                return state.left_epoch >= epoch;
            }
        }
        true
    }

    /// Blocks until there is something to do; returns the state locked.
    pub fn wait(&self) -> MutexGuard<'_, Exchange> {
        let mut state = self.lock();
        while !state.wake && !state.quit {
            self.wake.wait(&mut state);
        }
        state
    }
}
