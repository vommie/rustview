//! What the buttons of the Debug page do. Everything that looks at the system runs on a worker thread and reports back to the
//! UI thread; nothing runs without a click.

use super::{DebugPageController, report, show_checks, toggle, views};
use crate::system::diagnostics;
use crate::system::{SystemClipboard, SystemFileHandler, app_dirs};
use crate::ui::{DebugUi, SettingsWindow};
use crate::viewer::mpv_engine::self_test::{self, SelfTestOutcome};
use rustview_core::diagnostics::{Check, CheckReport, Group, Status};
use rustview_core::settings::LogObscurer;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;

pub(super) fn install(page: &Rc<DebugPageController>, window: &SettingsWindow) {
    let ui = window.global::<DebugUi>();

    let weak = Rc::downgrade(page);
    ui.on_card_toggled(move |id| {
        if let Some(page) = weak.upgrade() {
            page.change_page_state(|state| toggle(&mut state.open_cards, &id));
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_passed_toggled(move |id| {
        if let Some(page) = weak.upgrade() {
            page.change_page_state(|state| toggle(&mut state.passed_shown, &id));
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_developer_toggled(move || {
        if let Some(page) = weak.upgrade() {
            page.change_page_state(|state| state.developer_open = !state.developer_open);
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_run_checks(move || {
        if let Some(page) = weak.upgrade() {
            run_checks(&page, None);
        }
    });
    ui.on_copy_text(|text| SystemClipboard::set_text(&text).is_ok());
    let window_weak = window.as_weak();
    ui.on_jump_to_page(move |index| {
        if let Some(window) = window_weak.upgrade() {
            window.set_active_category(index);
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_open_log_window(move || {
        if let Some(context) = weak.upgrade().and_then(|page| page.context()) {
            (context.open_log_window)();
        }
    });
    ui.on_open_folder(|id| open_folder(&id));
    let weak = Rc::downgrade(page);
    ui.on_copy_report(move || {
        if let Some(page) = weak.upgrade() {
            make_report(&page, None);
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_save_report(move || {
        let Some(page) = weak.upgrade() else { return };
        let Some(window) = page.window() else { return };
        let name = format!("rustview-report-{}.txt", chrono::Local::now().format("%Y-%m-%d-%H%M"));
        let target = crate::ui::dialog_parent::file_dialog(window.window())
            .set_title("Save the diagnostic report")
            .set_file_name(&name)
            .save_file();
        if let Some(path) = target {
            make_report(&page, Some(path));
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_run_self_test(move || {
        if let Some(page) = weak.upgrade() {
            run_self_test(&page);
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_reset_window(move || {
        if let Some(context) = weak.upgrade().and_then(|page| page.context()) {
            (context.reset_window)();
        }
    });
    let weak = Rc::downgrade(page);
    ui.on_reset_layout(move || {
        if let Some(context) = weak.upgrade().and_then(|page| page.context()) {
            (context.reset_layout)();
        }
    });
}

/// Opens one of the folders of `RustView` (the folder of the log files is made first: opening it is the user's wish).
fn open_folder(id: &str) {
    let path = match id {
        "settings" => app_dirs::config_dir(),
        "library" => app_dirs::data_dir(),
        "logs" => app_dirs::log_dir(),
        _ => return,
    };
    if let Err(error) = std::fs::create_dir_all(&path) {
        tracing::warn!(%error, folder = %path.display(), "[SettingsDialog: Debug] The folder cannot be made");
    }
    if let Err(error) = SystemFileHandler::open_folder(&path) {
        tracing::warn!(%error, folder = %path.display(), "[SettingsDialog: Debug] The folder cannot be opened");
    }
}

/// Runs the checks on a worker thread and shows them; `then` runs on the worker with the result (the report uses it).
fn run_checks(page: &Rc<DebugPageController>, then: Option<Box<dyn FnOnce(Arc<CheckReport>) + Send>>) {
    let Some(context) = page.check_context() else { return };
    if page.shared.checking.swap(true, Ordering::AcqRel) {
        return;
    }
    if let Some(window) = page.window() {
        window.global::<DebugUi>().set_checking(true);
    }
    tracing::info!("[SettingsDialog: Debug] Checks started");
    let shared = page.shared.clone();
    let window = page.window.borrow().clone();
    let manager = page.manager.clone();
    let spawned = std::thread::Builder::new().name("debug-checks".into()).spawn(move || {
        let report = Arc::new(diagnostics::run(&context));
        *shared.report.lock() = Some(report.clone());
        shared.checking.store(false, Ordering::Release);
        let shared_ui = shared.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(window) = window.and_then(|weak| weak.upgrade()) else {
                return;
            };
            show_checks(&window, &shared_ui, &manager);
            window.global::<DebugUi>().set_checking(false);
        });
        if let Some(then) = then {
            then(report);
        }
    });
    if let Err(error) = spawned {
        page.shared.checking.store(false, Ordering::Release);
        tracing::warn!(%error, "[SettingsDialog: Debug] The checks could not start");
    }
}

/// Builds the report (running the checks first when they never ran) and copies it (`target` = `None`) or saves it.
fn make_report(page: &Rc<DebugPageController>, target: Option<PathBuf>) {
    let settings = page.manager.get();
    let runtime = page.runtime_rows();
    let window = page.window.borrow().clone();
    let deliver = move |checks: Arc<CheckReport>| {
        let private = LogObscurer::is_enabled();
        let inputs = report::ReportInputs {
            checks: &checks,
            settings: &settings,
            runtime: &runtime,
        };
        let text = report::build(&inputs, private);
        let (note, error) = match &target {
            None => match SystemClipboard::set_text(&text) {
                Ok(()) => ("Copied to the clipboard".to_owned(), false),
                Err(e) => (format!("The clipboard refused it: {e}"), true),
            },
            Some(path) => match std::fs::write(path, &text) {
                Ok(()) => (format!("Saved as {}", path.display()), false),
                Err(e) => (format!("Could not save it: {e}"), true),
            },
        };
        tracing::info!(
            saved = target.is_some(),
            private,
            failed = error,
            "[SettingsDialog: Debug] Diagnostic report made"
        );
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(window) = window.and_then(|weak| weak.upgrade()) {
                let ui = window.global::<DebugUi>();
                ui.set_report_note(note.into());
                ui.set_report_note_error(error);
            }
        });
    };
    let last = page.shared.report.lock().clone();
    match last {
        Some(checks) => {
            let spawned = std::thread::Builder::new()
                .name("debug-report".into())
                .spawn(move || deliver(checks));
            if let Err(error) = spawned {
                tracing::warn!(%error, "[SettingsDialog: Debug] The report could not start");
            }
        }
        None => run_checks(page, Some(Box::new(deliver))),
    }
}

/// The self-test's outcome as rows of the Tools card.
pub(super) fn self_test_checks(outcome: &SelfTestOutcome) -> Vec<Check> {
    let row = |id, status, title: String, detail: String| Check::new(id, Group::Health, status, title, detail);
    if let Some(failure) = &outcome.failure
        && outcome.first_frame_ms.is_none()
    {
        return vec![row(
            "self_test",
            Status::Problem,
            "The test did not run".to_owned(),
            failure.clone(),
        )];
    }
    let mut rows = Vec::new();
    rows.push(match outcome.first_frame_ms {
        Some(ms) => row(
            "first_frame",
            Status::Ok,
            format!("First frame after {ms} ms"),
            String::new(),
        ),
        None => row(
            "first_frame",
            Status::Problem,
            "No frame was shown".to_owned(),
            "mpv could not draw the test picture (its errors are below and in the log)".to_owned(),
        ),
    });
    if let Some(path) = &outcome.render_path {
        let gl = outcome.gl_renderer.as_deref().unwrap_or("?");
        let decoding = outcome
            .decoding
            .as_deref()
            .map_or_else(String::new, |d| format!(", decoder {d}"));
        rows.push(row(
            "drawn_with",
            Status::Ok,
            format!("Drawn with {path}"),
            format!("{gl}{decoding}"),
        ));
    }
    match outcome.picture_ok {
        Some(true) | None => {}
        Some(false) => rows.push(row(
            "picture",
            Status::Problem,
            "The picture stayed empty".to_owned(),
            "mpv rendered frames, but they hold one color only (a black video)".to_owned(),
        )),
    }
    if outcome.first_frame_ms.is_some() {
        rows.push(if outcome.dropped > 0 {
            row(
                "frames",
                Status::Warning,
                format!(
                    "{} of {} frames dropped",
                    outcome.dropped,
                    outcome.frames + outcome.dropped
                ),
                "the display could not keep up".to_owned(),
            )
        } else {
            row(
                "frames",
                Status::Ok,
                format!("{} frames, none dropped", outcome.frames),
                String::new(),
            )
        });
    }
    if !outcome.errors.is_empty() {
        let shown: Vec<&str> = outcome.errors.iter().take(3).map(String::as_str).collect();
        rows.push(row(
            "mpv_errors",
            Status::Warning,
            "mpv reported errors".to_owned(),
            shown.join("; "),
        ));
    }
    rows
}

/// Plays the test picture through mpv's render API (in the share group of the main window, not shown) and shows what happened.
fn run_self_test(page: &Rc<DebugPageController>) {
    if page.shared.self_test.swap(true, Ordering::AcqRel) {
        return;
    }
    let group = crate::viewer::video_surface::hosts::share_group(crate::viewer::video_surface::MAIN_WINDOW);
    if let Some(window) = page.window() {
        let ui = window.global::<DebugUi>();
        ui.set_self_test_running(true);
        ui.set_self_test_results(views::self_test_rows(&[]));
    }
    let shared = page.shared.clone();
    let manager = page.manager.clone();
    let window = page.window.borrow().clone();
    let spawned = std::thread::Builder::new()
        .name("debug-self-test".into())
        .spawn(move || {
            let outcome = self_test::run(group);
            if let (Some(path), Some(decoding)) = (&outcome.render_path, &outcome.decoding) {
                diagnostics::graphics::record_video_path(&format!("{path}, decoder {decoding}"));
            }
            shared.self_test.store(false, Ordering::Release);
            let _ = slint::invoke_from_event_loop(move || {
                let Some(window) = window.and_then(|weak| weak.upgrade()) else {
                    return;
                };
                let ui = window.global::<DebugUi>();
                ui.set_self_test_running(false);
                ui.set_self_test_results(views::self_test_rows(&self_test_checks(&outcome)));
                // The test may have told the health card how videos draw.
                show_checks(&window, &shared, &manager);
            });
        });
    if let Err(error) = spawned {
        page.shared.self_test.store(false, Ordering::Release);
        tracing::warn!(%error, "[SettingsDialog: Debug] The self-test could not start");
    }
}

#[cfg(test)]
mod tests {
    use super::self_test_checks;
    use crate::viewer::mpv_engine::self_test::SelfTestOutcome;
    use rustview_core::diagnostics::Status;

    #[test]
    fn the_self_test_outcome_becomes_rows() {
        let good = SelfTestOutcome {
            first_frame_ms: Some(180),
            render_path: Some("mpv's render API, GLX".into()),
            video_output: Some("libmpv".into()),
            picture_ok: Some(true),
            frames: 90,
            ..SelfTestOutcome::default()
        };
        let rows = self_test_checks(&good);
        assert!(rows.iter().all(|r| r.status == Status::Ok), "{rows:?}");
        assert_eq!(rows[0].title, "First frame after 180 ms");

        let dropping = SelfTestOutcome {
            dropped: 30,
            frames: 60,
            errors: vec!["vo/gpu: Failed".into()],
            ..good
        };
        let rows = self_test_checks(&dropping);
        assert!(
            rows.iter()
                .any(|r| r.title == "30 of 90 frames dropped" && r.status == Status::Warning)
        );
        assert!(rows.iter().any(|r| r.id == "mpv_errors"));

        let failed = SelfTestOutcome {
            failure: Some("libmpv does not start".into()),
            ..SelfTestOutcome::default()
        };
        assert_eq!(self_test_checks(&failed)[0].status, Status::Problem);
    }
}
