//! The render thread of one video: it owns mpv's render context and the textures of the video, in a GL context that shares
//! them with the Slint window that shows the video. When the video moves to another window (the fullscreen and the main window
//! have contexts of their own) the thread frees its render context and textures and creates new ones in the other window's
//! share group; mpv keeps playing meanwhile. Before it frees the textures of a window the video leaves it reads the last picture
//! back (one read per move), and the new window shows that still until the first frame of the new render context.
//!
//! The thread calls no libmpv function but `mpv_render_*` (render.h, "Threading"): loading a file or selecting the video track
//! again happens on the UI thread, as `ReadyAction`s the thread posts once its render context exists.

use super::exchange::{Binding, Channel, Fence, Frame, ReadyAction, SLOTS};
use super::gl_api::{describe, glow_context};
use super::share_group::ThreadContext;
use super::target::{INTERNAL_FORMAT, VideoTarget};
use crate::viewer::mpv_engine::{MpvRenderContext, RenderCore};
use glow::HasContext as _;
use rustview_core::util::cast::SaturatingAs;
use slint::{Rgba8Pixel, SharedPixelBuffer};
use std::sync::Arc;

/// The render context of one binding and everything made in its GL context.
struct Bound {
    epoch: u64,
    context: ThreadContext,
    gl: glow::Context,
    /// `None` only while the binding is torn down.
    render: Option<MpvRenderContext>,
    slots: [Option<VideoTarget>; SLOTS],
    /// Slots that can be rendered into, each with the fence the UI attached when it gave it back.
    free: Vec<(usize, Option<Fence>)>,
    rendered_size: (u32, u32),
    /// The slot of the newest rendered picture.
    newest: Option<usize>,
    /// mpv reported a video frame in this render context. Before that a render only clears to black (after a move mpv selects
    /// the video track again and decodes for a while), so nothing is handed to the UI and the window keeps the still.
    has_frame: bool,
    redraw: Arc<dyn Fn() + Send + Sync>,
}

pub fn run(surface: u64, core: &RenderCore, channel: &Arc<Channel>) {
    let mut bound: Option<Bound> = None;
    // The last binding a render context was tried for (a failed one is not tried again on every wake-up).
    let mut tried = 0;
    loop {
        let mut state = channel.wait();
        if state.quit {
            break;
        }
        state.wake = false;
        let mut forced = false;
        if tried != state.epoch {
            let epoch = state.epoch;
            tried = epoch;
            let binding = state.binding.clone();
            let stale = (state.ready.take(), std::mem::take(&mut state.released));
            let keep_still = std::mem::take(&mut state.keep_still);
            state.ready_epoch = 0;
            drop(state);
            let still = bound.take().and_then(|old| {
                let still = keep_still.then(|| old.read_still(surface)).flatten();
                old.unbind(stale.0, stale.1);
                still
            });
            channel.mark_left(epoch, still);
            if let Some(binding) = binding {
                bound = bind(surface, core, channel, epoch, binding);
                forced = true;
            }
            state = channel.lock();
        }
        let Some(b) = bound.as_mut() else {
            continue;
        };
        b.free.append(&mut state.released);
        let size = state.size;
        drop(state);
        b.render_frame(surface, channel, size, forced);
    }
    if let Some(old) = bound.take() {
        let mut state = channel.lock();
        let stale = (state.ready.take(), std::mem::take(&mut state.released));
        drop(state);
        old.unbind(stale.0, stale.1);
    }
    tracing::debug!(surface, "[VideoSurface: Render] Render thread ended");
}

/// Creates the GL context and mpv's render context of a new binding; posts the waiting UI actions once both exist.
fn bind(surface: u64, core: &RenderCore, channel: &Arc<Channel>, epoch: u64, binding: Binding) -> Option<Bound> {
    let created = binding.group.create_context().and_then(|context| {
        if let Err(e) = context.make_current() {
            context.destroy();
            return Err(e);
        }
        let api = context.api();
        let gl = glow_context(api);
        match core.create_context(api, binding.display) {
            Ok(render) => Ok((context, gl, render)),
            Err(e) => {
                context.destroy();
                Err(e)
            }
        }
    });
    let (context, gl, mut render) = match created {
        Ok(parts) => parts,
        Err(e) => {
            tracing::error!(surface, error = %e, "[VideoSurface: Render] No render context; the video stays dark");
            channel.lock().error = Some(e);
            return None;
        }
    };
    let notify = Arc::clone(channel);
    render.set_update_callback(move || notify.notify());
    tracing::info!(
        surface,
        epoch,
        api = context.api().name(),
        gl = %describe(&gl),
        "[VideoSurface: Render] Render context created in the window's share group"
    );
    let actions: Vec<ReadyAction> = {
        let mut state = channel.lock();
        state.error = None;
        if state.epoch == epoch {
            state.ready_epoch = epoch;
            std::mem::take(&mut state.on_ready)
        } else {
            Vec::new()
        }
    };
    for action in actions {
        if let Err(e) = slint::invoke_from_event_loop(action) {
            tracing::warn!(surface, error = %e, "[VideoSurface: Render] A waiting action could not reach the UI thread");
        }
    }
    Some(Bound {
        epoch,
        context,
        gl,
        render: Some(render),
        slots: [None, None, None],
        free: (0..SLOTS).map(|i| (i, None)).collect(),
        rendered_size: (0, 0),
        newest: None,
        has_frame: false,
        redraw: binding.redraw,
    })
}

impl Bound {
    /// Renders mpv's current frame if there is a new one (or the size changed, or `forced`) and hands it to the UI.
    fn render_frame(&mut self, surface: u64, channel: &Channel, size: (u32, u32), forced: bool) {
        let Some(render) = self.render.as_ref() else {
            return;
        };
        // Must be called after every update callback, whether or not a frame is drawn.
        let new_frame = render.has_new_frame();
        if new_frame && !self.has_frame {
            self.has_frame = true;
            tracing::debug!(
                surface,
                epoch = self.epoch,
                "[VideoSurface: Render] First video frame of the render context"
            );
        }
        let resized = size != self.rendered_size;
        if !(new_frame || resized || forced) || !self.has_frame || size.0 == 0 || size.1 == 0 {
            return;
        }
        // A free slot; when the UI holds all others, take back the frame it has not picked up yet (a newer one replaces it).
        let (slot, release) = if let Some(free) = self.free.pop() {
            free
        } else {
            let Some(old) = channel.lock().ready.take() else {
                return;
            };
            // SAFETY: this thread's context is current; the fence is our own, of a frame nobody will read.
            unsafe { self.gl.delete_sync(old.fence.0) };
            (old.slot, None)
        };
        if let Some(fence) = release {
            // SAFETY: this thread's context is current; the fence is the UI's release fence of this slot.
            unsafe {
                self.gl.wait_sync(fence.0, 0, glow::TIMEOUT_IGNORED);
                self.gl.delete_sync(fence.0);
            }
        }
        if self.slots[slot].as_ref().is_none_or(|t| t.size() != size) {
            if let Some(old) = self.slots[slot].take() {
                old.delete(&self.gl);
            }
            match VideoTarget::new(&self.gl, size.0, size.1) {
                Ok(target) => self.slots[slot] = Some(target),
                Err(e) => {
                    tracing::error!(surface, error = %e, "[VideoSurface: Render] Texture could not be created");
                    self.free.push((slot, None));
                    return;
                }
            }
        }
        let Some(target) = self.slots[slot].as_ref() else {
            return;
        };
        // Blocks until the frame's display time: mpv keeps its own timing, the UI thread never waits for it.
        if let Err(e) = render.render(
            target.fbo(),
            size.0.saturating_as(),
            size.1.saturating_as(),
            INTERNAL_FORMAT,
        ) {
            tracing::warn!(surface, error = %e, "[VideoSurface: Render] mpv could not render a frame");
            self.free.push((slot, None));
            return;
        }
        self.rendered_size = size;
        self.newest = Some(slot);
        // SAFETY: this thread's context is current; the fence follows mpv's commands and is flushed so another context of the
        // share group can wait for it.
        let fence = unsafe {
            let fence = self.gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0);
            self.gl.flush();
            fence
        };
        let Ok(fence) = fence else {
            self.free.push((slot, None));
            return;
        };
        let frame = Frame {
            slot,
            texture: target.texture,
            size,
            fence: Fence(fence),
            epoch: self.epoch,
        };
        let superseded = {
            let mut state = channel.lock();
            if state.epoch != self.epoch {
                // The video moves to another window; this frame is never shown.
                drop(state);
                // SAFETY: our own fence of a frame nobody will read.
                unsafe { self.gl.delete_sync(frame.fence.0) };
                self.free.push((slot, None));
                return;
            }
            state.ready.replace(frame)
        };
        if let Some(old) = superseded {
            // SAFETY: our own fence of a frame the UI never took.
            unsafe { self.gl.delete_sync(old.fence.0) };
            self.free.push((old.slot, None));
        }
        (self.redraw)();
    }

    /// The newest picture in memory (top row first, like the texture Slint shows), or `None` when nothing was rendered.
    fn read_still(&self, surface: u64) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
        let target = self.slots[self.newest?].as_ref()?;
        let (width, height) = target.size();
        let mut still = SharedPixelBuffer::<Rgba8Pixel>::new(width, height);
        target.read_into(&self.gl, still.make_mut_bytes());
        tracing::debug!(
            surface,
            epoch = self.epoch,
            width,
            height,
            "[VideoSurface: Render] Last picture kept as a still for the next window"
        );
        Some(still)
    }

    /// Frees mpv's render context (mpv disables the video track then) and everything of this GL context, then the context.
    fn unbind(mut self, ready: Option<Frame>, released: Vec<(usize, Option<Fence>)>) {
        self.render = None;
        let fences = ready
            .map(|f| f.fence)
            .into_iter()
            .chain(released.into_iter().filter_map(|(_, f)| f));
        let fences = fences.chain(self.free.drain(..).filter_map(|(_, f)| f));
        for fence in fences {
            // SAFETY: this thread's context is current; every fence belongs to this share group and is deleted once.
            unsafe { self.gl.delete_sync(fence.0) };
        }
        for target in self.slots.iter_mut().filter_map(Option::take) {
            target.delete(&self.gl);
        }
        self.context.destroy();
        tracing::debug!(
            epoch = self.epoch,
            "[VideoSurface: Render] Render context and textures released"
        );
    }
}
