//! The keys of the fullscreen window and of the windows of its videos: the keymap's Fullscreen area (`docs/hotkeys.md`).
//!
//! Every action runs through the window's own callbacks (`MultiviewCommand`s on the selected medium), so a key and its button
//! share one path. A key a video's window reported arrives as a chord through the router.

use super::session;
use crate::ui::hotkeys::{Resolved, Trigger, resolve};
use crate::ui::keymap::{Action, Area, KeyPress};
use crate::viewer::multiview::command::MultiviewCommand;

/// Connects the key callback of the fullscreen window.
pub(super) fn install(win: &crate::ui::MultiviewFullscreenWindow) {
    win.on_hotkey(|text, control, alt, shift, meta, repeat| {
        let press = KeyPress {
            text: text.as_str(),
            control,
            alt,
            shift,
            meta,
            repeat,
        };
        dispatch(&Trigger::Press(press))
    });
}

/// Runs the action `trigger` stands for in the fullscreen; `false` when it has none (the key goes on).
pub(super) fn dispatch(trigger: &Trigger<'_>) -> bool {
    let Some(win) = session::window_weak().and_then(|w| w.upgrade()) else {
        return false;
    };
    // The video player keys come first while the fullscreen shows a video (Left and Right page only a single picture).
    let (has_video, ab_repeat) = session::instance().map_or((false, false), |instance| {
        let guard = instance.lock();
        (guard.has_video(), guard.ab_repeat_active())
    });
    let areas: &[Area] = match (has_video, ab_repeat) {
        (true, true) => &[Area::AbRepeat, Area::Player, Area::Fullscreen],
        (true, false) => &[Area::Player, Area::Fullscreen],
        _ => &[Area::Fullscreen],
    };
    let action = match resolve(trigger, areas, false) {
        Resolved::Pass => return false,
        Resolved::Swallow => return true,
        Resolved::Run(action) => action,
    };
    tracing::debug!(action = action.id(), "[Hotkeys] Fullscreen");
    if let Some(rating) = action.rating() {
        if let Some(handle) = session::handle() {
            handle.rate_fullscreen(rating);
        }
        return true;
    }
    if let Some(button) = action.media_bar() {
        session::run(|s| s.command(MultiviewCommand::Bar(button)));
        return true;
    }
    if let Some(command) = action.player() {
        session::run(|s| s.command(MultiviewCommand::Player(command)));
        return true;
    }
    if let Some(button) = action.primary_bar() {
        session::run(|s| s.on_primary_action(button.code()));
        return true;
    }
    match action {
        Action::FullscreenRename => win.invoke_request_rename(),
        Action::FullscreenDelete => win.invoke_request_delete(),
        Action::FullscreenRefresh => refresh_selected(),
        Action::FullscreenZoomReset => win.invoke_zoom_key(0),
        Action::FullscreenZoomIn => win.invoke_zoom_key(1),
        Action::FullscreenZoomOut => win.invoke_zoom_key(2),
        Action::FullscreenApplyCrop => return win.invoke_edit_key(1),
        Action::FullscreenSave => return win.invoke_edit_key(2),
        Action::FullscreenSaveAs => return win.invoke_edit_key(3),
        Action::FullscreenUndo => return win.invoke_edit_key(4),
        Action::FullscreenRedo => return win.invoke_edit_key(5),
        Action::FullscreenPrevious | Action::FullscreenNext => {
            // Only a single image of a Multiview opened from Browse pages.
            if win.get_navigation() == 0 {
                return false;
            }
            win.invoke_navigate(action == Action::FullscreenNext);
        }
        Action::FullscreenClose => win.invoke_request_close(),
        _ => return false,
    }
    true
}

/// The refresh key: the selected medium is read again from the drive (an explicit user action), through the same callback
/// of the main window as the refresh key of the tab view.
fn refresh_selected() {
    let Some(instance) = session::instance() else {
        return;
    };
    let Some(media_id) = super::commands::target_media(&instance) else {
        return;
    };
    if let Some(handle) = session::handle() {
        handle.refresh_medium(media_id);
    }
}
