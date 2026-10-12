//! User-facing texts for thumbnail regeneration (menu label, toolbar tooltip, status bar messages).
//! Pure functions so the singular/plural wording of the context menu and the toolbar can never drift apart.

/// Label of the context menu entry (singular for one file, plural with count otherwise).
pub fn menu_label(selected_count: usize, has_video: bool) -> String {
    match (selected_count == 1, has_video) {
        (true, true) => "Thumbnail (Random Pos.)".to_string(),
        (true, false) => "Thumbnail".to_string(),
        (false, true) => format!("Thumbnails (Random Pos.) ({} Files)", selected_count),
        (false, false) => format!("Thumbnails ({} Files)", selected_count),
    }
}

/// Tooltip of the Browse toolbar button; mirrors the singular/plural wording of the context menu entry.
pub fn tooltip(selected_count: usize, has_video: bool) -> String {
    match (selected_count, has_video) {
        (0, _) => "Regenerate thumbnails: select one or more files first".to_string(),
        (1, true) => "Regenerate thumbnail at a random video position".to_string(),
        (1, false) => "Regenerate thumbnail".to_string(),
        (n, true) => format!("Regenerate thumbnails of {} selected files (videos at random positions)", n),
        (n, false) => format!("Regenerate thumbnails of {} selected files", n),
    }
}

pub fn busy_message() -> &'static str {
    "Cannot regenerate thumbnail: another operation is already in progress."
}

pub fn nothing_selected_message() -> &'static str {
    "Select one or more files to regenerate their thumbnails."
}

pub fn start_message(total: usize) -> String {
    if total == 1 {
        "Regenerating thumbnail...".to_string()
    } else {
        format!("Regenerating thumbnails for {} selected files...", total)
    }
}

/// Facts needed to word the final status bar message.
#[derive(Debug, Clone, Default)]
pub struct RegenerationSummary {
    pub total: usize,
    pub succeeded: usize,
    pub elapsed_ms: u128,
    /// File name and random video position (if any) of the first regenerated file.
    pub first_success: Option<(String, Option<u8>)>,
    /// File name and reason of the first failed file.
    pub first_failure: Option<(String, String)>,
}

impl RegenerationSummary {
    pub fn failed(&self) -> usize {
        self.total.saturating_sub(self.succeeded)
    }
}

/// Final status bar message; the caller picks the level (`failed() == 0` success, `succeeded == 0` error, else warning).
pub fn result_message(s: &RegenerationSummary) -> String {
    let failure_hint = s
        .first_failure
        .as_ref()
        .map(|(name, reason)| format!("'{}': {}", name, reason))
        .unwrap_or_else(|| "unexpected error".to_string());

    if s.failed() == 0 {
        return success_message(s);
    }
    if s.succeeded == 0 && s.total == 1 {
        return format!("Could not regenerate thumbnail for {}. The previous thumbnail was kept.", failure_hint);
    }
    if s.succeeded == 0 {
        return format!(
            "Could not regenerate any of the {} thumbnails (e.g. {}). Previous thumbnails were kept.",
            s.total, failure_hint
        );
    }
    format!(
        "Regenerated {} of {} thumbnails in {}ms; {} failed (e.g. {}). Previous thumbnails were kept.",
        s.succeeded,
        s.total,
        s.elapsed_ms,
        s.failed(),
        failure_hint
    )
}

fn success_message(s: &RegenerationSummary) -> String {
    match (&s.first_success, s.total) {
        (Some((name, Some(pct))), 1) => {
            format!("Regenerated thumbnail at {}% for '{}' in {}ms.", pct, name, s.elapsed_ms)
        }
        (Some((name, None)), 1) => format!("Regenerated thumbnail for '{}' in {}ms.", name, s.elapsed_ms),
        _ => format!("Regenerated {} thumbnails in {}ms.", s.succeeded, s.elapsed_ms),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_label_is_singular_for_one_file() {
        assert_eq!(menu_label(1, true), "Thumbnail (Random Pos.)");
        assert_eq!(menu_label(1, false), "Thumbnail");
    }

    #[test]
    fn menu_label_is_plural_with_count_for_many_files() {
        assert_eq!(menu_label(3, true), "Thumbnails (Random Pos.) (3 Files)");
        assert_eq!(menu_label(3, false), "Thumbnails (3 Files)");
    }

    #[test]
    fn tooltip_follows_selection_size_and_content() {
        assert!(tooltip(0, false).contains("select"));
        assert_eq!(tooltip(1, true), "Regenerate thumbnail at a random video position");
        assert_eq!(tooltip(1, false), "Regenerate thumbnail");
        assert_eq!(tooltip(4, true), "Regenerate thumbnails of 4 selected files (videos at random positions)");
        assert_eq!(tooltip(4, false), "Regenerate thumbnails of 4 selected files");
    }

    #[test]
    fn start_message_is_singular_or_plural() {
        assert_eq!(start_message(1), "Regenerating thumbnail...");
        assert_eq!(start_message(5), "Regenerating thumbnails for 5 selected files...");
    }

    fn summary(total: usize, succeeded: usize) -> RegenerationSummary {
        RegenerationSummary {
            total,
            succeeded,
            elapsed_ms: 12,
            first_success: Some(("a.mp4".into(), Some(42))),
            first_failure: Some(("b.mp4".into(), "File not found on disk".into())),
        }
    }

    #[test]
    fn result_message_reports_full_success() {
        assert_eq!(result_message(&summary(1, 1)), "Regenerated thumbnail at 42% for 'a.mp4' in 12ms.");
        assert_eq!(result_message(&summary(3, 3)), "Regenerated 3 thumbnails in 12ms.");
    }

    #[test]
    fn result_message_reports_partial_and_total_failure() {
        assert!(result_message(&summary(3, 2)).starts_with("Regenerated 2 of 3 thumbnails in 12ms; 1 failed"));
        assert!(result_message(&summary(1, 0)).starts_with("Could not regenerate thumbnail for 'b.mp4'"));
        assert!(result_message(&summary(4, 0)).starts_with("Could not regenerate any of the 4 thumbnails"));
    }
}
