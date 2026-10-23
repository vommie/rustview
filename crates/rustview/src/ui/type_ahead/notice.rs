//! The "No match" message of the status bar: shown while nothing starts with the typed text, and taken back (the status bar shows
//! again what it showed before) after `TYPE_AHEAD_NO_MATCH_MESSAGE_MS`, when a search finds something or ends. UI thread only.

use crate::ui::MainWindow;
use crate::ui::status_bar::{StatusBarBridge, StatusMessageLevel};
use rustview_core::settings::schema::TYPE_AHEAD_NO_MATCH_MESSAGE_MS;
use slint::{ComponentHandle, SharedString, Timer, TimerMode};
use std::cell::RefCell;
use std::time::Duration;

#[derive(Default)]
struct Notice {
    timer: Timer,
    /// What the status bar showed before the notice (`None` while no notice is up).
    saved: Option<(SharedString, i32)>,
    shown: SharedString,
}

thread_local! {
    static NOTICE: RefCell<Notice> = RefCell::new(Notice::default());
}

pub fn show(window: &MainWindow, text: String) {
    NOTICE.with(|cell| {
        let mut notice = cell.borrow_mut();
        // Another message that replaced ours in the meantime is what to go back to.
        if notice.saved.is_none() || window.get_status_text() != notice.shown {
            notice.saved = Some((window.get_status_text(), window.get_status_level()));
        }
        StatusBarBridge::set_message(window, text, StatusMessageLevel::Info);
        notice.shown = window.get_status_text();
        let weak = window.as_weak();
        notice.timer.start(
            TimerMode::SingleShot,
            Duration::from_millis(TYPE_AHEAD_NO_MATCH_MESSAGE_MS),
            move || {
                if let Some(window) = weak.upgrade() {
                    clear(&window);
                }
            },
        );
    });
}

/// Takes the notice back, unless something else has replaced it.
pub fn clear(window: &MainWindow) {
    NOTICE.with(|cell| {
        let mut notice = cell.borrow_mut();
        notice.timer.stop();
        if let Some((text, level)) = notice.saved.take()
            && window.get_status_text() == notice.shown
        {
            window.set_status_level(level);
            window.set_status_text(text);
        }
    });
}
