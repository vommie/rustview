//! The hint window of the Multiview (UI thread only): a short message centered over the view when a key has nothing to act
//! on ("Select a medium first", `viewer::multiview::hint`).
//!
//! The middle of the view may be a video, a native window that nothing inside the Multiview window can draw over, so the hint
//! is a small window of its own like the action bars: override-redirect (the window manager leaves it alone and it never takes
//! the focus) and blended by the compositing manager; where none runs it is solid and shaped like its disc and caption. The
//! pointer passes through it. It holds and fades like the feedback of a rating (`media_feedback::{HOLD, FADE}`); a new hint
//! replaces the one on screen at once. The window is created on first use and kept until the application ends. Without an X
//! server (Wayland) no hint is shown; the key still does nothing.

use super::bar_overlay::create_override_redirect;
use crate::ui::MultiviewHintWindow;
use crate::viewer::multiview::bar_placement::rounded_region;
use crate::viewer::multiview::hint::{HintAnchor, HintKind};
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::media_feedback::{FADE, HOLD};
use crate::viewer::{OverlayServer, XRectangle, get_raw_window_xid};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize, Timer, TimerMode};
use std::cell::RefCell;

/// What the X side of the window has to get once the native window exists: its size, scale and style.
#[derive(Clone, Copy)]
struct XPending {
    size: (i32, i32),
    scale: f32,
    translucent: bool,
}

struct HintWindow {
    window: MultiviewHintWindow,
    server: Option<OverlayServer>,
    xid: Option<u64>,
    /// Ends the hold (the fade starts) and then the fade (the window goes).
    timer: Timer,
    /// The native window of a window shown for the first time only exists a moment later (in the event loop): its shape and
    /// input region are applied then.
    x_pending: Option<XPending>,
    x_retry: Timer,
    x_attempts: u32,
    shown: bool,
}

/// How often (every `X_RETRY`) the shape of a new window is tried before giving up.
const X_ATTEMPTS: u32 = 40;
const X_RETRY: std::time::Duration = std::time::Duration::from_millis(15);

thread_local! {
    static HINT: RefCell<Option<HintWindow>> = const { RefCell::new(None) };
}

impl HintWindow {
    fn create() -> Option<Self> {
        let window = match create_override_redirect(MultiviewHintWindow::new) {
            Ok(window) => window,
            Err(e) => {
                tracing::error!(error = %e, "[Multiview: Hint] The hint window could not be created");
                return None;
            }
        };
        let server = match OverlayServer::new() {
            Ok(server) => Some(server),
            Err(e) => {
                tracing::info!(error = %e, "[Multiview: Hint] No X connection; hints are not shown");
                None
            }
        };
        window.set_fade(FADE.as_millis().saturating_as::<i64>());
        tracing::info!("[Multiview: Hint] Hint window created");
        Some(Self {
            window,
            server,
            xid: None,
            timer: Timer::default(),
            x_pending: None,
            x_retry: Timer::default(),
            x_attempts: 0,
            shown: false,
        })
    }

    /// The disc and the caption in window pixels (the visible part of a solid window).
    fn shape(&self, size: (i32, i32), scale: f32) -> Vec<XRectangle> {
        let px = |logical: f32| (logical * scale).round().to_int::<i32>();
        let disc = px(self.window.get_disc_size());
        let (caption_w, caption_h) = (
            px(self.window.get_caption_width()),
            px(self.window.get_caption_height()),
        );
        let mut region = rounded_region(Rect::new((size.0 - disc) / 2, 0, disc, disc), disc / 2);
        region.extend(rounded_region(
            Rect::new(
                (size.0 - caption_w) / 2,
                px(self.window.get_caption_top()),
                caption_w,
                caption_h,
            ),
            caption_h / 2,
        ));
        region
    }

    fn show(&mut self, anchor: HintAnchor, kind: HintKind) {
        let Some(server) = self.server.as_ref() else {
            return;
        };
        let Some((root_x, root_y)) = server.origin_in_root(anchor.host_xid) else {
            tracing::debug!("[Multiview: Hint] The host window is gone; no hint");
            return;
        };
        self.window.set_text(kind.text().into());
        let translucent =
            server.compositor_running() && self.xid.is_none_or(|xid| server.window_depth(xid) == Some(32));
        self.window.set_translucent(translucent);
        let scale = anchor.dpi.max(0.5);
        let px = |logical: f32| (logical * scale).round().to_int::<i32>().max(1);
        let size = (
            px(self.window.get_content_width()),
            px(self.window.get_content_height()),
        );
        let (center_x, center_y) = anchor.center();
        let window = self.window.window();
        window.set_size(PhysicalSize::new(
            size.0.saturating_as::<u32>(),
            size.1.saturating_as::<u32>(),
        ));
        window.set_position(PhysicalPosition::new(
            root_x + center_x - size.0 / 2,
            root_y + center_y - size.1 / 2,
        ));
        self.window.set_shown(true);
        if !self.shown {
            if let Err(e) = self.window.show() {
                tracing::warn!(error = %e, "[Multiview: Hint] The hint window could not be shown");
                return;
            }
            self.shown = true;
        }
        self.x_pending = Some(XPending {
            size,
            scale,
            translucent,
        });
        self.x_attempts = 0;
        if !self.apply_x() {
            self.x_retry.start(TimerMode::Repeated, X_RETRY, retry_x);
        }
        tracing::info!(?kind, translucent, "[Multiview: Hint] Shown");
    }

    /// Gives the native window its shape (solid style), an empty input region (the pointer passes through) and puts it on top.
    /// Returns false while the native window does not exist yet.
    fn apply_x(&mut self) -> bool {
        let Some(pending) = self.x_pending else {
            return true;
        };
        if self.xid.is_none() {
            self.xid = get_raw_window_xid(self.window.window());
        }
        let (Some(server), Some(xid)) = (self.server.as_ref(), self.xid) else {
            return false;
        };
        server.set_input_region(xid, &[]);
        let shape = (!pending.translucent).then(|| self.shape(pending.size, pending.scale));
        server.set_bounding_region(xid, shape.as_deref());
        server.raise(xid);
        self.x_pending = None;
        true
    }

    fn hide(&mut self) {
        self.timer.stop();
        self.x_retry.stop();
        self.window.set_shown(false);
        if !self.shown {
            return;
        }
        if let Err(e) = self.window.hide() {
            tracing::warn!(error = %e, "[Multiview: Hint] The hint window could not be hidden");
        }
        self.shown = false;
    }
}

/// Shows the hint `kind` centered over the view of `anchor`: held for `HOLD`, then faded out over `FADE` (a solid window
/// without a compositor goes at the end of the hold). A hint on screen is replaced at once.
pub fn show(anchor: HintAnchor, kind: HintKind) {
    HINT.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            return;
        };
        if slot.is_none() {
            *slot = HintWindow::create();
        }
        let Some(hint) = slot.as_mut() else {
            return;
        };
        hint.show(anchor, kind);
        if hint.shown {
            hint.timer.start(TimerMode::SingleShot, HOLD, end_hold);
        }
    });
}

/// The native window of the hint may exist now: its shape is applied, or tried again a moment later.
fn retry_x() {
    HINT.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            return;
        };
        let Some(hint) = slot.as_mut() else {
            return;
        };
        hint.x_attempts += 1;
        if hint.apply_x() {
            hint.x_retry.stop();
        } else if hint.x_attempts >= X_ATTEMPTS {
            hint.x_retry.stop();
            tracing::warn!("[Multiview: Hint] The native hint window did not appear; it keeps its rectangular shape");
        }
    });
}

/// The hold is over: the content fades (blended windows only), then the window goes.
fn end_hold() {
    HINT.with(|cell| {
        let Ok(mut slot) = cell.try_borrow_mut() else {
            return;
        };
        let Some(hint) = slot.as_mut() else {
            return;
        };
        if hint.window.get_translucent() {
            hint.window.set_shown(false);
            hint.timer.start(TimerMode::SingleShot, FADE, end_fade);
        } else {
            hint.hide();
        }
    });
}

fn end_fade() {
    HINT.with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut()
            && let Some(hint) = slot.as_mut()
        {
            hint.hide();
        }
    });
}

/// The application exits: the hint window goes (Slint destroys it when the event loop ends).
pub fn shutdown() {
    HINT.with(|cell| {
        if let Ok(mut slot) = cell.try_borrow_mut()
            && let Some(mut hint) = slot.take()
        {
            hint.hide();
            tracing::debug!("[Shutdown: Windows] Hint window closed");
        }
    });
}
