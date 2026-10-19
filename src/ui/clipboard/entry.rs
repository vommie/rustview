//! The "Paste" menu entry: its text, whether it is enabled and why not (memory only, decided when a menu opens).

use super::summary::ClipboardSummary;
use crate::ctx_menu::{CtxMenuActionId, CtxMenuBuilder};
use crate::icons::Icon;
use crate::system::file_clipboard::ClipboardMode;

/// What stands between the clipboard and a folder, as far as the menu can tell without touching the drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteTarget {
    Ready,
    /// A scan, refresh, transfer or paste is running.
    Busy,
    /// The folder is flagged missing on disk.
    MissingFolder,
    /// The view the entry belongs to is no folder (a search result, the recursive view).
    NotAFolder(&'static str),
    /// The session has no file clipboard.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasteEntry {
    pub label: String,
    pub enabled: bool,
    pub tooltip: String,
}

impl PasteEntry {
    pub fn label_for(summary: &ClipboardSummary) -> String {
        if !summary.known {
            return "Paste".to_string();
        }
        match summary.media {
            0 => "Paste".to_string(),
            1 => "Paste One File".to_string(),
            n => format!("Paste {n} Files"),
        }
    }

    pub fn new(summary: &ClipboardSummary, target: &PasteTarget) -> Self {
        let label = Self::label_for(summary);
        let description = match summary.mode {
            ClipboardMode::Cut => "Move the files from the clipboard into this folder",
            ClipboardMode::Copy => "Copy the files from the clipboard into this folder",
        };
        let reason = if *target == PasteTarget::Unsupported {
            Some("Disabled: the file clipboard needs an X11 session".to_string())
        } else if summary.known && summary.total == 0 {
            Some("Disabled: the clipboard holds no files".to_string())
        } else if summary.known && summary.media == 0 {
            Some("Disabled: the clipboard holds no images or videos".to_string())
        } else {
            match target {
                PasteTarget::Ready | PasteTarget::Unsupported => None,
                PasteTarget::Busy => Some("Disabled: another scan, refresh or transfer is currently in progress".to_string()),
                PasteTarget::MissingFolder => Some("Disabled: the folder does not exist on physical disk".to_string()),
                PasteTarget::NotAFolder(why) => Some(format!("Disabled: {why}")),
            }
        };
        match reason {
            Some(reason) => Self { label, enabled: false, tooltip: format!("{description}\n{reason}") },
            None => Self { label, enabled: true, tooltip: description.to_string() },
        }
    }

    /// Appends the entry to a menu. `shortcut` is only shown where Ctrl+V pastes into the very folder the menu is about.
    pub fn append(self, builder: CtxMenuBuilder, id: CtxMenuActionId, shortcut: Option<&str>) -> CtxMenuBuilder {
        builder.action_with_tooltip(id, self.label, shortcut, Some(Icon::Paste), self.enabled, self.tooltip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(total: usize, media: usize, mode: ClipboardMode) -> ClipboardSummary {
        ClipboardSummary { known: true, total, media, mode }
    }

    #[test]
    fn the_label_follows_the_number_of_media_files() {
        assert_eq!(PasteEntry::label_for(&summary(1, 1, ClipboardMode::Copy)), "Paste One File");
        assert_eq!(PasteEntry::label_for(&summary(5, 3, ClipboardMode::Copy)), "Paste 3 Files");
        assert_eq!(PasteEntry::label_for(&summary(2, 0, ClipboardMode::Copy)), "Paste");
        assert_eq!(PasteEntry::label_for(&ClipboardSummary::default()), "Paste");
    }

    #[test]
    fn the_entry_is_enabled_only_for_media_in_a_usable_place() {
        assert!(PasteEntry::new(&summary(2, 2, ClipboardMode::Copy), &PasteTarget::Ready).enabled);
        assert!(PasteEntry::new(&ClipboardSummary::default(), &PasteTarget::Ready).enabled, "unknown: the paste decides");
        let empty = PasteEntry::new(&summary(0, 0, ClipboardMode::Copy), &PasteTarget::Ready);
        assert!(!empty.enabled && empty.tooltip.contains("holds no files"));
        let text = PasteEntry::new(&summary(3, 0, ClipboardMode::Copy), &PasteTarget::Ready);
        assert!(!text.enabled && text.tooltip.contains("no images or videos"));
        let busy = PasteEntry::new(&summary(1, 1, ClipboardMode::Cut), &PasteTarget::Busy);
        assert!(!busy.enabled && busy.tooltip.contains("in progress") && busy.tooltip.starts_with("Move"));
        assert!(!PasteEntry::new(&summary(1, 1, ClipboardMode::Copy), &PasteTarget::MissingFolder).enabled);
        let query = PasteEntry::new(&summary(1, 1, ClipboardMode::Copy), &PasteTarget::NotAFolder("the view shows search results"));
        assert!(!query.enabled && query.tooltip.contains("search results"));
        assert!(!PasteEntry::new(&summary(1, 1, ClipboardMode::Copy), &PasteTarget::Unsupported).enabled);
    }
}
