//! The OpenGL renderer the windows of `RustView` draw with, read once when the main window sets up its rendering (one call
//! into the GL driver, no drive access), and mpv's GPU context of the last video.

use parking_lot::Mutex;
use std::ffi::{CStr, c_void};
use std::sync::OnceLock;

/// What the GL driver says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlInfo {
    pub renderer: String,
    pub version: String,
}

static GL_INFO: OnceLock<GlInfo> = OnceLock::new();
static MPV_CONTEXT: Mutex<Option<String>> = Mutex::new(None);

const GL_RENDERER: u32 = 0x1F01;
const GL_VERSION: u32 = 0x1F02;

type GlGetString = unsafe extern "C" fn(u32) -> *const u8;

/// Reads the renderer and the version from the current GL context (call from Slint's rendering notifier; the first call wins).
pub fn capture(api: &slint::GraphicsAPI<'_>) {
    if GL_INFO.get().is_some() {
        return;
    }
    let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
        return;
    };
    let address = get_proc_address(c"glGetString");
    if address.is_null() {
        return;
    }
    // SAFETY: the address is the driver's `glGetString`, whose C signature is `const GLubyte *glGetString(GLenum)`; Slint calls the
    // rendering notifier with its GL context current, which the call needs.
    let get_string: GlGetString = unsafe { std::mem::transmute::<*const c_void, GlGetString>(address) };
    let read = |name: u32| {
        // SAFETY: `glGetString` returns null or a static, NUL-terminated string owned by the driver.
        let text = unsafe { get_string(name) };
        if text.is_null() {
            String::new()
        } else {
            // SAFETY: not null (checked) and NUL-terminated (the GL specification guarantees it).
            unsafe { CStr::from_ptr(text.cast()) }.to_string_lossy().into_owned()
        }
    };
    let info = GlInfo {
        renderer: read(GL_RENDERER),
        version: read(GL_VERSION),
    };
    tracing::info!(renderer = %info.renderer, version = %info.version, "[SystemDiagnostics: Graphics] OpenGL renderer");
    let _ = GL_INFO.set(info);
}

pub fn gl_info() -> Option<&'static GlInfo> {
    GL_INFO.get()
}

/// Remembers the GPU context mpv chose for a video (`current-gpu-context`).
pub fn record_mpv_context(context: &str) {
    let mut last = MPV_CONTEXT.lock();
    if last.as_deref() != Some(context) {
        tracing::info!(context, "[SystemDiagnostics: Graphics] mpv draws with this GPU context");
        *last = Some(context.to_owned());
    }
}

/// mpv's GPU context of the last video (`None` = no video was shown yet).
pub fn mpv_context() -> Option<String> {
    MPV_CONTEXT.lock().clone()
}
