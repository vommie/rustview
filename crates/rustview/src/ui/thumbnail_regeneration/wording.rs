//! User-facing texts for thumbnail regeneration (menu label, toolbar tooltip, status bar messages).
//! Pure functions so the singular/plural wording of the context menu and the toolbar can never drift apart.

use super::eligibility::RegenerationEligibility;

/// Label of the context menu entry. Only valid videos are counted: images cannot get a random-position thumbnail.
pub fn menu_label(videos: usize) -> String {
    if videos <= 1 {
        "Thumbnail (Random Pos.)".to_string()
    } else {
        format!("Thumbnails (Random Pos.) ({} Videos)", videos)
    }
}

/// Explanation shown as tooltip of the toolbar button and of the context menu entry (one text for both, so they
/// can never drift apart). It always says that the thumbnail is regenerated at a random position, whether the
/// control is enabled or not, and why it is disabled otherwise.
pub fn hint(e: &RegenerationEligibility) -> String {
    if e.selected == 0 {
        return "Thumbnail (Random Pos.): select at least one video first".to_string();
    }
    if !e.is_actionable() {
        return "Thumbnail (Random Pos.): the selection contains no available video. Images have no duration; \
                use Refresh > File (Full) to regenerate their thumbnails"
            .to_string();
    }
    let base = if e.videos == 1 {
        "Regenerate the thumbnail at a new random video position".to_string()
    } else {
        format!(
            "Regenerate the thumbnails of {} videos at new random video positions",
            e.videos
        )
    };
    match e.skipped() {
        0 => base,
        1 => format!("{} (1 other selected file is skipped)", base),
        n => format!("{} ({} other selected files are skipped)", base, n),
    }
}

/// Tooltip of a menu entry that is disabled because another operation holds the busy lock.
pub fn busy_hint() -> &'static str {
    "Disabled: Another scan, refresh or thumbnail operation is currently in progress"
}

pub fn busy_message() -> &'static str {
    "Cannot regenerate thumbnails (Random Pos.): another operation is already in progress."
}

pub fn nothing_selected_message() -> &'static str {
    "Select at least one available video to regenerate its thumbnail at a random position."
}

pub fn start_message(total: usize) -> String {
    if total == 1 {
        "Regenerating thumbnail at a random video position...".to_string()
    } else {
        format!(
            "Regenerating thumbnails at random video positions for {} videos...",
            total
        )
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
        return format!(
            "Could not regenerate thumbnail for {}. The previous thumbnail was kept.",
            failure_hint
        );
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
            format!(
                "Regenerated thumbnail at {}% for '{}' in {}ms.",
                pct, name, s.elapsed_ms
            )
        }
        (Some((name, None)), 1) => format!("Regenerated thumbnail for '{}' in {}ms.", name, s.elapsed_ms),
        _ => format!(
            "Regenerated {} thumbnails at random positions in {}ms.",
            s.succeeded, s.elapsed_ms
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(selected: usize, videos: usize) -> RegenerationEligibility {
        RegenerationEligibility { selected, videos }
    }

    #[test]
    fn menu_label_always_says_random_pos_and_counts_only_videos() {
        assert_eq!(menu_label(0), "Thumbnail (Random Pos.)");
        assert_eq!(menu_label(1), "Thumbnail (Random Pos.)");
        assert_eq!(menu_label(3), "Thumbnails (Random Pos.) (3 Videos)");
    }

    #[test]
    fn hint_explains_why_the_control_is_disabled() {
        assert!(hint(&e(0, 0)).contains("select at least one video"));
        let no_video = hint(&e(4, 0));
        assert!(no_video.contains("no available video") && no_video.contains("Refresh > File (Full)"));
    }

    #[test]
    fn hint_names_the_random_position_for_single_and_multiple_videos() {
        assert_eq!(
            hint(&e(1, 1)),
            "Regenerate the thumbnail at a new random video position"
        );
        assert_eq!(
            hint(&e(4, 4)),
            "Regenerate the thumbnails of 4 videos at new random video positions"
        );
    }

    #[test]
    fn hint_mentions_skipped_files_of_mixed_selections() {
        assert!(hint(&e(2, 1)).ends_with("(1 other selected file is skipped)"));
        assert!(
            hint(&e(5, 2)).contains("2 videos") && hint(&e(5, 2)).ends_with("(3 other selected files are skipped)")
        );
    }

    #[test]
    fn start_message_is_singular_or_plural() {
        assert_eq!(start_message(1), "Regenerating thumbnail at a random video position...");
        assert_eq!(
            start_message(5),
            "Regenerating thumbnails at random video positions for 5 videos..."
        );
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
        assert_eq!(
            result_message(&summary(1, 1)),
            "Regenerated thumbnail at 42% for 'a.mp4' in 12ms."
        );
        assert_eq!(
            result_message(&summary(3, 3)),
            "Regenerated 3 thumbnails at random positions in 12ms."
        );
    }

    #[test]
    fn result_message_reports_partial_and_total_failure() {
        assert!(result_message(&summary(3, 2)).starts_with("Regenerated 2 of 3 thumbnails in 12ms; 1 failed"));
        assert!(result_message(&summary(1, 0)).starts_with("Could not regenerate thumbnail for 'b.mp4'"));
        assert!(result_message(&summary(4, 0)).starts_with("Could not regenerate any of the 4 thumbnails"));
    }
}
