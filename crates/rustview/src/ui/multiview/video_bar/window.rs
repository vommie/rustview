//! One control bar window: its content, X11 shape and placement.

use super::{InputHandler, VideoBarHost};
use crate::ui::multiview::bar_overlay::{self, BarOwner};
use crate::ui::multiview::views::sync_model;
use crate::ui::{MultiviewVideoBarWindow, VideoBarSpanView};
use crate::viewer::multiview::bar_placement::{rounded_region, xrectangle};
use crate::viewer::multiview::follow::TileKey;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::video_bar::placement::{VideoBarPlacement, bubble_height};
use crate::viewer::multiview::video_bar::prefs::{BarStyle, VideoBarPrefs};
use crate::viewer::multiview::video_bar::reveal::Phase;
use crate::viewer::multiview::video_bar::view::{DragPhase, VideoBarInput, VideoBarTarget, VideoBarView};
use crate::viewer::{OverlayServer, get_raw_window_xid};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use slint::{ComponentHandle, ModelRc, PhysicalPosition, PhysicalSize, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// What the callbacks of the window need as well.
#[derive(Default)]
struct Shared {
    /// The tile the bar belongs to now.
    key: Cell<TileKey>,
    on_input: RefCell<Option<InputHandler>>,
    x: RefCell<XState>,
}

/// The X side of the window.
#[derive(Default)]
struct XState {
    server: Option<OverlayServer>,
    xid: Option<u64>,
    /// Blended by a compositing manager; otherwise solid and shaped like the bar.
    translucent: bool,
    /// The bar inside the window, physical pixels relative to the window.
    bar: Option<Rect>,
    /// The time bubble above the bar, physical pixels relative to the window.
    bubble: Option<Rect>,
    bar_radius: i32,
    bubble_radius: i32,
    scale: f32,
}

impl XState {
    /// Only the bar takes the pointer (the room of the bubble lets it through to the video); without a compositing manager
    /// the window is cut to the bar and the bubble.
    fn apply_shape(&self) {
        let (Some(server), Some(xid), Some(bar)) = (self.server.as_ref(), self.xid, self.bar) else {
            return;
        };
        server.set_input_region(xid, &[xrectangle(bar)]);
        if self.translucent {
            server.set_bounding_region(xid, None);
            return;
        }
        let mut region = rounded_region(bar, self.bar_radius);
        if let Some(bubble) = self.bubble {
            region.extend(rounded_region(bubble, self.bubble_radius));
        }
        server.set_bounding_region(xid, Some(&region));
    }
}

pub(super) struct VideoBarWindow {
    /// The host and the tile this window serves (`None`: free).
    pub(super) assigned: Option<(BarOwner, TileKey)>,
    window: MultiviewVideoBarWindow,
    spans: Rc<VecModel<VideoBarSpanView>>,
    shared: Rc<Shared>,
    shown: bool,
    /// Where the window is on the screen.
    placed: Option<VideoBarPlacement>,
    last_view: Option<VideoBarView>,
}

impl VideoBarWindow {
    pub(super) fn new() -> Option<Self> {
        let window = match bar_overlay::create_override_redirect(MultiviewVideoBarWindow::new) {
            Ok(window) => window,
            Err(e) => {
                tracing::error!(error = %e, "[Multiview: VideoBar] Failed to create a control bar window");
                return None;
            }
        };
        let server = OverlayServer::new()
            .map_err(|e| tracing::warn!(error = %e, "[Multiview: VideoBar] No X connection; the bar cannot be placed"))
            .ok();
        let shared = Rc::new(Shared::default());
        shared.x.borrow_mut().server = server;
        let spans = Rc::new(VecModel::default());
        window.set_spans(ModelRc::from(spans.clone()));
        connect(&window, &shared);
        tracing::info!("[Multiview: VideoBar] Control bar window created");
        Some(Self {
            assigned: None,
            window,
            spans,
            shared,
            shown: false,
            placed: None,
            last_view: None,
        })
    }

    /// Gives the window to the bar of tile `key` of `owner`.
    pub(super) fn assign(&mut self, owner: BarOwner, key: TileKey) {
        self.hide("the window serves another tile");
        self.assigned = Some((owner, key));
        self.shared.key.set(key);
        self.last_view = None;
    }

    pub(super) fn translucent(&self) -> bool {
        self.shared.x.borrow().translucent
    }

    /// Shows the bar of `target` (or lets it fade out). Returns whether the window is visible.
    pub(super) fn show(&mut self, host: &VideoBarHost, target: &VideoBarTarget, prefs: &VideoBarPrefs) -> bool {
        *self.shared.on_input.borrow_mut() = Some(host.on_input.clone());
        // Before the check below: with the pointer on the bar, the bar's own window is the one in front.
        if self.shown {
            self.adopt_native_window();
        }
        let Some((origin, in_front)) = self.host_state(host.host_xid) else {
            self.hide("the host window is not known to X");
            return false;
        };
        // A bar never floats over another application (a drag that left the host keeps it, though).
        if !in_front && !target.held {
            self.hide("another window is in front at the pointer");
            return false;
        }
        if !self.shown {
            self.refresh_glass();
        }
        self.sync_prefs(prefs);
        self.window.set_mini(target.mini);
        self.sync_view(&target.view);
        self.place(target.placement.translated(origin.0, origin.1), prefs);
        if !self.shown {
            if let Err(e) = self.window.show() {
                tracing::warn!(error = %e, "[Multiview: VideoBar] A control bar could not be shown");
                return false;
            }
            self.shown = true;
            tracing::debug!(tile = target.key, "[Multiview: VideoBar] Bar shown");
        }
        self.adopt_native_window();
        self.window.set_revealed(target.phase == Phase::Shown);
        true
    }

    pub(super) fn hide(&mut self, reason: &'static str) {
        if !self.shown {
            return;
        }
        self.window.set_revealed(false);
        if let Err(e) = self.window.hide() {
            tracing::warn!(error = %e, "[Multiview: VideoBar] A control bar could not be hidden");
        }
        self.shown = false;
        self.placed = None;
        self.shared.x.borrow_mut().bubble = None;
        tracing::debug!(reason, "[Multiview: VideoBar] Bar hidden");
    }

    fn sync_prefs(&self, prefs: &VideoBarPrefs) {
        let w = &self.window;
        w.set_pill(prefs.style == BarStyle::Pill);
        w.set_bar_opacity(prefs.opacity);
        w.set_bar_height(prefs.height);
        w.set_bubble_height(bubble_height(prefs.height));
        w.set_fade(prefs.fade);
    }

    fn sync_view(&mut self, view: &VideoBarView) {
        if self.last_view.as_ref() == Some(view) {
            return;
        }
        let w = &self.window;
        w.set_playing(view.playing);
        w.set_current_text(view.times.current.as_str().into());
        w.set_current_template(view.times.current_template.as_str().into());
        w.set_remaining_text(view.times.remaining.as_str().into());
        w.set_remaining_template(view.times.remaining_template.as_str().into());
        w.set_volume_share(view.volume_share);
        w.set_volume_icon(view.volume_icon.code());
        w.set_muted(view.muted);
        match &view.hover {
            Some((share, text)) => {
                w.set_hover_share(*share);
                w.set_hover_text(text.as_str().into());
                w.set_hover_shown(true);
            }
            None => w.set_hover_shown(false),
        }
        let spans: Vec<VideoBarSpanView> = view
            .spans
            .iter()
            .map(|span| VideoBarSpanView {
                from: span.from,
                to: span.to,
                kind: span.kind.code(),
            })
            .collect();
        if self.last_view.as_ref().is_none_or(|last| last.spans != view.spans) {
            sync_model(&self.spans, spans);
        }
        self.last_view = Some(view.clone());
    }

    fn place(&mut self, placement: VideoBarPlacement, prefs: &VideoBarPrefs) {
        let moved = self.placed != Some(placement);
        if moved {
            let window = self.window.window();
            window.set_size(PhysicalSize::new(
                placement.window.w.max(1).saturating_as::<u32>(),
                placement.window.h.max(1).saturating_as::<u32>(),
            ));
            window.set_position(PhysicalPosition::new(placement.window.x, placement.window.y));
            self.placed = Some(placement);
        }
        let scale = self.window.window().scale_factor().max(0.5);
        let mut state = self.shared.x.borrow_mut();
        let bar = Rect::new(
            placement.bar.x - placement.window.x,
            placement.bar.y - placement.window.y,
            placement.bar.w,
            placement.bar.h,
        );
        let radius = match prefs.style {
            BarStyle::Pill => bar.h / 2,
            BarStyle::Bar => 0,
        };
        let changed =
            state.bar != Some(bar) || state.bar_radius != radius || (state.scale - scale).abs() > f32::EPSILON;
        state.bar = Some(bar);
        state.bar_radius = radius;
        state.bubble_radius = (4.0 * prefs.height / 30.0 * scale).round().to_int::<i32>();
        state.scale = scale;
        if (changed || moved)
            && let (Some(server), Some(xid)) = (state.server.as_ref(), state.xid)
        {
            server.raise(xid);
            state.apply_shape();
        }
    }

    /// Slint creates the native window in its event loop, so it may not exist yet right after the first `show`: as soon as it
    /// does, its X id is known, it counts as part of the Multiview, comes to the front and gets its shape.
    fn adopt_native_window(&self) {
        let mut state = self.shared.x.borrow_mut();
        if state.xid.is_some() {
            return;
        }
        state.xid = get_raw_window_xid(self.window.window());
        let Some(xid) = state.xid else {
            return;
        };
        bar_overlay::register_overlay_xid(xid);
        if let Some(server) = state.server.as_ref() {
            server.raise(xid);
        }
        state.apply_shape();
        drop(state);
        // The depth of the window decides whether it can be glass; it is known only now.
        self.refresh_glass();
    }

    /// Where the host window is on the screen, and whether it (or a bar over it) is in front at the pointer.
    fn host_state(&self, host_xid: u64) -> Option<((i32, i32), bool)> {
        let state = self.shared.x.borrow();
        let server = state.server.as_ref()?;
        let origin = server.origin_in_root(host_xid)?;
        let host_top = server.top_level_of(host_xid)?;
        let in_front = server
            .top_level_at_pointer()
            .is_some_and(|front| front == host_top || bar_overlay::is_overlay_xid(front));
        Some((origin, in_front))
    }

    /// Glass if a compositing manager blends the window and the window has an alpha channel, solid otherwise.
    fn refresh_glass(&self) {
        let translucent = {
            let state = self.shared.x.borrow();
            state.server.as_ref().is_some_and(|server| {
                server.compositor_running() && state.xid.is_none_or(|xid| server.window_depth(xid) == Some(32))
            })
        };
        let changed = std::mem::replace(&mut self.shared.x.borrow_mut().translucent, translucent) != translucent;
        if changed {
            tracing::info!(translucent, "[Multiview: VideoBar] Bar style");
        }
        self.window.set_translucent(translucent);
    }
}

/// Hands every click, drag and wheel of the window to the input handler of its host, with the tile it belongs to.
fn connect(window: &MultiviewVideoBarWindow, shared: &Rc<Shared>) {
    let send = {
        let shared = shared.clone();
        move |input: VideoBarInput| {
            let handler = shared.on_input.borrow().clone();
            if let Some(handler) = handler {
                handler(shared.key.get(), input);
            }
        }
    };
    let send = Rc::new(send);
    let s = send.clone();
    window.on_play_pause(move || s(VideoBarInput::PlayPause));
    let s = send.clone();
    window.on_toggle_ms(move || s(VideoBarInput::ToggleMs));
    let s = send.clone();
    window.on_speaker(move || s(VideoBarInput::Speaker));
    let s = send.clone();
    window.on_seek(move |share, phase| {
        if let Some(phase) = DragPhase::from_code(phase) {
            s(VideoBarInput::Seek { share, phase });
        }
    });
    let s = send.clone();
    window.on_hover(move |inside, share| {
        s(VideoBarInput::Hover {
            share: inside.then_some(share),
        });
    });
    let s = send.clone();
    window.on_volume(move |share, phase| {
        if let Some(phase) = DragPhase::from_code(phase) {
            s(VideoBarInput::Volume { share, phase });
        }
    });
    window.on_wheel(move |delta_y| send(VideoBarInput::Wheel { delta_y }));
    let for_bubble = shared.clone();
    window.on_bubble_changed(move |visible, x, y, w, h| {
        let Ok(mut state) = for_bubble.x.try_borrow_mut() else {
            return;
        };
        let scale = state.scale.max(0.5);
        let px = |logical: f32| (logical * scale).round().to_int::<i32>();
        state.bubble = (visible && w > 2.0 && h > 2.0).then(|| Rect::new(px(x), px(y), px(w).max(1), px(h).max(1)));
        state.apply_shape();
    });
}
