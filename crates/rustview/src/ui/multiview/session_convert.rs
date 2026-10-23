//! Ctrl+double click in the fullscreen Multiview: the running Multiview becomes a background tab.
//!
//! Nothing is restarted. The video windows (with their running mpv players), the arrangement, the mute states and the decoded
//! images stay in the instance, which moves from the fullscreen session to the tab. The windows are reparented below the
//! main window BEFORE the fullscreen window is hidden, because hiding a window can destroy its children.

use super::handle;
use super::session::{Session, finish_close, take_session};
use crate::ui::status_bar::StatusBarBridge;
use crate::viewer::multiview::layout::Rect;
use crate::viewer::multiview::route::OriginKind;
use crate::viewer::{TabId, get_slint_window_xid};
use slint::ComponentHandle;

pub(super) fn convert_to_tab() {
    let session = match take_session() {
        Ok(Some(session)) => session,
        Ok(None) => return,
        Err(()) => {
            let _ = slint::invoke_from_event_loop(convert_to_tab);
            return;
        }
    };
    match adopt(&session) {
        Ok(tab_id) => complete(session, tab_id),
        Err(reason) => {
            tracing::warn!(
                reason,
                "[Multiview: Convert] Could not move the Multiview into a tab; closing it instead"
            );
            finish_close(session, true);
        }
    }
}

/// Parks the windows below the main window and hands the instance to the tab manager.
fn adopt(session: &Session) -> Result<TabId, String> {
    if !session.populated {
        return Err("the Multiview is still opening".to_string());
    }
    let tab_manager = session.handle.tab_manager().ok_or("no tab manager is bound")?;
    session.instance.lock().set_origin(OriginKind::Tab);
    park_below_main_window(session)?;
    Ok(tab_manager.adopt_multiview_in_background(session.instance.clone()))
}

/// Moves the video windows of the session's instance below the main window (hidden and paused), so the fullscreen window can
/// go away without taking them along.
pub(super) fn park_below_main_window(session: &Session) -> Result<(), String> {
    let tab_manager = session.handle.tab_manager().ok_or("no tab manager is bound")?;
    let main_window = session
        .handle
        .main_window()
        .and_then(|weak| weak.upgrade())
        .ok_or("the main window is gone")?;
    let main_xid = get_slint_window_xid(main_window.window()).ok_or("the main window has no native handle")?;
    let (x, y, width, height) = tab_manager.get_viewport_rect();
    session
        .instance
        .lock()
        .park(main_xid, Rect::new(x, y, width as i32, height as i32))
}

/// The instance lives in the tab now: the fullscreen window goes away and the application returns to the Browse view with
/// the new tab in the tab bar.
fn complete(session: Session, tab_id: TabId) {
    let handle = session.handle.clone();
    let instance_id = session.instance.lock().id();
    let title = handle
        .tab_manager()
        .and_then(|tm| tm.get_tab(tab_id))
        .map(|tab| tab.title)
        .unwrap_or_default();
    session.shutdown();
    handle::set_open(false);
    tracing::info!(
        tab_id,
        instance = instance_id,
        "[Multiview: Convert] The Multiview moved into a background tab"
    );
    handle.notify_closed(None);
    if let Some(main_window) = handle.main_window() {
        let _ = main_window.upgrade_in_event_loop(move |w| {
            StatusBarBridge::success(&w, format!("Multiview moved to tab '{title}'"));
        });
    }
}
