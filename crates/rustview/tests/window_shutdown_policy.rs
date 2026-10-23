//! Repository-level shutdown policy: every Slint window of the application is closed by the orderly shutdown.
//!
//! `src/ui/shutdown/windows.rs` lists each window component and how the shutdown closes it. A window that a `.slint` file
//! declares but that list does not mention would stay open when the main window closes (and keep the process alive).

use std::fs;
use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Names of the components that `inherits Window` in `text`.
fn window_components(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim_start().strip_prefix("export ").unwrap_or(line.trim_start());
            let rest = line.strip_prefix("component ")?;
            let (name, inherits) = rest.split_once(" inherits ")?;
            let base = inherits.split_whitespace().next()?;
            (base == "Window").then(|| name.trim().to_owned())
        })
        .collect()
}

/// Names of the windows in the `WINDOWS` table of `manifest`: every string literal that opens a tuple, whatever way rustfmt wraps it.
fn listed_windows(manifest: &str) -> Vec<&str> {
    let parts: Vec<&str> = manifest.split('"').collect();
    (1..parts.len().saturating_sub(1))
        .step_by(2)
        .filter(|&i| parts[i - 1].trim_end().ends_with('(') && parts[i + 1].trim_start().starts_with(','))
        .map(|i| parts[i])
        .collect()
}

fn slint_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "slint"))
        .collect();
    files.sort();
    files
}

#[test]
fn every_slint_window_is_handled_by_the_shutdown() {
    let manifest = fs::read_to_string(project_root().join("src/ui/shutdown/windows.rs"))
        .expect("src/ui/shutdown/windows.rs must exist");

    let mut windows = Vec::new();
    for file in slint_files(&project_root().join("ui")) {
        let text = fs::read_to_string(&file).unwrap_or_default();
        windows.extend(window_components(&text));
    }
    assert!(
        windows.len() >= 20,
        "found only {} windows; the scan is broken: {windows:?}",
        windows.len()
    );

    let listed = listed_windows(&manifest);
    let missing: Vec<&String> = windows.iter().filter(|name| !listed.contains(&name.as_str())).collect();
    assert!(
        missing.is_empty(),
        "windows missing in src/ui/shutdown/windows.rs (add them there and close them in `close_windows`): {missing:?}"
    );
}

#[test]
fn the_manifest_lists_no_window_that_does_not_exist() {
    let manifest = fs::read_to_string(project_root().join("src/ui/shutdown/windows.rs")).unwrap_or_default();
    let mut windows = Vec::new();
    for file in slint_files(&project_root().join("ui")) {
        windows.extend(window_components(&fs::read_to_string(&file).unwrap_or_default()));
    }
    let listed = listed_windows(&manifest);
    let stale: Vec<&&str> = listed
        .iter()
        .filter(|name| !windows.iter().any(|w| w == **name))
        .collect();
    assert!(
        stale.is_empty(),
        "windows listed in the manifest that no .slint file declares: {stale:?}"
    );
}
