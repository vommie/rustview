//! `rustview check`: the checks of Settings > Debug in the terminal (`docs/debug_page.md`). Reads the settings and the library
//! without changing anything; exits with 1 when a problem was found.

use crate::system::diagnostics::{self, CheckContext};
use crate::system::window_system;
use rustview_core::diagnostics::render;
use rustview_core::library::Library;
use rustview_core::settings::{AppSettings, MemoryPlan, SystemMemory};
use std::path::Path;
use std::sync::Arc;

/// Runs the checks and prints them; returns the exit status.
pub fn run(data_dir: &Path, config_dir: &Path) -> i32 {
    window_system::decide();
    // The settings are read, never repaired or written (a damaged file is reported by the check itself).
    let settings: AppSettings = std::fs::read_to_string(config_dir.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let db_dir = data_dir.join("library.lmdb");
    let library = if db_dir.is_dir() {
        match Library::open(&db_dir) {
            Ok(library) => Some(Arc::new(library)),
            Err(error) => {
                eprintln!("The library cannot be opened: {error}");
                None
            }
        }
    } else {
        None
    };
    let plan = MemoryPlan::resolve(settings.performance.memory_budget_mb, SystemMemory::read());
    let context = CheckContext {
        library,
        thumbnail_dir: data_dir.join("thumbnails"),
        db_dir,
        data_dir: data_dir.to_path_buf(),
        config_dir: config_dir.to_path_buf(),
        root: settings.general.root_folder.clone(),
        memory: Some(diagnostics::memory_facts(&plan)),
        in_app: false,
    };
    let report = diagnostics::run(&context);
    print!("{}", render::render(&report, false));
    i32::from(report.has_problems())
}
