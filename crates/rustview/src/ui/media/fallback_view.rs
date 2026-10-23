use crate::icons::Icon;
use rustview_core::query::filters::{MediaTypeFilter, QueryFilter};
use rustview_core::query::ScopeMode;

/// Context-sensitive fallback overlay state data model.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FallbackViewState {
    pub is_active: bool,
    pub title: String,
    pub description: String,
    /// Icon shown above the title; `None` renders no icon.
    pub icon: Option<Icon>,
}

/// Parameters passed to the Fallback View decision engine.
#[derive(Debug, Clone)]
pub struct FallbackEvaluationParams<'a> {
    pub total_in_dataset: usize,
    pub matched_count: usize,
    pub is_query_active: bool,
    pub filter: &'a QueryFilter,
    pub has_root: bool,
    pub root_exists: bool,
    pub current_folder_name: Option<&'a str>,
    /// Some format is switched off in Settings > File Formats: an empty folder may only look empty.
    pub formats_switched_off: bool,
}

/// Encapsulated, pure evaluator determining empty state messages and visibility.
pub struct FallbackViewEvaluator;

impl FallbackViewEvaluator {
    /// Evaluates the current state and returns a context-sensitive FallbackViewState.
    pub fn evaluate(params: FallbackEvaluationParams<'_>) -> FallbackViewState {
        // Fast Path: If media items are visible, empty state is inactive
        if params.matched_count > 0 {
            return FallbackViewState {
                is_active: false,
                title: String::new(),
                description: String::new(),
                icon: None,
            };
        }

        // State 1: No Root Directory Configured
        if !params.has_root {
            return FallbackViewState {
                is_active: true,
                title: "No Root Directory".to_string(),
                description: "Configure a media root folder in Settings to browse your library.".to_string(),
                icon: Some(Icon::Settings),
            };
        }

        // State 2: Configured Root Folder Does Not Exist on Physical Disk
        if !params.root_exists {
            return FallbackViewState {
                is_active: true,
                title: "Directory Not Found".to_string(),
                description: "The configured root folder does not exist on physical storage.".to_string(),
                icon: Some(Icon::Folder),
            };
        }

        let media_type = params.filter.media_type_filter;
        let search_query = params.filter.search_query.trim();
        let has_search = !search_query.is_empty();

        // State 3: Active Tag / Rating Combinatorial Query
        if params.is_query_active {
            let scope_desc = match params.filter.scope_mode {
                ScopeMode::Folder => "in this folder",
                ScopeMode::FolderAndSubfolders => "in this folder and subfolders",
                ScopeMode::Global => "in the library",
            };

            let (title, description) = match media_type {
                MediaTypeFilter::Videos => (
                    "No Matching Videos".to_string(),
                    format!("No video files {} match the active tag and rating criteria.", scope_desc),
                ),
                MediaTypeFilter::Images => (
                    "No Matching Images".to_string(),
                    format!("No image files {} match the active tag and rating criteria.", scope_desc),
                ),
                MediaTypeFilter::All => (
                    "No Query Results".to_string(),
                    format!("No media files {} match the active tag and rating criteria.", scope_desc),
                ),
            };

            return FallbackViewState {
                is_active: true,
                title,
                description,
                icon: Some(Icon::Tag),
            };
        }

        // State 4: Text Search / Regex Filter Active
        if has_search {
            if params.total_in_dataset == 0 {
                return FallbackViewState {
                    is_active: true,
                    title: "Empty Folder".to_string(),
                    description: Self::no_media_text(None, params.formats_switched_off),
                    icon: Some(Icon::Folder),
                };
            }

            let (title, description) = match media_type {
                MediaTypeFilter::Videos => (
                    "No Matching Videos".to_string(),
                    format!("No video files match the filter \"{}\".", search_query),
                ),
                MediaTypeFilter::Images => (
                    "No Matching Images".to_string(),
                    format!("No image files match the filter \"{}\".", search_query),
                ),
                MediaTypeFilter::All => (
                    "No Search Results".to_string(),
                    format!("No files match the filter \"{}\".", search_query),
                ),
            };

            return FallbackViewState {
                is_active: true,
                title,
                description,
                icon: Some(if params.filter.is_regex { Icon::Regex } else { Icon::Search }),
            };
        }

        // State 5: Media Type Filter Mismatch (Folder contains files, but none of the chosen type)
        if params.total_in_dataset > 0 && media_type != MediaTypeFilter::All {
            let (title, description, icon) = match media_type {
                MediaTypeFilter::Videos => (
                    "No Videos in Folder".to_string(),
                    "This folder contains image files, but no videos.".to_string(),
                    Some(Icon::MediaVideo),
                ),
                MediaTypeFilter::Images => (
                    "No Images in Folder".to_string(),
                    "This folder contains video files, but no images.".to_string(),
                    Some(Icon::MediaImage),
                ),
                MediaTypeFilter::All => (
                    "Empty Folder".to_string(),
                    "This folder contains no supported media files.".to_string(),
                    Some(Icon::Folder),
                ),
            };

            return FallbackViewState {
                is_active: true,
                title,
                description,
                icon,
            };
        }

        // State 6: Empty Folder (Default Fallback)
        let folder_name = params.current_folder_name.unwrap_or("This folder");
        FallbackViewState {
            is_active: true,
            title: "Empty Folder".to_string(),
            description: Self::no_media_text(Some(folder_name), params.formats_switched_off),
            icon: Some(Icon::Folder),
        }
    }

    fn no_media_text(folder_name: Option<&str>, formats_switched_off: bool) -> String {
        let subject = match folder_name {
            Some(name) => format!("'{name}'"),
            None => "This folder".to_string(),
        };
        if formats_switched_off {
            format!("{subject} contains no media files of the formats that are switched on (Settings > File Formats).")
        } else {
            format!("{subject} contains no supported media files.")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fallback_evaluator_inactive_when_items_present() {
        let filter = QueryFilter::default();
        let state = FallbackViewEvaluator::evaluate(FallbackEvaluationParams {
            total_in_dataset: 10,
            matched_count: 5,
            is_query_active: false,
            filter: &filter,
            has_root: true,
            root_exists: true,
            current_folder_name: Some("Photos"),
            formats_switched_off: false,
        });

        assert!(!state.is_active);
    }

    #[test]
    fn test_fallback_evaluator_no_root_configured() {
        let filter = QueryFilter::default();
        let state = FallbackViewEvaluator::evaluate(FallbackEvaluationParams {
            total_in_dataset: 0,
            matched_count: 0,
            is_query_active: false,
            filter: &filter,
            has_root: false,
            root_exists: false,
            current_folder_name: None,
            formats_switched_off: false,
        });

        assert!(state.is_active);
        assert_eq!(state.title, "No Root Directory");
        assert_eq!(state.icon, Some(Icon::Settings));
    }

    #[test]
    fn test_fallback_evaluator_search_query_mismatch() {
        let filter = QueryFilter {
            search_query: "sunset".to_string(),
            media_type_filter: MediaTypeFilter::Videos,
            ..Default::default()
        };

        let state = FallbackViewEvaluator::evaluate(FallbackEvaluationParams {
            total_in_dataset: 20,
            matched_count: 0,
            is_query_active: false,
            filter: &filter,
            has_root: true,
            root_exists: true,
            current_folder_name: Some("Vacation"),
            formats_switched_off: false,
        });

        assert!(state.is_active);
        assert_eq!(state.title, "No Matching Videos");
        assert!(state.description.contains("sunset"));
        assert_eq!(state.icon, Some(Icon::Search));
    }

    #[test]
    fn test_fallback_evaluator_media_type_mismatch() {
        let filter = QueryFilter {
            media_type_filter: MediaTypeFilter::Videos,
            ..Default::default()
        };

        let state = FallbackViewEvaluator::evaluate(FallbackEvaluationParams {
            total_in_dataset: 15,
            matched_count: 0,
            is_query_active: false,
            filter: &filter,
            has_root: true,
            root_exists: true,
            current_folder_name: Some("Pictures"),
            formats_switched_off: false,
        });

        assert!(state.is_active);
        assert_eq!(state.title, "No Videos in Folder");
        assert_eq!(state.icon, Some(Icon::MediaVideo));
    }
}
