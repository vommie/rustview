//! The Slint windows that show videos (the main window, every Multiview fullscreen window), on the UI thread.
//!
//! Each window registers a model of images (`video-frames` in Slint; a video tile shows the row its surface got) and forwards
//! its rendering notifier here. At `RenderingSetup` the share group of the window's GL context is captured and every surface
//! attached to the window gets its render context there; at `BeforeRendering` the newest frame of each surface becomes the image
//! of its row; at `RenderingTeardown` the surfaces lose their render context (a window that is hidden for good).

use super::exchange::{Binding, Channel, Fence};
use super::gl_api::{NativeApi, describe, glow_context};
use super::share_group::ShareGroup;
use crate::viewer::mpv_engine::NativeDisplay;
use glow::HasContext as _;
use slint::{Model as _, VecModel};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// A window that can show videos.
pub type HostWindowId = u64;

/// The main window (tab view and tab grid).
pub const MAIN_WINDOW: HostWindowId = 1;
/// The Multiview fullscreen window (one window, shown again for every fullscreen).
pub const FULLSCREEN_WINDOW: HostWindowId = 2;

/// What the window's GL context turned out to be (Settings > Debug).
#[derive(Debug, Clone, Default)]
pub struct HostGlFacts {
    pub api: Option<NativeApi>,
    pub description: String,
    pub error: Option<String>,
}

struct Attached {
    surface: u64,
    channel: Arc<Channel>,
    row: usize,
    /// The texture slot whose texture the row shows.
    shown: Option<usize>,
    /// The binding the surface got from this window.
    epoch: u64,
}

struct HostWindow {
    frames: Rc<VecModel<slint::Image>>,
    redraw: Arc<dyn Fn() + Send + Sync>,
    display: Box<dyn Fn() -> NativeDisplay>,
    group: Option<ShareGroup>,
    gl: Option<glow::Context>,
    attached: Vec<Attached>,
    facts: HostGlFacts,
}

thread_local! {
    static HOSTS: RefCell<HashMap<HostWindowId, HostWindow>> = RefCell::new(HashMap::new());
}

/// Registers a window that shows videos. `frames` is the model behind its `video-frames` property, `redraw` asks it for a new
/// frame from any thread, `display` names its native display once it exists.
pub fn register_window(
    id: HostWindowId,
    frames: Rc<VecModel<slint::Image>>,
    redraw: Arc<dyn Fn() + Send + Sync>,
    display: Box<dyn Fn() -> NativeDisplay>,
) {
    HOSTS.with_borrow_mut(|hosts| {
        hosts.insert(
            id,
            HostWindow {
                frames,
                redraw,
                display,
                group: None,
                gl: None,
                attached: Vec::new(),
                facts: HostGlFacts::default(),
            },
        );
    });
    tracing::debug!(window = id, "[VideoSurface: Host] Window registered");
}

/// Forgets a window; its surfaces lose their render context.
pub fn unregister_window(id: HostWindowId) {
    let removed = HOSTS.with_borrow_mut(|hosts| hosts.remove(&id));
    if let Some(window) = removed {
        for attached in &window.attached {
            rebind(&attached.channel, None);
        }
        tracing::debug!(
            window = id,
            surfaces = window.attached.len(),
            "[VideoSurface: Host] Window unregistered"
        );
    }
}

/// What the GL context of a window is (`None` until it rendered once).
pub fn gl_facts(id: HostWindowId) -> Option<HostGlFacts> {
    HOSTS.with_borrow(|hosts| hosts.get(&id).map(|w| w.facts.clone()))
}

/// The share group of a window, once it rendered (the self-test renders into it).
pub fn share_group(id: HostWindowId) -> Option<(ShareGroup, NativeDisplay)> {
    HOSTS.with_borrow(|hosts| hosts.get(&id).and_then(|w| w.group.map(|g| (g, display_for(w, g)))))
}

/// The window's rendering notifier, forwarded.
pub fn on_rendering(id: HostWindowId, state: &slint::RenderingState, api: &slint::GraphicsAPI<'_>) {
    match state {
        slint::RenderingState::RenderingSetup => setup(id, api),
        slint::RenderingState::BeforeRendering => take_frames(id),
        slint::RenderingState::RenderingTeardown => teardown(id),
        _ => {}
    }
}

fn display_for(window: &HostWindow, group: ShareGroup) -> NativeDisplay {
    match group.x11_display() {
        Some(display) => NativeDisplay::X11(display.cast()),
        None => (window.display)(),
    }
}

fn binding_of(window: &HostWindow) -> Option<Binding> {
    let group = window.group?;
    Some(Binding {
        group,
        display: display_for(window, group),
        redraw: Arc::clone(&window.redraw),
    })
}

/// Gives the surface a new binding (another window, or none); the render thread follows on its next wake-up.
fn rebind(channel: &Channel, binding: Option<Binding>) -> u64 {
    let mut state = channel.lock();
    state.binding = binding;
    state.epoch += 1;
    state.ready_epoch = 0;
    let epoch = state.epoch;
    channel.notify_locked(state);
    epoch
}

fn setup(id: HostWindowId, api: &slint::GraphicsAPI<'_>) {
    let captured = match api {
        slint::GraphicsAPI::NativeOpenGL { .. } => NativeApi::current()
            .ok_or_else(|| "no current GLX or EGL context".to_owned())
            .and_then(|native| {
                let gl = glow_context(native);
                ShareGroup::capture(native, &gl).map(|group| (native, gl, group))
            }),
        _ => Err("the renderer does not use OpenGL".to_owned()),
    };
    HOSTS.with_borrow_mut(|hosts| {
        let Some(window) = hosts.get_mut(&id) else {
            return;
        };
        match captured {
            Ok((native, gl, group)) => {
                let version = group.version();
                window.facts = HostGlFacts {
                    api: Some(native),
                    description: describe(&gl),
                    error: None,
                };
                tracing::info!(
                    window = id,
                    api = native.name(),
                    gl = %window.facts.description,
                    es = version.es,
                    core = version.core,
                    "[VideoSurface: Host] GL context of the window captured; videos render in its share group"
                );
                if id == MAIN_WINDOW {
                    crate::system::diagnostics::graphics::record_video_path(&format!(
                        "mpv's render API, {}",
                        native.name()
                    ));
                }
                window.group = Some(group);
                window.gl = Some(gl);
                let binding = binding_of(window);
                for attached in &mut window.attached {
                    attached.shown = None;
                    attached.epoch = rebind(&attached.channel, binding.clone());
                }
            }
            Err(e) => {
                tracing::error!(window = id, error = %e, "[VideoSurface: Host] Videos cannot be drawn in this window");
                window.facts = HostGlFacts {
                    api: None,
                    description: String::new(),
                    error: Some(e),
                };
            }
        }
    });
}

fn teardown(id: HostWindowId) {
    HOSTS.with_borrow_mut(|hosts| {
        let Some(window) = hosts.get_mut(&id) else {
            return;
        };
        for attached in &mut window.attached {
            window.frames.set_row_data(attached.row, slint::Image::default());
            attached.shown = None;
            attached.epoch = rebind(&attached.channel, None);
        }
        window.group = None;
        window.gl = None;
        tracing::debug!(window = id, "[VideoSurface: Host] GL context of the window gone");
    });
}

/// Shows the newest frame of every surface of the window (Slint's context is current).
fn take_frames(id: HostWindowId) {
    HOSTS.with_borrow_mut(|hosts| {
        let Some(window) = hosts.get_mut(&id) else {
            return;
        };
        let Some(gl) = window.gl.as_ref() else {
            return;
        };
        for attached in &mut window.attached {
            let mut state = attached.channel.lock();
            if state.ready.as_ref().is_none_or(|f| f.epoch != attached.epoch) {
                continue;
            }
            let Some(frame) = state.ready.take() else {
                continue;
            };
            if state.still.take().is_some() {
                tracing::debug!(
                    window = id,
                    surface = attached.surface,
                    "[VideoSurface: Still] First frame in the new window replaces the still"
                );
            }
            // SAFETY: Slint's context is current; the fence belongs to its share group and is waited on and deleted once.
            unsafe {
                gl.wait_sync(frame.fence.0, 0, glow::TIMEOUT_IGNORED);
                gl.delete_sync(frame.fence.0);
            }
            if let Some(old) = attached.shown.replace(frame.slot) {
                // Everything that read the old texture was submitted before this fence; the render thread waits for it before
                // it renders into that texture again.
                // SAFETY: Slint's context is current.
                let release = unsafe {
                    let fence = gl.fence_sync(glow::SYNC_GPU_COMMANDS_COMPLETE, 0).ok();
                    gl.flush();
                    fence
                };
                state.released.push((old, release.map(Fence)));
                attached.channel.notify_locked(state);
            } else {
                drop(state);
            }
            // SAFETY: the texture belongs to the window's share group and stays alive until this row stops showing it: the render
            // thread reuses or deletes it only after the UI gave its slot back or the binding changed (which clears the row).
            let image = unsafe {
                slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(frame.texture.0, frame.size.into())
            }
            .origin(slint::BorrowedOpenGLTextureOrigin::TopLeft)
            .build();
            window.frames.set_row_data(attached.row, image);
        }
    });
}

/// Attaches a surface to a window: returns the row of `video-frames` that shows it (the still of a video that comes from another
/// window until its first frame here). The surface gets the window's binding now or, when the window has not rendered yet, at its
/// `RenderingSetup`.
pub(super) fn attach(id: HostWindowId, surface: u64, channel: &Arc<Channel>) -> Option<usize> {
    HOSTS.with_borrow_mut(|hosts| {
        let Some(window) = hosts.get_mut(&id) else {
            tracing::warn!(window = id, surface, "[VideoSurface: Host] Attach to an unknown window");
            rebind(channel, None);
            return None;
        };
        let used: Vec<usize> = window.attached.iter().map(|a| a.row).collect();
        let row = (0..window.frames.row_count())
            .find(|r| !used.contains(r))
            .unwrap_or_else(|| {
                window.frames.push(slint::Image::default());
                window.frames.row_count() - 1
            });
        let still = channel.lock().still.clone();
        let shows_still = still.is_some();
        window
            .frames
            .set_row_data(row, still.map(slint::Image::from_rgba8).unwrap_or_default());
        let epoch = rebind(channel, binding_of(window));
        window.attached.push(Attached {
            surface,
            channel: Arc::clone(channel),
            row,
            shown: None,
            epoch,
        });
        tracing::debug!(
            window = id,
            surface,
            row,
            shows_still,
            "[VideoSurface: Host] Surface attached"
        );
        Some(row)
    })
}

/// Detaches a surface from a window: its row shows nothing from now on and the surface loses its render context. Returns the
/// epoch of that change (`None` when the surface was not attached there).
pub(super) fn detach(id: HostWindowId, surface: u64) -> Option<u64> {
    HOSTS.with_borrow_mut(|hosts| {
        let window = hosts.get_mut(&id)?;
        let index = window.attached.iter().position(|a| a.surface == surface)?;
        let attached = window.attached.swap_remove(index);
        window.frames.set_row_data(attached.row, slint::Image::default());
        let epoch = rebind(&attached.channel, None);
        tracing::debug!(window = id, surface, "[VideoSurface: Host] Surface detached");
        Some(epoch)
    })
}
