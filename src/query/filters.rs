use crate::library::Library;
use crate::library::models::{MediaRecord, TagId};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeMode {
    Folder,
    Global,
}

#[derive(Debug, Clone)]
pub struct QueryFilter {
    pub rating: Option<u8>,
    pub search_query: String,
    pub is_regex: bool,
    pub tag_ids: HashSet<TagId>,
    pub match_mode: MatchMode,
    pub scope_mode: ScopeMode,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            rating: None,
            search_query: String::new(),
            is_regex: false,
            tag_ids: HashSet::new(),
            match_mode: MatchMode::All,
            scope_mode: ScopeMode::Folder,
        }
    }
}

impl QueryFilter {
    pub fn matches(&self, media: &MediaRecord, media_tags: &[TagId]) -> bool {
        // Rating match
        if let Some(r) = self.rating
            && media.rating != r
        {
            return false;
        }

        // Search text / Regex match on Filename
        if !self.search_query.is_empty() {
            if self.is_regex {
                if let Ok(re) = regex::Regex::new(&self.search_query)
                    && !re.is_match(&media.filename)
                {
                    return false;
                }
            } else if !media
                .filename
                .to_lowercase()
                .contains(&self.search_query.to_lowercase())
            {
                return false;
            }
        }

        // Tag matching
        if !self.tag_ids.is_empty() {
            let item_tags_set: HashSet<TagId> = media_tags.iter().copied().collect();
            match self.match_mode {
                MatchMode::All => {
                    if !self.tag_ids.is_subset(&item_tags_set) {
                        return false;
                    }
                }
                MatchMode::Any => {
                    if self.tag_ids.is_disjoint(&item_tags_set) {
                        return false;
                    }
                }
            }
        }

        true
    }

    pub fn execute(&self, library: &Library, folder_media: &[MediaRecord]) -> Vec<MediaRecord> {
        folder_media
            .iter()
            .filter(|m| {
                let tags = library
                    .tags()
                    .get_tags_for_media(m.id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| t.id)
                    .collect::<Vec<_>>();
                self.matches(m, &tags)
            })
            .cloned()
            .collect()
    }
}
