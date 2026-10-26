//! mpv's OpenGL render API (`render.h`, `render_gl.h`) through libmpv's raw client API.
//!
//! libmpv2's own `RenderContext` borrows the `Mpv` it was made from and cannot move to another thread; a video surface needs the
//! render context on its render thread for its whole life (`viewer::video_surface`). This one keeps an `Arc` of the core, so the
//! context is always freed before the core is destroyed ("You must free the context with `mpv_render_context_free()` before the
//! mpv core is destroyed"), and it is `Send`: libmpv allows every `mpv_render_*` call from any thread as long as the GL context
//! is current there and the calls do not overlap.

use super::MpvPlayer;
use crate::viewer::video_surface::gl_api::{NativeApi, mpv_get_proc_address};
use libmpv2::Mpv;
use libmpv2_sys::{
    mpv_opengl_fbo, mpv_opengl_init_params, mpv_render_context, mpv_render_param,
    mpv_render_param_type_MPV_RENDER_PARAM_API_TYPE as PARAM_API_TYPE,
    mpv_render_param_type_MPV_RENDER_PARAM_BLOCK_FOR_TARGET_TIME as PARAM_BLOCK_FOR_TARGET_TIME,
    mpv_render_param_type_MPV_RENDER_PARAM_FLIP_Y as PARAM_FLIP_Y,
    mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_FBO as PARAM_OPENGL_FBO,
    mpv_render_param_type_MPV_RENDER_PARAM_OPENGL_INIT_PARAMS as PARAM_OPENGL_INIT_PARAMS,
    mpv_render_param_type_MPV_RENDER_PARAM_WL_DISPLAY as PARAM_WL_DISPLAY,
    mpv_render_param_type_MPV_RENDER_PARAM_X11_DISPLAY as PARAM_X11_DISPLAY,
    mpv_render_update_flag_MPV_RENDER_UPDATE_FRAME as UPDATE_FRAME,
};
use std::ffi::c_void;
use std::sync::Arc;

/// The native display mpv's hardware decoders may need for their GL interop (`MPV_RENDER_PARAM_X11_DISPLAY` / `_WL_DISPLAY`).
#[derive(Debug, Clone, Copy)]
pub enum NativeDisplay {
    None,
    X11(*mut c_void),
    Wayland(*mut c_void),
}

// SAFETY: the pointer names the process-wide display connection of the windowing system (winit opened the X display with
// XInitThreads); it is only handed to libmpv, which may use it from its own threads, and never dereferenced here.
unsafe impl Send for NativeDisplay {}
// SAFETY: see `Send`; the value is never mutated through a shared reference.
unsafe impl Sync for NativeDisplay {}

/// The callback mpv calls (on one of its threads) when a new frame should be rendered. It must not call mpv.
type UpdateCallback = Box<dyn Fn() + Send + Sync>;

pub struct MpvRenderContext {
    ctx: *mut mpv_render_context,
    /// The update callback mpv holds a pointer to (double boxed: a thin pointer for the C side).
    update: Option<Box<UpdateCallback>>,
    /// Keeps the core alive until the context is freed.
    _core: Arc<Mpv>,
}

// SAFETY: libmpv's render functions may be called from any thread (render.h, "Threading"); this type is used by one thread at a
// time (it is moved to the render thread and only called there) and the context is freed on the thread that renders.
unsafe impl Send for MpvRenderContext {}

fn param(type_: u32, data: *mut c_void) -> mpv_render_param {
    mpv_render_param { type_, data }
}

const END: mpv_render_param = mpv_render_param {
    type_: 0,
    data: std::ptr::null_mut(),
};

unsafe extern "C" fn on_update(ctx: *mut c_void) {
    // SAFETY: `ctx` is the `Box<UpdateCallback>` of the render context, which outlives the callback: it is only dropped after
    // `mpv_render_context_free` (which waits for every callback to return) or after the callback was replaced.
    let callback = unsafe { &*ctx.cast::<UpdateCallback>() };
    callback();
}

/// The core of a player as the render thread needs it: only to create the render context (the render thread calls no other
/// libmpv function, render.h "Threading").
#[derive(Clone)]
pub struct RenderCore(Arc<Mpv>);

impl RenderCore {
    /// The core of a player that is not an `MpvPlayer` (the self-test's).
    pub fn of(core: &Arc<Mpv>) -> Self {
        Self(Arc::clone(core))
    }

    /// Creates mpv's OpenGL renderer on the GL context that is current on this thread (`api` names its windowing API). Only one
    /// render context may exist per core at a time.
    pub fn create_context(&self, api: NativeApi, display: NativeDisplay) -> Result<MpvRenderContext, String> {
        MpvRenderContext::create(Arc::clone(&self.0), api, display)
    }
}

impl MpvPlayer {
    pub fn render_core(&self) -> RenderCore {
        RenderCore(Arc::clone(&self.mpv))
    }

    /// Selects the video track again after the render context of another window was freed (mpv disables video then).
    pub fn restore_video_track(&self, vid: &str) {
        if vid.is_empty() || vid == "no" {
            return;
        }
        if let Err(e) = self.mpv.set_property("vid", vid) {
            tracing::warn!(error = ?e, "[MpvPlayer: Render] The video track could not be selected again");
        }
    }

    /// The selected video track (`vid`), to restore it after a render context was replaced.
    pub fn video_track(&self) -> String {
        self.mpv.get_property::<String>("vid").unwrap_or_default()
    }
}

impl MpvRenderContext {
    fn create(core: Arc<Mpv>, api: NativeApi, display: NativeDisplay) -> Result<Self, String> {
        let mut init = mpv_opengl_init_params {
            get_proc_address: Some(mpv_get_proc_address),
            get_proc_address_ctx: api.as_loader_ctx(),
        };
        let mut params = vec![
            param(
                PARAM_API_TYPE,
                libmpv2_sys::MPV_RENDER_API_TYPE_OPENGL.as_ptr().cast_mut().cast(),
            ),
            param(PARAM_OPENGL_INIT_PARAMS, (&raw mut init).cast()),
        ];
        match display {
            NativeDisplay::X11(d) if !d.is_null() => params.push(param(PARAM_X11_DISPLAY, d)),
            NativeDisplay::Wayland(d) if !d.is_null() => params.push(param(PARAM_WL_DISPLAY, d)),
            _ => {}
        }
        params.push(END);
        let mut ctx: *mut mpv_render_context = std::ptr::null_mut();
        // SAFETY: the core handle is alive (the Arc); `params` is a valid, END-terminated array whose pointers outlive the call
        // (mpv copies what it keeps; the loader itself is a static function).
        let err =
            unsafe { libmpv2_sys::mpv_render_context_create(&raw mut ctx, core.ctx.as_ptr(), params.as_mut_ptr()) };
        if err < 0 || ctx.is_null() {
            return Err(format!(
                "mpv_render_context_create failed: {}",
                libmpv2_sys::mpv_error_str(err)
            ));
        }
        Ok(Self {
            ctx,
            update: None,
            _core: core,
        })
    }

    /// Calls `callback` (on one of mpv's threads) whenever the render thread should look for a new frame.
    pub fn set_update_callback(&mut self, callback: impl Fn() + Send + Sync + 'static) {
        let mut boxed: Box<UpdateCallback> = Box::new(Box::new(callback));
        let raw: *mut UpdateCallback = &raw mut *boxed;
        // SAFETY: the context is valid; `raw` points into `boxed`, which is stored below and outlives every call (see
        // `on_update`). Setting the new callback first means mpv never calls the old one after it is dropped.
        unsafe { libmpv2_sys::mpv_render_context_set_update_callback(self.ctx, Some(on_update), raw.cast()) };
        self.update = Some(boxed);
    }

    /// Whether mpv has a new frame (must be called after each update callback, on the render thread).
    pub fn has_new_frame(&self) -> bool {
        // SAFETY: the context is valid and used by this thread only.
        let flags = unsafe { libmpv2_sys::mpv_render_context_update(self.ctx) };
        flags & u64::from(UPDATE_FRAME) != 0
    }

    /// Renders the current frame into framebuffer `fbo` (`width` x `height`, texture format `internal_format`), top row first.
    /// Blocks until the frame's display time (mpv's own timing).
    pub fn render(&self, fbo: i32, width: i32, height: i32, internal_format: i32) -> Result<(), String> {
        let mut target = mpv_opengl_fbo {
            fbo,
            w: width,
            h: height,
            internal_format,
        };
        let mut flip: i32 = 0;
        let mut block: i32 = 1;
        let mut params = [
            param(PARAM_OPENGL_FBO, (&raw mut target).cast()),
            param(PARAM_FLIP_Y, (&raw mut flip).cast()),
            param(PARAM_BLOCK_FOR_TARGET_TIME, (&raw mut block).cast()),
            END,
        ];
        // SAFETY: the context is valid and its GL context current on this thread; the parameters live for the call.
        let err = unsafe { libmpv2_sys::mpv_render_context_render(self.ctx, params.as_mut_ptr()) };
        if err < 0 {
            Err(libmpv2_sys::mpv_error_str(err).to_owned())
        } else {
            Ok(())
        }
    }
}

impl Drop for MpvRenderContext {
    fn drop(&mut self) {
        // SAFETY: the context is valid and freed exactly once, before `_core` (the Arc of the core) is released. mpv waits for
        // the update callback to return, so the callback box can go afterwards.
        unsafe { libmpv2_sys::mpv_render_context_free(self.ctx) };
        self.update = None;
    }
}
