//! Repository-level tooltip policy.
//!
//! A `TooltipHost` (`ui/tooltips.slint`) shows its bubble only when its owner reports the pointer with `on-hover-enter` and
//! `on-hover-leave`; nothing in Slint enforces that, and a host nobody calls is a tooltip that silently never shows (the A-B
//! buttons and the bar menu's buttons had such tooltips from 0.5.4 to 0.5.7). So every `TooltipHost` in a Slint file has an id,
//! and the same file calls both functions of that id.

use std::fs;
use std::path::PathBuf;

const MARKER: &str = ":= TooltipHost";

/// The Slint files of the UI except `tooltips.slint` (which defines the host), with their text.
fn slint_files() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ui");
    let mut files: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "slint"))
        .filter(|p| p.file_name().is_some_and(|n| n != "tooltips.slint"))
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().to_string(),
                fs::read_to_string(&p).unwrap_or_default(),
            )
        })
        .collect();
    files.sort();
    files
}

/// The ids of the `TooltipHost`s declared in `text` (`id := TooltipHost {`).
fn host_ids(text: &str) -> Vec<String> {
    text.match_indices(MARKER)
        .filter_map(|(at, _)| {
            let before = text[..at].trim_end();
            let id: String = before
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            (!id.is_empty()).then_some(id)
        })
        .collect()
}

#[test]
fn every_tooltip_host_is_told_about_the_pointer() {
    let mut problems = Vec::new();
    let mut hosts = 0;
    for (file, text) in slint_files() {
        let unnamed = text.matches("TooltipHost {").count() - text.matches(&format!("{MARKER} {{")).count();
        if unnamed > 0 {
            problems.push(format!(
                "{file}: {unnamed} TooltipHost without an id (nobody can call it)"
            ));
        }
        for id in host_ids(&text) {
            hosts += 1;
            for function in ["on-hover-enter(", "on-hover-leave("] {
                if !text.contains(&format!("{id}.{function}")) {
                    problems.push(format!("{file}: `{id}` is never called with `{function}`"));
                }
            }
        }
    }
    assert!(
        hosts > 0,
        "no TooltipHost found: the policy test looks in the wrong place"
    );
    assert!(problems.is_empty(), "silent tooltips:\n{}", problems.join("\n"));
}

#[test]
fn host_ids_are_read_from_their_declaration() {
    let text = "a := Rectangle {}\n    tip-host := TooltipHost { text: \"x\"; }\n    save_tip := TooltipHost {}";
    assert_eq!(host_ids(text), vec!["tip-host".to_owned(), "save_tip".to_owned()]);
}
