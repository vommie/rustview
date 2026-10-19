//! The words of a paste: the status line and the notice that lists what was not pasted. Pure and unit tested.

use crate::library::paste::{PasteSkip, SkipReason};
use crate::system::file_clipboard::ClipboardMode;

/// How many file names a notice lists per reason before it says "and N more".
const NAMES_PER_REASON: usize = 5;
/// Failures kept for the notice (the count is exact, the list is not).
pub const MAX_LISTED_FAILURES: usize = 20;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PasteStats {
    pub done: usize,
    pub renamed: usize,
    pub overwritten: usize,
    pub skipped: usize,
    pub missing: usize,
    pub failed: usize,
    pub cancelled: usize,
}

pub fn files(count: usize) -> String {
    if count == 1 {
        "1 file".to_string()
    } else {
        format!("{count} files")
    }
}

/// `Copied 3 files into 'Holiday' in 120 ms (1 auto-renamed, 2 failed: Permission denied)`.
pub fn status_line(mode: ClipboardMode, folder: &str, stats: &PasteStats, first_failure: Option<&str>, elapsed_ms: u128) -> String {
    let verb = match mode {
        ClipboardMode::Cut => "Moved",
        ClipboardMode::Copy => "Copied",
    };
    let mut details = Vec::new();
    if stats.renamed > 0 {
        details.push(format!("{} auto-renamed", stats.renamed));
    }
    if stats.overwritten > 0 {
        details.push(format!("{} overwritten", stats.overwritten));
    }
    if stats.skipped > 0 {
        details.push(format!("{} skipped", stats.skipped));
    }
    if stats.missing > 0 {
        details.push(format!("{} no longer exist", stats.missing));
    }
    if stats.failed > 0 {
        let reason = first_failure.map(|r| format!(": {r}")).unwrap_or_default();
        details.push(format!("{} failed{reason}", stats.failed));
    }
    if stats.cancelled > 0 {
        details.push(format!("{} cancelled", stats.cancelled));
    }
    let tail = if details.is_empty() { String::new() } else { format!(" ({})", details.join(", ")) };
    if stats.done == 0 {
        return format!("Nothing was {} into '{folder}'{tail}", if mode == ClipboardMode::Cut { "moved" } else { "copied" });
    }
    format!("{verb} {} into '{folder}' in {elapsed_ms} ms{tail}", files(stats.done))
}

fn reason_title(reason: &SkipReason) -> &'static str {
    match reason {
        SkipReason::SourceMissing => "No longer exist",
        SkipReason::Unreadable(_) => "Cannot be read",
        SkipReason::IsFolder => "Folders (not supported)",
        SkipReason::NotAFile => "Not regular files",
        SkipReason::Unsupported => "Not images or videos",
        SkipReason::InvalidName => "Names that cannot be used",
        SkipReason::AlreadyInFolder => "In this folder already",
        SkipReason::Duplicate => "Listed twice",
    }
}

/// Whether a skipped file is worth telling the user about (a cut file that lies in the target folder already is not).
pub fn is_notable(reason: &SkipReason) -> bool {
    !matches!(reason, SkipReason::AlreadyInFolder | SkipReason::Duplicate)
}

fn names_line(title: &str, names: &[String]) -> String {
    let shown = names.iter().take(NAMES_PER_REASON).cloned().collect::<Vec<_>>().join(", ");
    match names.len().saturating_sub(NAMES_PER_REASON) {
        0 => format!("{title}: {shown}"),
        more => format!("{title}: {shown} and {more} more"),
    }
}

/// The lines of the notice: files that were left out (grouped by reason), files that vanished during the paste and failures.
pub fn notice_details(skipped: &[PasteSkip], vanished: &[String], failures: &[(String, String)]) -> String {
    let mut lines = Vec::new();
    let reasons = [
        SkipReason::SourceMissing,
        SkipReason::IsFolder,
        SkipReason::Unsupported,
        SkipReason::NotAFile,
        SkipReason::InvalidName,
    ];
    let mut missing: Vec<String> = skipped
        .iter()
        .filter(|s| s.reason == SkipReason::SourceMissing)
        .map(|s| s.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
        .collect();
    missing.extend(vanished.iter().cloned());
    if !missing.is_empty() {
        lines.push(names_line(reason_title(&SkipReason::SourceMissing), &missing));
    }
    for reason in reasons.iter().filter(|r| **r != SkipReason::SourceMissing) {
        let names: Vec<String> = skipped
            .iter()
            .filter(|s| &s.reason == reason)
            .map(|s| s.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())
            .collect();
        if !names.is_empty() {
            lines.push(names_line(reason_title(reason), &names));
        }
    }
    let unreadable: Vec<String> = skipped
        .iter()
        .filter_map(|s| match &s.reason {
            SkipReason::Unreadable(why) => Some(format!("{} ({why})", s.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default())),
            _ => None,
        })
        .collect();
    if !unreadable.is_empty() {
        lines.push(names_line(reason_title(&SkipReason::Unreadable(String::new())), &unreadable));
    }
    if !failures.is_empty() {
        let failed: Vec<String> = failures.iter().map(|(name, why)| format!("{name} ({why})")).collect();
        lines.push(names_line("Failed", &failed));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn skip(name: &str, reason: SkipReason) -> PasteSkip {
        PasteSkip { path: PathBuf::from(format!("/x/{name}")), reason }
    }

    #[test]
    fn the_status_line_says_what_happened() {
        let stats = PasteStats { done: 3, renamed: 1, ..PasteStats::default() };
        assert_eq!(status_line(ClipboardMode::Copy, "Holiday", &stats, None, 12), "Copied 3 files into 'Holiday' in 12 ms (1 auto-renamed)");
        let one = PasteStats { done: 1, ..PasteStats::default() };
        assert_eq!(status_line(ClipboardMode::Cut, "A", &one, None, 5), "Moved 1 file into 'A' in 5 ms");
        let failed = PasteStats { done: 2, failed: 2, missing: 1, ..PasteStats::default() };
        assert_eq!(
            status_line(ClipboardMode::Cut, "A", &failed, Some("Permission denied"), 9),
            "Moved 2 files into 'A' in 9 ms (1 no longer exist, 2 failed: Permission denied)"
        );
        let none = PasteStats { skipped: 2, ..PasteStats::default() };
        assert_eq!(status_line(ClipboardMode::Copy, "A", &none, None, 1), "Nothing was copied into 'A' (2 skipped)");
    }

    #[test]
    fn the_notice_groups_by_reason_and_shortens_long_lists() {
        let skipped = vec![
            skip("gone.jpg", SkipReason::SourceMissing),
            skip("notes.txt", SkipReason::Unsupported),
            skip("sub", SkipReason::IsFolder),
            skip("here.jpg", SkipReason::AlreadyInFolder),
        ];
        let text = notice_details(&skipped, &["vanished.png".to_string()], &[("bad.png".to_string(), "Permission denied".to_string())]);
        assert_eq!(
            text,
            "No longer exist: gone.jpg, vanished.png\nFolders (not supported): sub\nNot images or videos: notes.txt\nFailed: bad.png (Permission denied)"
        );
        let many: Vec<PasteSkip> = (0..8).map(|i| skip(&format!("f{i}.txt"), SkipReason::Unsupported)).collect();
        assert_eq!(notice_details(&many, &[], &[]), "Not images or videos: f0.txt, f1.txt, f2.txt, f3.txt, f4.txt and 3 more");
        assert!(notice_details(&[skip("here.jpg", SkipReason::AlreadyInFolder)], &[], &[]).is_empty());
    }

    #[test]
    fn only_surprising_skips_are_notable() {
        assert!(is_notable(&SkipReason::SourceMissing));
        assert!(!is_notable(&SkipReason::AlreadyInFolder));
        assert!(!is_notable(&SkipReason::Duplicate));
    }
}
