//! Repository-level icon policy checks.
//!
//! - Icons only come from the icon library (`assets/icons/icons.toml`): no raw `@image-url` in hand-written Slint.
//! - The one exception is `ui/app_identity.slint`: the window icon is the colored app icon, not a Tabler icon.
//! - No emoji or Unicode symbols are used as icon substitutes in UI strings; real SVG icons are used instead.
//! - Every Tabler file below `assets/icons/tabler` is referenced by the manifest and nothing is missing.

use std::fs;
use std::path::{Path, PathBuf};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn files_with_extension(dir: &Path, extension: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = fs::read_dir(&current) else { continue };
        for path in entries.flatten().map(|e| e.path()) {
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == extension) {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Returns `(line number, line)` of all non-comment lines that contain a non-ASCII character.
/// Scanning stops at the first `#[cfg(test)]` so test fixtures may use arbitrary text.
fn non_ascii_code_lines(path: &Path) -> Vec<(usize, String)> {
    let text = fs::read_to_string(path).unwrap_or_default();
    let mut hits = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("#[cfg(test)]") {
            break;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        if !line.is_ascii() {
            hits.push((index + 1, line.trim().to_owned()));
        }
    }
    hits
}

#[test]
fn slint_icons_only_come_from_the_registry() {
    let ui_dir = project_root().join("ui");
    for path in files_with_extension(&ui_dir, "slint") {
        if path.file_name().is_some_and(|name| name == "app_identity.slint") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("readable slint file");
        for (index, line) in text.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            assert!(
                !code.contains("@image-url"),
                "{}:{} uses @image-url; reference icons through IconId/AppIcon instead",
                path.display(),
                index + 1
            );
        }
    }
}

#[test]
fn slint_ui_has_no_emoji_or_unicode_symbols() {
    let ui_dir = project_root().join("ui");
    let mut offenders = Vec::new();
    for path in files_with_extension(&ui_dir, "slint") {
        for (line, text) in non_ascii_code_lines(&path) {
            offenders.push(format!("{}:{line}: {text}", path.display()));
        }
    }
    assert!(
        offenders.is_empty(),
        "non-ASCII symbols in Slint UI (use icons):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn rust_ui_modules_have_no_emoji_or_unicode_symbols() {
    let root = project_root().join("src");
    let mut paths = files_with_extension(&root.join("ui"), "rs");
    paths.extend(files_with_extension(&root.join("ctx_menu"), "rs"));
    paths.extend(files_with_extension(&root.join("icons"), "rs"));
    paths.push(root.join("system/dnd.rs"));

    let mut offenders = Vec::new();
    for path in paths {
        for (line, text) in non_ascii_code_lines(&path) {
            offenders.push(format!("{}:{line}: {text}", path.display()));
        }
    }
    assert!(
        offenders.is_empty(),
        "non-ASCII symbols in UI strings (use icons):\n{}",
        offenders.join("\n")
    );
}

#[test]
fn tabler_files_match_the_manifest() {
    let manifest = fs::read_to_string(project_root().join("assets/icons/icons.toml")).expect("manifest");
    let table: toml_like::Manifest = toml_like::parse(&manifest);

    let tabler_root = project_root().join("assets/icons/tabler");
    let mut on_disk: Vec<String> = ["outline", "filled"]
        .iter()
        .flat_map(|style| {
            files_with_extension(&tabler_root.join(style), "svg")
                .into_iter()
                .map(move |p| format!("{style}/{}", p.file_name().expect("file name").to_string_lossy()))
        })
        .collect();
    on_disk.sort();

    let mut wanted = table.tabler_files;
    wanted.sort();
    wanted.dedup();
    assert_eq!(
        on_disk, wanted,
        "run scripts/sync-tabler-icons.sh to sync assets/icons/tabler with icons.toml"
    );
    assert!(
        tabler_root.join("LICENSE").is_file(),
        "the Tabler MIT license must ship with the icons"
    );
}

/// Minimal line-based reader for the one-line `[icons]` entries (avoids a TOML dev-dependency).
mod toml_like {
    pub struct Manifest {
        pub tabler_files: Vec<String>,
    }

    fn quoted_value(line: &str, key: &str) -> Option<String> {
        let start = line.find(&format!("{key} = \""))? + key.len() + 4;
        let end = line[start..].find('"')? + start;
        Some(line[start..end].to_owned())
    }

    pub fn parse(text: &str) -> Manifest {
        let mut in_icons = false;
        let mut tabler_files = Vec::new();
        for line in text.lines() {
            if line.starts_with('[') {
                in_icons = line.trim() == "[icons]";
                continue;
            }
            if !in_icons {
                continue;
            }
            if let Some(name) = quoted_value(line, "tabler") {
                let style = quoted_value(line, "style").unwrap_or_else(|| "outline".to_owned());
                tabler_files.push(format!("{style}/{name}.svg"));
            }
        }
        Manifest { tabler_files }
    }
}
