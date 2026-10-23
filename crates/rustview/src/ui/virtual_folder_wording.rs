//! Tooltip wording of the "Recursive folder view" toolbar button.
//! The tooltip always explains the function and, while the button is disabled, adds the reason on a second line.
//! Pure functions, so the text can be tested without a window.

/// Explanation of the function for the current state of the toggle.
pub fn description(is_virtual_mode: bool) -> &'static str {
    if is_virtual_mode {
        "Disable recursive folder view (show current folder only)"
    } else {
        "Recursive folder view: Show media from current folder and all subfolders"
    }
}

/// Full tooltip: the function description plus the reason when the button is disabled.
pub fn tooltip(is_virtual_mode: bool, disabled_reason: Option<&str>) -> String {
    match disabled_reason {
        Some(reason) => format!("{}\nDisabled: {}", description(is_virtual_mode), reason),
        None => description(is_virtual_mode).to_string(),
    }
}
