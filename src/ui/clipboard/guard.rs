//! Where Ctrl+V may paste: only into the folder the Browse view shows, and only when that is a real, usable folder.

use crate::library::models::FolderId;

/// What the Browse view shows, from memory.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ViewFacts {
    pub folder_id: Option<FolderId>,
    pub folder_name: String,
    /// The list is the result of a search, rating or tag query.
    pub is_query: bool,
    /// The list is a folder with all its subfolders.
    pub is_recursive: bool,
    /// The folder is flagged missing on disk.
    pub folder_missing: bool,
}

/// Why nothing is pasted into the view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteBlock {
    NoFolder,
    Query,
    Recursive,
    MissingFolder(String),
}

impl PasteBlock {
    /// A short reason for a tooltip.
    pub fn short(&self) -> &'static str {
        match self {
            Self::NoFolder => "no folder is open",
            Self::Query => "the view shows search results, not a folder",
            Self::Recursive => "the view shows a folder with all its subfolders, not one folder",
            Self::MissingFolder(_) => "the open folder does not exist on physical disk",
        }
    }

    /// `(heading, message, footnote)` of the dialog that explains it.
    pub fn explanation(&self) -> (&'static str, String, &'static str) {
        match self {
            Self::NoFolder => (
                "No Folder Is Open",
                "Files are pasted into the folder the Browse view shows, and no folder is open.".to_string(),
                "Select a folder in the folder tree, then paste again.",
            ),
            Self::Query => (
                "Cannot Paste Into a Search Result",
                "The Browse view shows the result of a search (text, rating or tags), not a folder. A pasted file has to go into one specific folder."
                    .to_string(),
                "Clear the search or select a folder in the folder tree, then paste again.",
            ),
            Self::Recursive => (
                "Cannot Paste Into the Recursive View",
                "The Browse view shows a folder together with all its subfolders. A pasted file has to go into one specific folder, and this view does not say which."
                    .to_string(),
                "Switch the recursive view off or select the target folder in the folder tree, then paste again. A folder's context menu pastes into that folder.",
            ),
            Self::MissingFolder(name) => (
                "The Folder Does Not Exist",
                format!("The folder '{name}' is flagged as missing: it does not exist on the physical disk any more, so nothing can be pasted into it."),
                "Select an existing folder, or refresh the folder tree if the folder is back.",
            ),
        }
    }
}

/// The folder Ctrl+V pastes into, or why there is none.
pub fn visible_folder(view: &ViewFacts) -> Result<FolderId, PasteBlock> {
    let Some(folder_id) = view.folder_id else {
        return Err(PasteBlock::NoFolder);
    };
    if view.is_query {
        return Err(PasteBlock::Query);
    }
    if view.is_recursive {
        return Err(PasteBlock::Recursive);
    }
    if view.folder_missing {
        return Err(PasteBlock::MissingFolder(view.folder_name.clone()));
    }
    Ok(folder_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder() -> ViewFacts {
        ViewFacts { folder_id: Some(7), folder_name: "Holiday".into(), ..ViewFacts::default() }
    }

    #[test]
    fn a_plain_folder_is_a_target() {
        assert_eq!(visible_folder(&folder()), Ok(7));
    }

    #[test]
    fn every_other_view_says_why_not() {
        assert_eq!(visible_folder(&ViewFacts::default()), Err(PasteBlock::NoFolder));
        assert_eq!(visible_folder(&ViewFacts { is_query: true, ..folder() }), Err(PasteBlock::Query));
        assert_eq!(visible_folder(&ViewFacts { is_recursive: true, ..folder() }), Err(PasteBlock::Recursive));
        assert_eq!(visible_folder(&ViewFacts { folder_missing: true, ..folder() }), Err(PasteBlock::MissingFolder("Holiday".into())));
        // A query wins over a recursive view: it is what the user typed last.
        assert_eq!(visible_folder(&ViewFacts { is_query: true, is_recursive: true, ..folder() }), Err(PasteBlock::Query));
    }

    #[test]
    fn the_explanations_are_complete_sentences() {
        for block in [PasteBlock::NoFolder, PasteBlock::Query, PasteBlock::Recursive, PasteBlock::MissingFolder("A".into())] {
            let (heading, message, footnote) = block.explanation();
            assert!(!heading.is_empty() && message.ends_with('.') && footnote.ends_with('.'), "{block:?}");
            assert!(!block.short().is_empty());
        }
    }
}
