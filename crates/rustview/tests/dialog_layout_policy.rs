//! Repository-level dialog layout policy.
//!
//! Slint cannot size a window to wrapped text on its own: a text inside nested layouts is measured as one line, and a window with a
//! fixed height clips whatever grows (an extra line, a longer message). So every dialog (`ui/*_dialog.slint` that is a `Window`)
//! follows one rule instead of per-dialog pixel tuning:
//!
//! - the window has a fixed `width`, or a `preferred-width` plus a `min-width` when the user may make it wider (never a
//!   `max-height` or `height`);
//! - it opens at the height of its content (`preferred-height: dialog-layout.preferred-height;`, where `dialog-layout` is the outer
//!   layout; a `FocusScope` child alone would report the height of one line per wrapped text) and may be made lower than that
//!   (`min-height: dialog-layout.min-height;`);
//! - its content lives in a `DialogBody` (`ui/dialog_body.slint`): at the top at its own height (a taller window leaves empty space
//!   below it, a single-line field never stretches), with a scroll bar when the window is lower, and growing the window when a note
//!   appears (`ui::dialog_fit::install`, called for every dialog window right after it is created);
//! - the outer layout has `width: root.width;` and the content `width: body.width;`, which lets Slint measure wrapped text at the
//!   real width.

use std::fs;
use std::path::PathBuf;

fn dialogs() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui");
    let mut found: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with("_dialog.slint"))
        })
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().to_string(),
                fs::read_to_string(p).unwrap_or_default(),
            )
        })
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
fn dialog_windows_open_at_the_height_of_their_content() {
    let mut problems = Vec::new();
    for (name, text) in dialogs() {
        let props = window_properties(&text);
        let has = |key: &str| props.iter().any(|l| l.trim_start().starts_with(key));
        let fixed = props
            .iter()
            .any(|l| l.trim_start().starts_with("width:") && l.trim_end().ends_with("px;"));
        let resizable = has("preferred-width:") && has("min-width:") && !has("width:");
        if fixed == resizable {
            problems.push(format!(
                "{name}: the window needs either a fixed `width: <n>px;` or `preferred-width:` plus `min-width:`"
            ));
        }
        for forbidden in ["max-height:", "height:", "max-width:"] {
            if has(forbidden) {
                problems.push(format!(
                    "{name}: the window must not set `{forbidden}` (the height follows the content)"
                ));
            }
        }
        if fixed && (has("min-width:") || has("preferred-width:")) {
            problems.push(format!("{name}: a fixed width needs no `min-width`/`preferred-width`"));
        }
        for required in [
            "preferred-height: dialog-layout.preferred-height;",
            "min-height: dialog-layout.min-height;",
        ] {
            if !props.iter().any(|l| l.trim() == required) {
                problems.push(format!("{name}: the window needs `{required}`"));
            }
        }
        if !text.contains("dialog-layout := VerticalLayout") {
            problems.push(format!("{name}: the outer layout must be named `dialog-layout`"));
        }
    }
    assert!(
        problems.is_empty(),
        "dialog windows break the layout rule:\n{}",
        problems.join("\n")
    );
}

#[test]
fn dialog_content_sits_in_a_scrolling_body_at_its_own_height() {
    let mut problems = Vec::new();
    for (name, text) in dialogs() {
        for required in [
            "import { DialogBody } from \"dialog_body.slint\";",
            "body := DialogBody {",
            "content-height: content.preferred-height;",
            "width: body.width;",
            "height: self.preferred-height;",
        ] {
            if !text.contains(required) {
                problems.push(format!("{name}: missing `{required}`"));
            }
        }
        if !text.contains("content := VerticalLayout {") && !text.contains("content := HorizontalLayout {") {
            problems.push(format!(
                "{name}: the content layout of the body must be named `content`"
            ));
        }
        if text.matches("width: root.width;").count() < 1 {
            problems.push(format!("{name}: the outer layout needs `width: root.width;`"));
        }
    }
    assert!(
        problems.is_empty(),
        "dialog content is squeezed or measured wrongly without it:\n{}",
        problems.join("\n")
    );
}

/// The Slint component names of the dialogs (`export component <Name> inherits Window`).
fn dialog_components() -> Vec<String> {
    dialogs()
        .iter()
        .filter_map(|(_, text)| {
            let line = text.lines().find(|l| l.contains("inherits Window"))?;
            line.split_whitespace().nth(2).map(ToString::to_string)
        })
        .collect()
}

fn rust_sources(dir: &std::path::Path, found: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            found.push((
                path.display().to_string(),
                fs::read_to_string(&path).unwrap_or_default(),
            ));
        }
    }
}

#[test]
fn every_dialog_window_follows_its_content() {
    let mut sources = Vec::new();
    rust_sources(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src"), &mut sources);
    let mut problems = Vec::new();
    for component in dialog_components() {
        let creation = format!("{component}::new()");
        let sites: Vec<&(String, String)> = sources.iter().filter(|(_, text)| text.contains(&creation)).collect();
        if sites.is_empty() {
            problems.push(format!("{component}: no place creates it"));
        }
        for (file, text) in sites {
            if !text.contains("dialog_fit::install(") {
                problems.push(format!("{file}: creates {component} without `dialog_fit::install`"));
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
