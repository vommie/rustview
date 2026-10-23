//! The question before something closes that holds pictures with unsaved edits (pure): a list with one checkbox per picture
//! (checked = save it, unchecked = discard the edits) and the buttons Apply / Cancel. Pictures whose format cannot be written
//! back are saved through the export dialog; the close waits until every checked picture is written and is called off when
//! one of them fails or the user cancels its dialog.

use crate::viewer::multiview::follow::TileKey;
use rustview_core::library::models::MediaId;

/// One picture with unsaved edits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsavedEntry {
    pub tile: TileKey,
    pub media_id: MediaId,
    pub filename: String,
    pub folder: String,
    /// "Crop to 893 x 1116 px".
    pub description: String,
    /// The file can be overwritten in its own format (otherwise the export dialog asks where to save).
    pub overwritable: bool,
    /// Checked in the dialog: save it.
    pub save: bool,
}

/// What is about to close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Fullscreen,
    /// Closing this many tabs.
    Tabs(usize),
    /// Removing a tile from its view.
    Tile,
    /// The selection the fullscreen follows changes (or it pages to another picture).
    SelectionChange,
    /// A tab of this view is merged into another one.
    Merge,
    /// The file is refreshed from disk.
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsavedPrompt {
    pub title: String,
    pub message: String,
    pub entries: Vec<UnsavedEntry>,
}

/// The dialog for `entries` (all checked at first).
pub fn prompt(reason: CloseReason, entries: Vec<UnsavedEntry>) -> UnsavedPrompt {
    let count = entries.len();
    let pictures = if count == 1 {
        "1 picture has".to_string()
    } else {
        format!("{count} pictures have")
    };
    let what = match reason {
        CloseReason::Fullscreen => "Closing the fullscreen".to_string(),
        CloseReason::Tabs(1) => "Closing the tab".to_string(),
        CloseReason::Tabs(n) => format!("Closing {n} tabs"),
        CloseReason::Tile => "Removing the picture from the view".to_string(),
        CloseReason::SelectionChange => "Showing other pictures".to_string(),
        CloseReason::Merge => "Merging the tabs".to_string(),
        CloseReason::Refresh => "Reading the file again".to_string(),
    };
    UnsavedPrompt {
        title: "Unsaved Changes".to_string(),
        message: format!(
            "{pictures} unsaved changes. {what} ends the editing: checked pictures are saved, the changes of the others are \
             discarded."
        ),
        entries: entries.into_iter().map(|e| UnsavedEntry { save: true, ..e }).collect(),
    }
}

/// What the user decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsavedDecision {
    /// Saved over the original.
    pub overwrite: Vec<TileKey>,
    /// Saved through the export dialog, one after the other.
    pub export: Vec<TileKey>,
    pub discard: Vec<TileKey>,
}

impl UnsavedDecision {
    /// Nothing has to be written: the close can go on at once.
    pub fn is_immediate(&self) -> bool {
        self.overwrite.is_empty() && self.export.is_empty()
    }
}

/// The decision for the checkboxes of `entries`.
pub fn decide(entries: &[UnsavedEntry]) -> UnsavedDecision {
    let mut decision = UnsavedDecision {
        overwrite: Vec::new(),
        export: Vec::new(),
        discard: Vec::new(),
    };
    for entry in entries {
        match (entry.save, entry.overwritable) {
            (true, true) => decision.overwrite.push(entry.tile),
            (true, false) => decision.export.push(entry.tile),
            (false, _) => decision.discard.push(entry.tile),
        }
    }
    decision
}

/// The text of the Apply button for the current checkboxes.
pub fn apply_label(entries: &[UnsavedEntry]) -> &'static str {
    let saving = entries.iter().filter(|e| e.save).count();
    if saving == 0 {
        "Discard Changes"
    } else if saving == entries.len() {
        if entries.len() == 1 { "Save" } else { "Save All" }
    } else {
        "Save Checked"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(tile: TileKey, overwritable: bool) -> UnsavedEntry {
        UnsavedEntry {
            tile,
            media_id: tile,
            filename: format!("{tile}.jpg"),
            folder: "/pictures".to_string(),
            description: "Crop to 10 x 10 px".to_string(),
            overwritable,
            save: false,
        }
    }

    #[test]
    fn every_picture_is_checked_at_first_and_the_text_counts_them() {
        let one = prompt(CloseReason::Fullscreen, vec![entry(1, true)]);
        assert!(one.entries[0].save);
        assert!(one.message.starts_with("1 picture has"));
        assert!(one.message.contains("Closing the fullscreen"));
        let two = prompt(CloseReason::Tabs(3), vec![entry(1, true), entry(2, false)]);
        assert!(two.message.starts_with("2 pictures have"));
        assert!(two.message.contains("Closing 3 tabs"));
        assert_eq!(apply_label(&two.entries), "Save All");
        assert_eq!(apply_label(&one.entries), "Save");
    }

    #[test]
    fn the_checkboxes_decide_what_is_saved_exported_or_discarded() {
        let mut entries = prompt(CloseReason::Tile, vec![entry(1, true), entry(2, false), entry(3, true)]).entries;
        entries[2].save = false;
        assert_eq!(apply_label(&entries), "Save Checked");
        let decision = decide(&entries);
        assert_eq!(decision.overwrite, [1]);
        assert_eq!(decision.export, [2]);
        assert_eq!(decision.discard, [3]);
        assert!(!decision.is_immediate());
        for e in &mut entries {
            e.save = false;
        }
        assert_eq!(apply_label(&entries), "Discard Changes");
        assert!(decide(&entries).is_immediate());
    }
}
