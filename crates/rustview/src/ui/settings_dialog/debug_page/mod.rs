//! Settings > Debug (`docs/debug_page.md`): the checks, the log options, the diagnostic report, the tools, the startup timeline
//! and the live runtime values. Checks and the report run on worker threads and only when the user clicked; what the app
//! knows in memory is shown at once. Which cards are open is kept at once (`view.debug_page`), the log options with Save.

mod actions;
mod report;
mod runtime;
mod views;

use crate::system::app_dirs;
use crate::system::diagnostics::{self, CheckContext};
use crate::ui::{DebugFolderView, DebugInfoRow, DebugStageView, DebugUi, SettingsWindow};
use parking_lot::Mutex;
use rustview_core::diagnostics::{Check, CheckReport};
use rustview_core::library::Library;
use rustview_core::logging::{self, LogLevel};
use rustview_core::settings::memory_budget::{MemoryPlan, SystemMemory};
use rustview_core::settings::registry::SettingsManager;
use rustview_core::settings::schema::{AppSettings, DebugPageState};
use rustview_core::thumbnails::ThumbnailManager;
use rustview_core::util::cast::{Az, SaturatingAs};
use rustview_core::util::startup_clock;
use slint::{ComponentHandle, Model, ModelRc, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The index of the page Debug in the sidebar of the settings window (`ui/settings_sidebar.slint`).
pub const DEBUG_PAGE: i32 = 11;

/// What the page needs from the rest of the app (set once by the owner of the settings window).
pub struct DebugContext {
    pub library: Arc<Library>,
    pub thumbnails: Arc<ThumbnailManager>,
    pub open_log_window: Rc<dyn Fn()>,
    pub reset_window: Rc<dyn Fn()>,
    pub reset_layout: Rc<dyn Fn()>,
}

/// State the workers share with the page.
#[derive(Default)]
struct Shared {
    report: Mutex<Option<Arc<CheckReport>>>,
    checking: AtomicBool,
    self_test: AtomicBool,
}

pub struct DebugPageController {
    manager: Arc<SettingsManager>,
    window: RefCell<Option<slint::Weak<SettingsWindow>>>,
    context: RefCell<Option<Rc<DebugContext>>>,
    shared: Arc<Shared>,
    runtime_rows: Rc<VecModel<DebugInfoRow>>,
    timer: slint::Timer,
}

impl DebugPageController {
    pub fn new(manager: Arc<SettingsManager>) -> Rc<Self> {
        Rc::new(Self {
            manager,
            window: RefCell::new(None),
            context: RefCell::new(None),
            shared: Arc::new(Shared::default()),
            runtime_rows: Rc::new(VecModel::default()),
            timer: slint::Timer::default(),
        })
    }

    pub fn set_context(&self, context: DebugContext) {
        *self.context.borrow_mut() = Some(Rc::new(context));
    }

    fn window(&self) -> Option<SettingsWindow> {
        self.window.borrow().as_ref().and_then(slint::Weak::upgrade)
    }

    fn context(&self) -> Option<Rc<DebugContext>> {
        self.context.borrow().clone()
    }

    /// Registers the callbacks of the page on a freshly created settings window.
    pub fn install(self: &Rc<Self>, window: &SettingsWindow) {
        *self.window.borrow_mut() = Some(window.as_weak());
        window
            .global::<DebugUi>()
            .set_runtime(ModelRc::from(self.runtime_rows.clone()));
        actions::install(self, window);
    }

    /// Fills the page when the settings window opens: the log options, the open cards and what is known in memory.
    pub fn populate(&self, window: &SettingsWindow, settings: &AppSettings) {
        window.set_log_level_idx(
            LogLevel::from_setting(&settings.debug.log_level)
                .index()
                .saturating_as::<i32>(),
        );
        window.set_log_files(settings.debug.log_files);
        window.set_mpv_log(settings.debug.mpv_log);
        show_page_state(window, &settings.view.debug_page);
        let ui = window.global::<DebugUi>();
        ui.set_env_override(logging::env_override().unwrap_or_default().into());
        ui.set_log_folder(app_dirs::log_dir().display().to_string().into());
        ui.set_report_summary("copy or save it for a bug report".into());
        ui.set_folders(folder_views());
        self.refresh(window);
    }

    /// Writes the log options of the page into `current` (Save).
    pub fn save(window: &SettingsWindow, current: &mut AppSettings) {
        let level = LogLevel::from_index(window.get_log_level_idx().saturating_as::<usize>());
        level.as_str().clone_into(&mut current.debug.log_level);
        current.debug.log_files = window.get_log_files();
        current.debug.mpv_log = window.get_mpv_log();
    }

    /// The page was opened or left: what is known in memory is shown again, and the live values run while it is shown.
    pub fn page_shown(self: &Rc<Self>, shown: bool) {
        if !shown {
            self.timer.stop();
            return;
        }
        if let Some(window) = self.window() {
            self.refresh(&window);
        }
        let weak = Rc::downgrade(self);
        self.timer
            .start(slint::TimerMode::Repeated, Duration::from_secs(1), move || {
                let Some(page) = weak.upgrade() else { return };
                match page.window() {
                    Some(window) if window.window().is_visible() && window.get_active_category() == DEBUG_PAGE => {
                        page.refresh_live(&window);
                    }
                    _ => page.timer.stop(),
                }
            });
    }

    /// Everything the page shows from memory: the checks (the last run plus the facts in memory), summaries, startup, runtime.
    fn refresh(&self, window: &SettingsWindow) {
        show_checks(window, &self.shared, &self.manager);
        let ui = window.global::<DebugUi>();
        ui.set_checking(self.shared.checking.load(Ordering::Acquire));
        ui.set_self_test_running(self.shared.self_test.load(Ordering::Acquire));
        show_startup(window);
        self.refresh_live(window);
    }

    /// The values that change while the page is open (every second).
    fn refresh_live(&self, window: &SettingsWindow) {
        let ui = window.global::<DebugUi>();
        ui.set_log_summary(log_summary().into());
        ui.set_runtime_summary(runtime::summary().into());
        if ui.get_runtime_open() {
            let rows = self.runtime_rows();
            let views: Vec<DebugInfoRow> = rows
                .into_iter()
                .map(|(label, value)| DebugInfoRow {
                    label: label.into(),
                    value: value.into(),
                })
                .collect();
            if self.runtime_rows.row_count() == views.len() {
                for (index, row) in views.into_iter().enumerate() {
                    if self.runtime_rows.row_data(index).as_ref() != Some(&row) {
                        self.runtime_rows.set_row_data(index, row);
                    }
                }
            } else {
                self.runtime_rows.set_vec(views);
            }
        }
    }

    fn memory_plan(&self) -> MemoryPlan {
        MemoryPlan::resolve(self.manager.get().performance.memory_budget_mb, SystemMemory::read())
    }

    fn runtime_rows(&self) -> Vec<(String, String)> {
        let cache = self
            .context()
            .map(|context| context.thumbnails.cache_handle().stats())
            .unwrap_or_default();
        runtime::rows(&runtime::RuntimeInputs {
            plan: self.memory_plan(),
            cache,
        })
    }

    /// What a run of the checks needs (`None` before the owner set the context).
    fn check_context(&self) -> Option<CheckContext> {
        let context = self.context()?;
        let data_dir = app_dirs::data_dir();
        Some(CheckContext {
            library: Some(context.library.clone()),
            db_dir: data_dir.join("library.lmdb"),
            thumbnail_dir: data_dir.join("thumbnails"),
            data_dir,
            config_dir: app_dirs::config_dir(),
            root: self.manager.get().general.root_folder,
            memory: Some(diagnostics::memory_facts(&self.memory_plan())),
            in_app: true,
        })
    }

    /// Changes the remembered state of the page (open cards, passed lists) and shows it.
    fn change_page_state(&self, change: impl FnOnce(&mut DebugPageState)) {
        let mut settings = self.manager.get();
        change(&mut settings.view.debug_page);
        self.manager.save_quiet_async(&settings);
        if let Some(window) = self.window() {
            show_page_state(&window, &settings.view.debug_page);
            if window.global::<DebugUi>().get_runtime_open() {
                self.refresh_live(&window);
            }
        }
    }
}

/// Shows the last run of the checks with the facts in memory as they are now (also from the closures workers send back).
fn show_checks(window: &SettingsWindow, shared: &Shared, manager: &SettingsManager) {
    let report = shared.report.lock().clone();
    let plan = MemoryPlan::resolve(manager.get().performance.memory_budget_mb, SystemMemory::read());
    let in_memory: Vec<Check> = diagnostics::in_memory_checks(Some(diagnostics::memory_facts(&plan)));
    views::show(window, report.as_deref(), &in_memory);
}

/// Adds `id` to `list` or removes it.
fn toggle(list: &mut Vec<String>, id: &str) {
    if let Some(index) = list.iter().position(|entry| entry == id) {
        list.remove(index);
    } else {
        list.push(id.to_owned());
    }
}

fn show_page_state(window: &SettingsWindow, state: &DebugPageState) {
    let ui = window.global::<DebugUi>();
    let open = |id: &str| state.open_cards.iter().any(|card| card == id);
    let passed = |id: &str| state.passed_shown.iter().any(|card| card == id);
    ui.set_dependencies_open(open("dependencies"));
    ui.set_health_open(open("health"));
    ui.set_log_open(open("log"));
    ui.set_report_open(open("report"));
    ui.set_tools_open(open("tools"));
    ui.set_startup_open(open("startup"));
    ui.set_runtime_open(open("runtime"));
    ui.set_privacy_open(open("privacy"));
    ui.set_dependencies_passed_shown(passed("dependencies"));
    ui.set_health_passed_shown(passed("health"));
    ui.set_developer_open(state.developer_open);
}

fn log_summary() -> String {
    let level =
        logging::env_override().map_or_else(|| logging::level().label().to_owned(), |v| format!("RUST_LOG={v}"));
    let files = if logging::sinks().files_running() {
        "files on"
    } else {
        "files off"
    };
    format!(
        "{level}, {} lines in memory, {files}",
        views::count(logging::ring().len())
    )
}

fn folder_views() -> ModelRc<DebugFolderView> {
    let folders = [
        ("settings", "Settings", app_dirs::config_dir()),
        ("library", "Library and thumbnails", app_dirs::data_dir()),
        ("logs", "Log files", app_dirs::log_dir()),
    ];
    let views: Vec<DebugFolderView> = folders
        .into_iter()
        .map(|(id, label, path)| DebugFolderView {
            id: id.into(),
            label: label.into(),
            path: path.display().to_string().into(),
        })
        .collect();
    ModelRc::new(VecModel::from(views))
}

/// The startup timeline: each stage is a bar from the stage before it to itself.
fn show_startup(window: &SettingsWindow) {
    let stages = startup_clock::stages();
    let total = stages.iter().map(|(_, ms)| *ms).fold(0.0_f64, f64::max).max(1.0);
    let mut previous = 0.0_f64;
    let views: Vec<DebugStageView> = stages
        .iter()
        .map(|(name, ms)| {
            let view = DebugStageView {
                name: name.as_str().into(),
                start: (previous / total).az::<f32>(),
                end: (ms / total).az::<f32>(),
                time: format!("{ms:.0} ms").into(),
                first_frame: name == startup_clock::FIRST_FRAME,
            };
            previous = *ms;
            view
        })
        .collect();
    let summary = stages
        .iter()
        .find(|(name, _)| name == startup_clock::FIRST_FRAME)
        .map(|(_, ms)| format!("first frame after {ms:.0} ms"))
        .unwrap_or_default();
    let ui = window.global::<DebugUi>();
    ui.set_stages(ModelRc::new(VecModel::from(views)));
    ui.set_startup_summary(summary.into());
}
