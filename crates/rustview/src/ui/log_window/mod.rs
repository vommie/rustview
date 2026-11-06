//! The log window (`docs/debug_page.md`, `ui/log_window.slint`): the lines `rustview_core::logging` keeps in memory, live. A window
//! of its own that locks nothing; it polls the ring four times a second while it is shown and stops when it is hidden. Its place,
//! size, filter, view and "follow" are kept in `view.log_window`.

use crate::system::{SystemClipboard, SystemFileHandler, app_dirs};
use crate::ui::{LogLineView, LogWindow, TooltipSettings};
use rustview_core::logging::{self, LogLine, RING_CAPACITY};
use rustview_core::settings::registry::SettingsManager;
use rustview_core::settings::schema::LogWindowState;
use rustview_core::util::cast::{Az, FloatToInt, SaturatingAs};
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::{Cell, RefCell};
use std::fmt::Write as _;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tracing::Level;

const POLL: Duration = Duration::from_millis(250);
const FILTERS: [&str; 3] = ["all", "warnings", "errors"];
/// The names of the levels of [`level_index`].
const LEVEL_NAMES: [&str; 5] = ["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];

thread_local! {
    static WINDOW: RefCell<Option<Rc<LogWindowController>>> = const { RefCell::new(None) };
}

struct LogWindowController {
    window: LogWindow,
    manager: Arc<SettingsManager>,
    lines: Rc<VecModel<LogLineView>>,
    last_seq: Cell<u64>,
    timer: slint::Timer,
}

/// Opens the log window, or brings it to the front when it is open.
pub fn open(manager: &Arc<SettingsManager>) {
    let existing = WINDOW.with(|cell| cell.borrow().clone());
    let controller = match existing {
        Some(controller) => controller,
        None => match create(manager) {
            Some(controller) => controller,
            None => return,
        },
    };
    controller.show();
}

/// Hides the log window and keeps its place (the shutdown and the window's own close button).
pub fn close() {
    if let Some(controller) = WINDOW.with(|cell| cell.borrow().clone()) {
        controller.hide();
    }
}

fn level_index(level: Level) -> i32 {
    match level {
        Level::ERROR => 0,
        Level::WARN => 1,
        Level::INFO => 2,
        Level::DEBUG => 3,
        _ => 4,
    }
}

/// Whether a line passes the filter (0 = every line, 1 = warnings and errors, 2 = errors).
fn wanted(line: &LogLine, filter: i32) -> bool {
    match filter {
        1 => line.at_least(Level::WARN),
        2 => line.at_least(Level::ERROR),
        _ => true,
    }
}

fn view(line: &LogLine, private: bool) -> LogLineView {
    LogLineView {
        time: line.time.format("%H:%M:%S%.3f").to_string().into(),
        level: level_index(line.level),
        text: line.text(private).into(),
    }
}

fn create(manager: &Arc<SettingsManager>) -> Option<Rc<LogWindowController>> {
    let window = match LogWindow::new() {
        Ok(window) => window,
        Err(error) => {
            tracing::warn!(%error, "[LogWindow] The log window cannot be created");
            return None;
        }
    };
    crate::ui::tooltip::install(&window);
    let settings = manager.get();
    let tooltips = window.global::<TooltipSettings>();
    tooltips.set_enabled(settings.appearance.enable_tooltips);
    tooltips.set_default_delay(i64::from(settings.appearance.tooltip_delay_ms));
    let state = &settings.view.log_window;
    window.set_filter_idx(
        FILTERS
            .iter()
            .position(|f| *f == state.filter)
            .unwrap_or(0)
            .saturating_as::<i32>(),
    );
    window.set_private_view(state.private_view);
    window.set_follow(state.follow);
    let lines = Rc::new(VecModel::default());
    window.set_lines(ModelRc::from(lines.clone()));
    let controller = Rc::new(LogWindowController {
        window,
        manager: manager.clone(),
        lines,
        last_seq: Cell::new(0),
        timer: slint::Timer::default(),
    });
    install(&controller);
    WINDOW.with(|cell| *cell.borrow_mut() = Some(controller.clone()));
    Some(controller)
}

fn install(controller: &Rc<LogWindowController>) {
    let window = &controller.window;
    let weak = Rc::downgrade(controller);
    window.on_filter_changed(move |_| {
        if let Some(c) = weak.upgrade() {
            c.remember();
            c.reload();
        }
    });
    let weak = Rc::downgrade(controller);
    window.on_private_toggled(move |_| {
        if let Some(c) = weak.upgrade() {
            c.remember();
            c.reload();
        }
    });
    let weak = Rc::downgrade(controller);
    window.on_follow_toggled(move |follow| {
        if let Some(c) = weak.upgrade() {
            c.remember();
            if follow {
                c.scroll_to_end_soon();
            }
        }
    });
    let weak = Rc::downgrade(controller);
    window.on_copy_shown(move || {
        if let Some(c) = weak.upgrade() {
            let text = c.lines.iter().fold(String::new(), |mut text, line| {
                let level = LEVEL_NAMES
                    .get(line.level.saturating_as::<usize>())
                    .copied()
                    .unwrap_or("TRACE");
                let _ = writeln!(text, "{} {level:>5} {}", line.time, line.text);
                text
            });
            if let Err(error) = SystemClipboard::set_text(&text) {
                tracing::warn!(%error, "[LogWindow] The lines could not be copied");
            }
        }
    });
    window.on_open_folder(|| {
        let dir = app_dirs::log_dir();
        let _ = std::fs::create_dir_all(&dir);
        if let Err(error) = SystemFileHandler::open_folder(&dir) {
            tracing::warn!(%error, "[LogWindow] The folder of the log files cannot be opened");
        }
    });
    let weak = Rc::downgrade(controller);
    window.window().on_close_requested(move || {
        if let Some(c) = weak.upgrade() {
            c.timer.stop();
            c.remember();
        }
        slint::CloseRequestResponse::HideWindow
    });
}

impl LogWindowController {
    fn show(self: &Rc<Self>) {
        if !self.window.window().is_visible() {
            let state = self.manager.get().view.log_window;
            self.window.window().set_size(slint::LogicalSize::new(
                state.width.az::<f32>(),
                state.height.az::<f32>(),
            ));
            if let (Some(x), Some(y)) = (state.x, state.y) {
                self.window.window().set_position(slint::PhysicalPosition::new(x, y));
            }
            self.reload();
            let weak = Rc::downgrade(self);
            self.timer.start(slint::TimerMode::Repeated, POLL, move || {
                if let Some(c) = weak.upgrade() {
                    c.poll();
                }
            });
            tracing::info!("[LogWindow] Opened");
        }
        if let Err(error) = self.window.show() {
            tracing::warn!(%error, "[LogWindow] The log window cannot be shown");
        }
    }

    fn hide(&self) {
        if self.window.window().is_visible() {
            self.timer.stop();
            self.remember();
            let _ = self.window.hide();
        }
    }

    /// Keeps the place, size, filter, view and "follow" in the settings (at once, without a Save).
    fn remember(&self) {
        let window = self.window.window();
        let size = window.size().to_logical(window.scale_factor());
        let position = window.position();
        let mut settings = self.manager.get();
        let previous = settings.view.log_window.clone();
        let mut state = LogWindowState {
            x: Some(position.x),
            y: Some(position.y),
            width: size.width.round().to_int::<u32>(),
            height: size.height.round().to_int::<u32>(),
            filter: FILTERS
                .get(self.window.get_filter_idx().saturating_as::<usize>())
                .copied()
                .unwrap_or("all")
                .to_owned(),
            private_view: self.window.get_private_view(),
            follow: self.window.get_follow(),
        };
        if !window.is_visible() {
            // A hidden window has no place of its own: keep the last one.
            (state.x, state.y, state.width, state.height) = (previous.x, previous.y, previous.width, previous.height);
        }
        state.sanitize();
        if state != previous {
            settings.view.log_window = state;
            self.manager.save_quiet_async(&settings);
        }
    }

    /// Every line of the ring that passes the filter, in the chosen view.
    fn reload(&self) {
        let (filter, private) = (self.window.get_filter_idx(), self.window.get_private_view());
        let all = logging::ring().since(0);
        self.last_seq.set(all.last().map_or(0, |line| line.seq));
        let views: Vec<LogLineView> = all
            .iter()
            .filter(|l| wanted(l, filter))
            .map(|l| view(l, private))
            .collect();
        self.lines.set_vec(views);
        self.show_summary();
        if self.window.get_follow() {
            self.scroll_to_end_soon();
        }
    }

    /// Adds the lines written since the last look.
    fn poll(&self) {
        let new = logging::ring().since(self.last_seq.get());
        let Some(last) = new.last() else { return };
        self.last_seq.set(last.seq);
        let (filter, private) = (self.window.get_filter_idx(), self.window.get_private_view());
        let mut added = false;
        for line in new.iter().filter(|l| wanted(l, filter)) {
            self.lines.push(view(line, private));
            added = true;
        }
        let excess = self.lines.row_count().saturating_sub(RING_CAPACITY);
        if excess > 0 {
            let kept: Vec<LogLineView> = self.lines.iter().skip(excess).collect();
            self.lines.set_vec(kept);
        }
        if added {
            self.show_summary();
            if self.window.get_follow() {
                self.scroll_to_end_soon();
            }
        }
    }

    fn show_summary(&self) {
        let count = self.lines.row_count();
        let lines = if count == 1 {
            "1 line".to_owned()
        } else {
            format!("{} lines", rustview_core::telemetry::format_count(count))
        };
        let files = if logging::sinks().files_running() {
            "log files on"
        } else {
            "log files off"
        };
        self.window.set_summary(format!("{lines}, {files}").into());
    }

    /// Scrolls to the newest line once the list has laid out the new rows.
    fn scroll_to_end_soon(&self) {
        let weak = self.window.as_weak();
        slint::Timer::single_shot(Duration::from_millis(15), move || {
            if let Some(window) = weak.upgrade() {
                window.invoke_scroll_to_end();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{level_index, wanted};
    use rustview_core::logging::LogRing;
    use tracing::Level;

    #[test]
    fn the_filter_keeps_what_it_says() {
        let ring = LogRing::new(8);
        let info = ring.push(chrono::Local::now(), Level::INFO, "i".into(), "i".into());
        let warn = ring.push(chrono::Local::now(), Level::WARN, "w".into(), "w".into());
        let error = ring.push(chrono::Local::now(), Level::ERROR, "e".into(), "e".into());
        assert!(wanted(&info, 0) && !wanted(&info, 1) && !wanted(&info, 2));
        assert!(wanted(&warn, 1) && !wanted(&warn, 2));
        assert!(wanted(&error, 2));
        assert_eq!(level_index(Level::TRACE), 4);
    }
}
