//! GL contexts that share textures with the context of a Slint window, one per video render thread.
//!
//! Slint does not share its contexts between windows, but every context created with Slint's as the share context sees the same
//! textures and sync objects. [`ShareGroup::capture`] reads what is needed to create such contexts while Slint's context is
//! current (its rendering notifier); [`ShareGroup::create_context`] then makes one for a render thread, which renders into
//! framebuffer objects only (a 1x1 pbuffer for GLX, surfaceless for EGL).

use super::gl_api::{NativeApi, egl_instance, glx_instance};
use glow::HasContext as _;
use khronos_egl as egl;
use rustview_core::util::cast::SaturatingAs;
use std::ffi::c_int;
use x11_dl::glx;

const GLX_CONTEXT_MAJOR_VERSION_ARB: c_int = 0x2091;
const GLX_CONTEXT_MINOR_VERSION_ARB: c_int = 0x2092;
const GLX_CONTEXT_PROFILE_MASK_ARB: c_int = 0x9126;
const GLX_CONTEXT_CORE_PROFILE_BIT_ARB: c_int = 0x1;
const GLX_CONTEXT_COMPATIBILITY_PROFILE_BIT_ARB: c_int = 0x2;
const GLX_CONTEXT_ES2_PROFILE_BIT_EXT: c_int = 0x4;
const GLX_PBUFFER_HEIGHT: c_int = 0x8040;
const GL_CONTEXT_PROFILE_MASK: u32 = 0x9126;
const GL_CONTEXT_CORE_PROFILE_BIT: i32 = 0x1;

type GlxCreateContextAttribs = unsafe extern "C" fn(
    *mut x11_dl::xlib::Display,
    glx::GLXFBConfig,
    glx::GLXContext,
    c_int,
    *const c_int,
) -> glx::GLXContext;

/// The version and profile of Slint's context; the render contexts get the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlVersion {
    pub es: bool,
    pub core: bool,
    pub major: i32,
    pub minor: i32,
}

/// What it takes to create more contexts in the share group of the context that was current at [`ShareGroup::capture`].
#[derive(Clone, Copy)]
pub enum ShareGroup {
    Glx {
        display: *mut x11_dl::xlib::Display,
        context: glx::GLXContext,
        config: glx::GLXFBConfig,
        version: GlVersion,
    },
    Egl {
        display: egl::Display,
        context: egl::Context,
        config: egl::Config,
        version: GlVersion,
    },
}

// SAFETY: the handles are process-wide driver objects (the X display winit opened with XInitThreads, GLX/EGL contexts and
// configs); they are only used to create new contexts and to make those current on other threads, which GLX and EGL allow.
unsafe impl Send for ShareGroup {}
// SAFETY: see `Send`; nothing is mutated through a shared reference.
unsafe impl Sync for ShareGroup {}

/// Parses "OpenGL ES 3.2 Mesa ..." or "4.6 (Core Profile) Mesa ...".
pub(super) fn parse_version(version: &str) -> (bool, i32, i32) {
    let es = version.starts_with("OpenGL ES");
    let numbers = version
        .trim_start_matches("OpenGL ES")
        .trim_start_matches(['-', 'C', 'M', ' ']);
    let mut it = numbers.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
    let major = it.next().and_then(|s| s.parse().ok()).unwrap_or(2);
    let minor = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (es, major, minor)
}

impl ShareGroup {
    /// Reads the current context (call it inside Slint's rendering notifier).
    pub fn capture(api: NativeApi, gl: &glow::Context) -> Result<Self, String> {
        // SAFETY: the context is current; a plain query.
        let text = unsafe { gl.get_parameter_string(glow::VERSION) };
        let (es, major, minor) = parse_version(&text);
        // The profile mask exists from desktop OpenGL 3.2 on (an ES context would record an error in Slint's context).
        let mask = if !es && (major, minor) >= (3, 2) {
            // SAFETY: the context is current; a plain query of a valid enum.
            unsafe { gl.get_parameter_i32(GL_CONTEXT_PROFILE_MASK) }
        } else {
            0
        };
        let version = GlVersion {
            es,
            core: mask & GL_CONTEXT_CORE_PROFILE_BIT != 0,
            major,
            minor,
        };
        match api {
            NativeApi::Glx => Self::capture_glx(version),
            NativeApi::Egl => Self::capture_egl(version),
        }
    }

    fn capture_glx(version: GlVersion) -> Result<Self, String> {
        let g = glx_instance().ok_or("no libGL")?;
        let xlib = x11_dl::xlib::Xlib::open().map_err(|e| e.to_string())?;
        // SAFETY: queries of the current GLX context and its display; the config list is freed after use.
        unsafe {
            let display = (g.glXGetCurrentDisplay)();
            let context = (g.glXGetCurrentContext)();
            if display.is_null() || context.is_null() {
                return Err("no current GLX context".into());
            }
            let mut fbconfig_id = 0;
            let mut screen = 0;
            (g.glXQueryContext)(display, context, glx::GLX_FBCONFIG_ID, &raw mut fbconfig_id);
            (g.glXQueryContext)(display, context, glx::GLX_SCREEN, &raw mut screen);
            let mut count = 0;
            let configs = (g.glXGetFBConfigs)(display, screen, &raw mut count);
            if configs.is_null() {
                return Err("glXGetFBConfigs failed".into());
            }
            let list = std::slice::from_raw_parts(configs, count.saturating_as::<usize>());
            let config = list.iter().copied().find(|&c| {
                let mut value = 0;
                (g.glXGetFBConfigAttrib)(display, c, glx::GLX_FBCONFIG_ID, &raw mut value);
                value == fbconfig_id
            });
            (xlib.XFree)(configs.cast());
            let config = config.ok_or("Slint's GLX config not found")?;
            Ok(Self::Glx {
                display,
                context,
                config,
                version,
            })
        }
    }

    fn capture_egl(version: GlVersion) -> Result<Self, String> {
        let e = egl_instance().ok_or("no libEGL")?;
        let display = e.get_current_display().ok_or("no current EGL display")?;
        let context = e.get_current_context().ok_or("no current EGL context")?;
        let config_id = e
            .query_context(display, context, egl::CONFIG_ID)
            .map_err(|e| e.to_string())?;
        let config = e
            .choose_first_config(display, &[egl::CONFIG_ID, config_id, egl::NONE])
            .map_err(|e| e.to_string())?
            .ok_or("Slint's EGL config not found")?;
        Ok(Self::Egl {
            display,
            context,
            config,
            version,
        })
    }

    pub fn api(&self) -> NativeApi {
        match self {
            Self::Glx { .. } => NativeApi::Glx,
            Self::Egl { .. } => NativeApi::Egl,
        }
    }

    pub fn version(&self) -> GlVersion {
        match self {
            Self::Glx { version, .. } | Self::Egl { version, .. } => *version,
        }
    }

    /// The X display of a GLX share group (mpv's hardware decoders may want it); `None` for EGL.
    pub fn x11_display(&self) -> Option<*mut x11_dl::xlib::Display> {
        match self {
            Self::Glx { display, .. } => Some(*display),
            Self::Egl { .. } => None,
        }
    }

    /// A new context in the share group; it is current nowhere yet.
    pub fn create_context(&self) -> Result<ThreadContext, String> {
        match *self {
            Self::Glx {
                display,
                context,
                config,
                version,
            } => {
                let g = glx_instance().ok_or("no libGL")?;
                let profile = if version.es {
                    GLX_CONTEXT_ES2_PROFILE_BIT_EXT
                } else if version.core {
                    GLX_CONTEXT_CORE_PROFILE_BIT_ARB
                } else {
                    GLX_CONTEXT_COMPATIBILITY_PROFILE_BIT_ARB
                };
                let attribs = [
                    GLX_CONTEXT_MAJOR_VERSION_ARB,
                    version.major,
                    GLX_CONTEXT_MINOR_VERSION_ARB,
                    version.minor,
                    GLX_CONTEXT_PROFILE_MASK_ARB,
                    profile,
                    0,
                ];
                // SAFETY: GLX calls on the display and config of Slint's context; the attribute lists end with 0, and the
                // function pointer has the signature of glXCreateContextAttribsARB.
                unsafe {
                    let create = (g.glXGetProcAddressARB)(c"glXCreateContextAttribsARB".as_ptr().cast::<u8>())
                        .ok_or("no glXCreateContextAttribsARB")?;
                    let create: GlxCreateContextAttribs = std::mem::transmute(create);
                    let ctx = create(display, config, context, 1, attribs.as_ptr());
                    if ctx.is_null() {
                        return Err("glXCreateContextAttribsARB failed".into());
                    }
                    let pbuffer_attribs = [glx::GLX_PBUFFER_WIDTH, 1, GLX_PBUFFER_HEIGHT, 1, 0];
                    let pbuffer = (g.glXCreatePbuffer)(display, config, pbuffer_attribs.as_ptr());
                    Ok(ThreadContext::Glx {
                        display,
                        context: ctx,
                        pbuffer,
                    })
                }
            }
            Self::Egl {
                display,
                context,
                config,
                version,
            } => {
                let e = egl_instance().ok_or("no libEGL")?;
                let api = if version.es {
                    egl::OPENGL_ES_API
                } else {
                    egl::OPENGL_API
                };
                e.bind_api(api).map_err(|e| e.to_string())?;
                let mut attribs = vec![
                    egl::CONTEXT_MAJOR_VERSION,
                    version.major,
                    egl::CONTEXT_MINOR_VERSION,
                    version.minor,
                ];
                if !version.es && version.core {
                    attribs.extend([egl::CONTEXT_OPENGL_PROFILE_MASK, egl::CONTEXT_OPENGL_CORE_PROFILE_BIT]);
                }
                attribs.push(egl::NONE);
                let ctx = e
                    .create_context(display, config, Some(context), &attribs)
                    .map_err(|e| e.to_string())?;
                Ok(ThreadContext::Egl {
                    display,
                    context: ctx,
                    api,
                })
            }
        }
    }
}

/// A context of a [`ShareGroup`], current on one render thread at a time.
pub enum ThreadContext {
    Glx {
        display: *mut x11_dl::xlib::Display,
        context: glx::GLXContext,
        pbuffer: glx::GLXPbuffer,
    },
    Egl {
        display: egl::Display,
        context: egl::Context,
        api: egl::Enum,
    },
}

// SAFETY: see `ShareGroup`; a thread context is made current on exactly one thread (its render thread) and destroyed there.
unsafe impl Send for ThreadContext {}

impl ThreadContext {
    pub fn api(&self) -> NativeApi {
        match self {
            Self::Glx { .. } => NativeApi::Glx,
            Self::Egl { .. } => NativeApi::Egl,
        }
    }

    pub fn make_current(&self) -> Result<(), String> {
        match *self {
            Self::Glx {
                display,
                context,
                pbuffer,
            } => {
                let g = glx_instance().ok_or("no libGL")?;
                // SAFETY: the context and the pbuffer were created on this display and are current nowhere else.
                let ok = unsafe { (g.glXMakeContextCurrent)(display, pbuffer, pbuffer, context) };
                if ok == 0 {
                    Err("glXMakeContextCurrent failed".into())
                } else {
                    Ok(())
                }
            }
            Self::Egl { display, context, api } => {
                let e = egl_instance().ok_or("no libEGL")?;
                e.bind_api(api).map_err(|e| e.to_string())?;
                // Surfaceless (EGL_KHR_surfaceless_context): the thread renders into framebuffer objects only.
                e.make_current(display, None, None, Some(context))
                    .map_err(|e| e.to_string())
            }
        }
    }

    /// Makes no context current on this thread and destroys this one.
    pub fn destroy(self) {
        match self {
            Self::Glx {
                display,
                context,
                pbuffer,
            } => {
                if let Some(g) = glx_instance() {
                    // SAFETY: the context belongs to this thread; it is released first, then destroyed.
                    unsafe {
                        (g.glXMakeContextCurrent)(display, 0, 0, std::ptr::null_mut());
                        (g.glXDestroyPbuffer)(display, pbuffer);
                        (g.glXDestroyContext)(display, context);
                    }
                }
            }
            Self::Egl { display, context, .. } => {
                if let Some(e) = egl_instance() {
                    let _ = e.make_current(display, None, None, None);
                    let _ = e.destroy_context(display, context);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_version;

    #[test]
    fn versions_of_desktop_and_es_contexts_are_read() {
        assert_eq!(parse_version("4.6 (Core Profile) Mesa 25.2.3"), (false, 4, 6));
        assert_eq!(parse_version("4.3 (Compatibility Profile) Mesa 25.2.3"), (false, 4, 3));
        assert_eq!(parse_version("OpenGL ES 3.2 Mesa 25.2.3"), (true, 3, 2));
        assert_eq!(parse_version("OpenGL ES-CM 1.1"), (true, 1, 1));
        assert_eq!(parse_version("garbage"), (false, 2, 0));
    }
}
