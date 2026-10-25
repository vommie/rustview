//! The diagnostic report (`docs/debug_page.md`): version, system, window system, graphics, the checks, the settings that differ
//! from the defaults, the startup, the runtime values and the last log lines. Names and paths are hidden while "Obfuscate" is on.

use rustview_core::diagnostics::{CheckReport, render};
use rustview_core::logging;
use rustview_core::settings::{AppSettings, LogObscurer};
use rustview_core::util::startup_clock;
use serde_json::Value;
use std::fmt::Write as _;

/// How many log lines the report ends with.
pub const LOG_LINES: usize = 300;
/// Parts of the settings that are window and layout state, not choices worth reporting.
const SKIPPED_SETTINGS: [&str; 3] = ["window", "view", "hotkeys"];
/// Remembered state inside the other parts (which tree nodes are open, the tag history).
const SKIPPED_STATE: [&str; 3] = [
    "folders.expanded_folder_ids",
    "tags.expanded_tag_ids",
    "tags.history_tag_ids",
];
/// A list longer than this is reported by its length only.
const MAX_LIST_ITEMS: usize = 8;

/// The settings that differ from the defaults, as `section.key = value` lines (absolute paths hidden when `private`).
pub fn changed_settings(current: &AppSettings, private: bool) -> Vec<String> {
    let (Ok(defaults), Ok(current)) = (
        serde_json::to_value(AppSettings::default()),
        serde_json::to_value(current),
    ) else {
        return Vec::new();
    };
    let mut lines = Vec::new();
    diff(&defaults, &current, "", private, &mut lines);
    let hotkeys = current
        .get("hotkeys")
        .and_then(Value::as_object)
        .map_or(0, serde_json::Map::len);
    if hotkeys > 0 {
        lines.push(format!("hotkeys = {hotkeys} actions with their own keys"));
    }
    lines
}

fn diff(default: &Value, current: &Value, prefix: &str, private: bool, lines: &mut Vec<String>) {
    match (default, current) {
        (Value::Object(defaults), Value::Object(values)) => {
            for (key, value) in values {
                if prefix.is_empty() && SKIPPED_SETTINGS.contains(&key.as_str()) {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if SKIPPED_STATE.contains(&path.as_str()) {
                    continue;
                }
                diff(defaults.get(key).unwrap_or(&Value::Null), value, &path, private, lines);
            }
        }
        _ if default == current => {}
        _ => lines.push(format!("{prefix} = {}", value_text(current, private))),
    }
}

fn value_text(value: &Value, private: bool) -> String {
    match value {
        Value::String(text) if private && text.starts_with('/') => LogObscurer::path(text).render(true),
        Value::Array(items) if items.len() > MAX_LIST_ITEMS => format!("[{} items]", items.len()),
        Value::Array(items) => {
            let shown: Vec<String> = items.iter().map(|item| value_text(item, private)).collect();
            format!("[{}]", shown.join(", "))
        }
        other => other.to_string(),
    }
}

/// The stages of this launch, one per line.
pub fn startup_lines() -> Vec<String> {
    startup_clock::stages()
        .into_iter()
        .map(|(stage, ms)| format!("{stage:<20} {ms:>8.1} ms"))
        .collect()
}

/// Everything the report is made of that is not read here.
pub struct ReportInputs<'a> {
    pub checks: &'a CheckReport,
    pub settings: &'a AppSettings,
    pub runtime: &'a [(String, String)],
}

fn section(out: &mut String, title: &str) {
    let _ = write!(out, "\n== {title} ==\n");
}

/// The whole report. `private` hides names and paths (the setting "Obfuscate").
pub fn build(inputs: &ReportInputs<'_>, private: bool) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "RustView {} diagnostic report, {}",
        env!("CARGO_PKG_VERSION"),
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %z")
    );
    let _ = writeln!(out, "Names and paths hidden: {}", if private { "yes" } else { "no" });
    section(&mut out, "System");
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_owned())
            })
        })
        .unwrap_or_else(|| "unknown Linux".to_owned());
    let kernel = rustix::system::uname().release().to_string_lossy().into_owned();
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".to_owned());
    let _ = writeln!(out, "{os}, kernel {kernel}, desktop {desktop}");
    for (label, value) in inputs.runtime {
        let _ = writeln!(out, "{label}: {value}");
    }
    section(&mut out, "Checks");
    out.push_str(&render::render(inputs.checks, private));
    section(&mut out, "Settings that differ from the defaults");
    let changed = changed_settings(inputs.settings, private);
    if changed.is_empty() {
        out.push_str("none\n");
    }
    for line in changed {
        let _ = writeln!(out, "{line}");
    }
    section(&mut out, "Startup");
    for line in startup_lines() {
        let _ = writeln!(out, "{line}");
    }
    section(&mut out, &format!("Last {LOG_LINES} log lines"));
    for line in logging::ring().last(LOG_LINES) {
        out.push_str(&line.file_line(private));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::changed_settings;
    use rustview_core::settings::AppSettings;

    #[test]
    fn only_changed_settings_are_listed_and_paths_can_be_hidden() {
        let mut settings = AppSettings::default();
        assert_eq!(changed_settings(&settings, false), Vec::<String>::new());
        settings.general.root_folder = Some("/media/Secret".into());
        settings.debug.log_level = "debug".into();
        settings.view.sidebar_left_width = 999.0;
        let shown = changed_settings(&settings, false);
        assert!(
            shown.contains(&"general.root_folder = \"/media/Secret\"".to_owned()),
            "{shown:?}"
        );
        assert!(shown.contains(&"debug.log_level = \"debug\"".to_owned()), "{shown:?}");
        assert!(
            shown.iter().all(|line| !line.starts_with("view.")),
            "layout state is left out"
        );
        let hidden = changed_settings(&settings, true);
        assert!(
            hidden
                .iter()
                .any(|line| line.starts_with("general.root_folder = [Path:")),
            "{hidden:?}"
        );
        assert!(hidden.iter().all(|line| !line.contains("Secret")));
    }
}
