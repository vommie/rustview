//! The windowing GL API of Slint's context and a GL loader that stays valid as long as the process runs.
//!
//! Slint's femtovg renderer uses GLX on X11, its skia renderer and every renderer on Wayland use EGL. mpv keeps the loader of
//! its render context and may resolve functions long after the context was created (when the hardware decoder's interop
//! starts), so it gets this `'static` one instead of the loader Slint lends to its rendering notifier.

use khronos_egl as egl;
use std::ffi::{CString, c_char, c_void};
use std::sync::OnceLock;
use x11_dl::glx;

pub(super) type Egl = egl::DynamicInstance<egl::EGL1_4>;

pub(super) fn egl_instance() -> Option<&'static Egl> {
    static EGL: OnceLock<Option<Egl>> = OnceLock::new();
    // SAFETY: loads the system's libEGL once; the instance only calls entry points that follow the EGL 1.4 signatures.
    EGL.get_or_init(|| unsafe { Egl::load_required() }.ok()).as_ref()
}

pub(super) fn glx_instance() -> Option<&'static glx::Glx> {
    static GLX: OnceLock<Option<glx::Glx>> = OnceLock::new();
    GLX.get_or_init(|| glx::Glx::open().ok()).as_ref()
}

/// The windowing API a GL context belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeApi {
    Glx,
    Egl,
}

impl NativeApi {
    /// The API of the GL context that is current on this thread (inside Slint's rendering notifier: Slint's).
    pub fn current() -> Option<Self> {
        if let Some(e) = egl_instance()
            && e.get_current_context().is_some()
        {
            return Some(Self::Egl);
        }
        let g = glx_instance()?;
        // SAFETY: a plain query of the calling thread's current context.
        (!unsafe { (g.glXGetCurrentContext)() }.is_null()).then_some(Self::Glx)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Glx => "GLX",
            Self::Egl => "EGL",
        }
    }

    /// Encodes the API as the `ctx` pointer of mpv's loader callback (no allocation that would have to outlive mpv).
    pub(crate) fn as_loader_ctx(self) -> *mut c_void {
        match self {
            Self::Glx => std::ptr::null_mut(),
            Self::Egl => std::ptr::NonNull::<c_void>::dangling().as_ptr(),
        }
    }

    pub(crate) fn from_loader_ctx(ctx: *mut c_void) -> Self {
        if ctx.is_null() { Self::Glx } else { Self::Egl }
    }
}

/// Resolves a GL function with the windowing API's own loader (valid for every context of that API).
pub fn get_proc_address(api: NativeApi, name: &str) -> *mut c_void {
    let Ok(cname) = CString::new(name) else {
        return std::ptr::null_mut();
    };
    match api {
        NativeApi::Egl => egl_instance()
            .and_then(|e| e.get_proc_address(name))
            .map_or(std::ptr::null_mut(), |f| f as *mut c_void),
        NativeApi::Glx => glx_instance()
            // SAFETY: a NUL-terminated name; glXGetProcAddressARB only looks it up.
            .and_then(|g| unsafe { (g.glXGetProcAddressARB)(cname.as_ptr().cast::<u8>()) })
            .map_or(std::ptr::null_mut(), |f| f as *mut c_void),
    }
}

/// mpv's loader callback: `ctx` encodes the API (see [`NativeApi::as_loader_ctx`]).
pub(crate) extern "C" fn mpv_get_proc_address(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if name.is_null() {
        return std::ptr::null_mut();
    }
    // SAFETY: mpv passes a NUL-terminated function name that lives for the duration of the call.
    let name = unsafe { std::ffi::CStr::from_ptr(name) };
    name.to_str().map_or(std::ptr::null_mut(), |n| {
        get_proc_address(NativeApi::from_loader_ctx(ctx), n)
    })
}

/// glow on the context of `api` that is current on this thread.
pub fn glow_context(api: NativeApi) -> glow::Context {
    // SAFETY: the loader returns the functions of the windowing API; glow calls them only while a context of it is current.
    unsafe { glow::Context::from_loader_function(|name| get_proc_address(api, name).cast_const()) }
}

/// Vendor, renderer and version of the current context ("llvmpipe" = software rendering).
pub fn describe(gl: &glow::Context) -> String {
    use glow::HasContext as _;
    // SAFETY: the context is current; string queries with valid enums.
    unsafe {
        format!(
            "{} / {} / {}",
            gl.get_parameter_string(glow::VENDOR),
            gl.get_parameter_string(glow::RENDERER),
            gl.get_parameter_string(glow::VERSION)
        )
    }
}
