//! Repository-level dialog layout policy.
//!
//! Slint cannot size a window to wrapped text on its own: a text inside nested layouts is measured as one line, and a window with a
//! fixed height clips whatever grows (an extra line, a longer message). So every dialog (`ui/*_dialog.slint` that is a `Window`)
//! follows one rule instead of per-dialog pixel tuning:
//!
//! - the window has a fixed `width` (no `min-width`, `preferred-width`, `max-height` or `height`);
//! - its height follows the content, also when the content grows later: `preferred-height` and `min-height` are bound to
//!   `dialog-layout.preferred-height`, where `dialog-layout` is the outer layout of the dialog (a FocusScope child alone would
//!   report the height of one line per wrapped text);
//! - the layout containers that hold the texts have `width: root.width;` (outer and content container), which lets Slint measure
//!   wrapped text at the real width.

use std::fs;
use std::path::PathBuf;

fn dialogs() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui");
    let mut found: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with("_dialog.slint")))
        .map(|p| (p.file_name().unwrap().to_string_lossy().to_string(), fs::read_to_string(p).unwrap_or_default()))
        .filter(|(_, text)| text.contains("inherits Window"))
        .collect();
    found.sort();
    found
}

/// The property lines of the window itself: lines indented by exactly four spaces after the `inherits Window` line.
fn window_properties(text: &str) -> Vec<&str> {
    let start = text.find("inherits Window").unwrap();
    text[start..]
        .lines()
        .skip(1)
        .filter(|line| line.starts_with("    ") && !line.starts_with("     "))
        .collect()
}

#[test]
fn there_are_dialogs_to_check() {
    assert!(dialogs().len() >= 15, "the dialog files were not found");
}

#[test]
fn dialog_windows_have_a_fixed_width_and_a_content_driven_height() {
    let mut problems = Vec::new();
    for (name, text) in dialogs() {
        let props = window_properties(&text);
        let has = |key: &str| props.iter().any(|l| l.trim_start().starts_with(key));
        if !props.iter().any(|l| l.trim_start().starts_with("width:") && l.trim_end().ends_with("px;")) {
            problems.push(format!("{name}: the window needs a fixed `width: <n>px;`"));
        }
        for forbidden in ["max-height:", "height:", "min-width:", "preferred-width:"] {
            if has(forbidden) {
                problems.push(format!("{name}: the window must not set `{forbidden}` (the height follows the content, the width is fixed)"));
            }
        }
        for required in ["preferred-height: dialog-layout.preferred-height;", "min-height: dialog-layout.preferred-height;"] {
            if !props.iter().any(|l| l.trim() == required) {
                problems.push(format!("{name}: the window needs `{required}`"));
            }
        }
        if !text.contains("dialog-layout := VerticalLayout") {
            problems.push(format!("{name}: the outer layout must be named `dialog-layout`"));
        }
    }
    assert!(problems.is_empty(), "dialog windows break the layout rule:\n{}", problems.join("\n"));
}

#[test]
fn dialog_containers_get_the_window_width() {
    let mut problems = Vec::new();
    for (name, text) in dialogs() {
        let count = text.matches("width: root.width;").count();
        if count < 2 {
            problems.push(format!("{name}: the outer and the content layout need `width: root.width;` (found {count})"));
        }
    }
    assert!(problems.is_empty(), "wrapped text is measured wrongly without it:\n{}", problems.join("\n"));
}
