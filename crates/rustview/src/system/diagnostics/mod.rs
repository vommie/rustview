//! The checks of Settings > Debug and `rustview check` (`docs/debug_page.md`): what only the app knows (the window system, the
//! X server, the GL renderer, how videos are drawn) plus the collectors of `rustview_core::diagnostics`.

pub mod graphics;

use crate::system::window_system::{self, Choice, REQUEST_VARIABLE, WaylandReason};
use rustview_core::diagnostics::dependencies::{self, WindowSystemKind};
use rustview_core::diagnostics::health::{self, AbRepeatFacts, DisplayFacts, HealthFacts, MemoryFacts};
use rustview_core::diagnostics::{Check, CheckReport, collect};
use rustview_core::library::Library;
use rustview_core::settings::MemoryPlan;
use rustview_core::settings::memory_budget::BYTES_PER_MB;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

/// What a run of the checks needs from the app.
pub struct CheckContext {
    /// `None` when there is no library yet (the command line before the first start).
    pub library: Option<Arc<Library>>,
    /// The LMDB folder of the library (`library.lmdb`).
    pub db_dir: PathBuf,
    /// The folder of the thumbnail pack files.
    pub thumbnail_dir: PathBuf,
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub root: Option<PathBuf>,
    pub memory: Option<MemoryFacts>,
    /// The checks run inside the app (graphics and mpv are known) rather than on the command line.
    pub in_app: bool,
}

/// The window system chosen at startup, and why it is native Wayland.
pub fn window_system_facts() -> (WindowSystemKind, Option<String>) {
    match window_system::current() {
        Choice::X11Session => (WindowSystemKind::X11Session, None),
        Choice::XWayland => (WindowSystemKind::XWayland, None),
        Choice::NoDisplay => (WindowSystemKind::NoDisplay, None),
        Choice::Wayland(reason) => {
            let why = match reason {
                WaylandReason::Requested => format!("{REQUEST_VARIABLE}=wayland"),
                WaylandReason::NoXDisplay => "the session has no XWayland".to_owned(),
                WaylandReason::XDisplayUnreachable => "XWayland does not answer".to_owned(),
            };
            (WindowSystemKind::Wayland, Some(why))
        }
    }
}

/// What is known about the display (inside the app: also the GL renderer and how videos are drawn).
pub fn display_facts(in_app: bool) -> DisplayFacts {
    let (window_system, wayland_reason) = window_system_facts();
    let gl = graphics::gl_info();
    DisplayFacts {
        window_system,
        wayland_reason,
        gl_renderer: gl.map(|info| info.renderer.clone()),
        gl_version: gl.map(|info| info.version.clone()),
        video_path: graphics::video_path(),
        in_app,
        ab_repeat: ab_repeat_facts(),
    }
}

/// The mpv script `ab_repeat.lua` as the players found it (memory only; `None` while its support is switched off).
fn ab_repeat_facts() -> Option<AbRepeatFacts> {
    use crate::viewer::ab_repeat::support::{self, Support};
    if !crate::viewer::ab_repeat::prefs::current().enabled {
        return None;
    }
    let found = support::last();
    Some(match &found {
        Support::Unknown => AbRepeatFacts::Unknown,
        Support::Compatible { version, .. } => AbRepeatFacts::Found(version.clone()),
        Support::Incompatible(_) => AbRepeatFacts::Incompatible(found.describe()),
        Support::NotFound => AbRepeatFacts::NotFound,
    })
}

/// The RAM budget as the health check sees it.
pub fn memory_facts(plan: &MemoryPlan) -> MemoryFacts {
    MemoryFacts {
        budget_mb: plan.budget_bytes / BYTES_PER_MB,
        automatic: plan.automatic,
        has_warnings: !plan.warnings.is_empty(),
    }
}

/// The checks the app can show at once, from memory (no drive access).
pub fn in_memory_checks(memory: Option<MemoryFacts>) -> Vec<Check> {
    health::in_memory(&display_facts(true), memory)
}

/// Runs every check. Looks at the system and the database: call on a worker thread, and only when the user asked for it.
pub fn run(context: &CheckContext) -> CheckReport {
    let started = Instant::now();
    let (window_system, _) = window_system_facts();
    let x11 = if window_system.is_x11() {
        crate::viewer::x11_server_facts()
    } else {
        None
    };
    let mut checks = dependencies::evaluate(&collect::dependencies(window_system, x11));
    let (library, audit, thumbnails) = match &context.library {
        Some(lib) => {
            let facts = collect::library(lib, &context.db_dir);
            let media = facts.as_ref().map_or(0, |f| f.media);
            (
                Some(facts),
                Some(collect::audit(lib)),
                Some(collect::thumbnails(lib, &context.thumbnail_dir, media)),
            )
        }
        None => (Some(Err("there is no library yet".to_owned())), None, None),
    };
    let facts = HealthFacts {
        library,
        audit,
        thumbnails,
        settings_file: Some(collect::settings_file(&context.config_dir)),
        space: Some(collect::space(&context.data_dir)),
        root: Some(collect::root(context.root.as_deref())),
        display: display_facts(context.in_app),
        memory: context.memory,
    };
    checks.extend(health::evaluate(&facts));
    let report = CheckReport::new(checks);
    tracing::info!(
        checks = report.checks.len(),
        problems = report
            .checks
            .iter()
            .filter(|c| c.status == rustview_core::diagnostics::Status::Problem)
            .count(),
        warnings = report
            .checks
            .iter()
            .filter(|c| c.status == rustview_core::diagnostics::Status::Warning)
            .count(),
        elapsed_ms = started.elapsed().as_millis(),
        "[SystemDiagnostics: Checks] Checks finished"
    );
    report
}
