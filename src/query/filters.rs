use crate::library::models::{MediaRecord, TagId};
use crate::library::Library;
use crate::media::MediaType;
use crate::settings::{LogObscurer, DEFAULT_MEDIA_TYPE_FILTER, DEFAULT_TAG_MATCH_MODE, DEFAULT_TAG_SCOPE_MODE};
use memchr::memmem;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use slint::SharedString;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MediaTypeFilter {
    Videos = 0,
    Images = 1,
    #[default]
    All = 2,
}

impl MediaTypeFilter {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Videos,
            1 => Self::Images,
            _ => Self::All,
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_str_name(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "videos" | "video" => Self::Videos,
            "images" | "image" => Self::Images,
            _ => Self::All,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Videos => "Videos",
            Self::Images => "Images",
            Self::All => "Videos & Images",
        }
    }

    #[inline]
    pub fn matches(&self, media_type: MediaType) -> bool {
        match self {
            Self::Videos => media_type == MediaType::Video,
            Self::Images => media_type == MediaType::Image,
            Self::All => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatchMode {
    #[default]
    All,
    Any,
}

impl MatchMode {
    pub fn from_str_name(s: &str) -> Self {
        if s.eq_ignore_ascii_case("any") {
            Self::Any
        } else {
            Self::All
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Any => "ANY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScopeMode {
    Folder = 0,
    FolderAndSubfolders = 1,
    #[default]
    Global = 2,
}

impl ScopeMode {
    pub fn from_index(idx: i32) -> Self {
        match idx {
            0 => Self::Folder,
            1 => Self::FolderAndSubfolders,
            _ => Self::Global,
        }
    }

    pub fn to_index(&self) -> i32 {
        *self as i32
    }

    pub fn from_str_name(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "folder" => Self::Folder,
            "folderandsubfolders" | "subfolders" | "recursive" | "folder & subfolders" => {
                Self::FolderAndSubfolders
            }
            _ => Self::Global,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Folder => "Folder",
            Self::FolderAndSubfolders => "Folder & Subfolders",
            Self::Global => "Global",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueryFilter {
    pub rating: Option<u8>,
    pub search_query: String,
    pub is_regex: bool,
    pub tag_ids: HashSet<TagId>,
    pub match_mode: MatchMode,
    pub scope_mode: ScopeMode,
    pub media_type_filter: MediaTypeFilter,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            rating: None,
            search_query: String::new(),
            is_regex: false,
            tag_ids: HashSet::new(),
            match_mode: MatchMode::from_str_name(DEFAULT_TAG_MATCH_MODE),
            scope_mode: ScopeMode::from_str_name(DEFAULT_TAG_SCOPE_MODE),
            media_type_filter: MediaTypeFilter::from_str_name(DEFAULT_MEDIA_TYPE_FILTER),
        }
    }
}

impl QueryFilter {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.search_query.trim().is_empty()
            && self.rating.map(|r| r == 0).unwrap_or(true)
            && self.tag_ids.is_empty()
            && self.media_type_filter == MediaTypeFilter::All
    }

    #[inline]
    pub fn is_query_filter_active(&self) -> bool {
        self.rating.map(|r| r > 0).unwrap_or(false) || !self.tag_ids.is_empty()
    }
}

/// Fully pre-indexed and pre-formatted media record holding zero-allocation Slint strings,
/// precalculated filesystem paths, directory path string, original folder index, and tag cache.
#[derive(Debug, Clone)]
pub struct IndexedMediaRecord {
    pub index: usize,
    pub record: MediaRecord,
    pub full_path: PathBuf,
    pub folder_path_str: SharedString,
    pub folder_path_lower: String,
    pub filename_lower: String,
    pub filename_slint: SharedString,
    pub modified_date_str: SharedString,
    pub created_date_str: SharedString,
    pub filesize_mb_str: SharedString,
    pub rating_str: SharedString,
    pub tags_count: usize,
    pub tags_count_str: SharedString,
    pub width_str: SharedString,
    pub height_str: SharedString,
    pub ar_str: SharedString,
    pub ar_value: f32,
    pub duration_str: SharedString,
    pub format_str: SharedString,
    pub flags_str: SharedString,
    pub is_corrupted: bool,
    pub is_deleted: bool,
    pub has_tags: bool,
}

pub struct PreparedFilter {
    rating: Option<u8>,
    query_trimmed: String,
    pub query_lower: String,
    regex_matcher: Option<Regex>,
    tag_ids: HashSet<TagId>,
    match_mode: MatchMode,
    media_type_filter: MediaTypeFilter,
}

impl PreparedFilter {
    pub fn from_filter(filter: &QueryFilter) -> Self {
        let trimmed = filter.search_query.trim().to_string();
        let query_lower = trimmed.to_lowercase();

        let regex_matcher = if filter.is_regex && !trimmed.is_empty() {
            match RegexBuilder::new(&trimmed).case_insensitive(true).build() {
                Ok(re) => Some(re),
                Err(e) => {
                    tracing::debug!(
                        query = %LogObscurer::query(&trimmed),
                        error = %e,
                        "[SearchFilter: Regex Warning] Invalid regex syntax, falling back to literal matching"
                    );
                    None
                }
            }
        } else {
            None
        };

        Self {
            rating: filter.rating,
            query_trimmed: trimmed,
            query_lower,
            regex_matcher,
            tag_ids: filter.tag_ids.clone(),
            match_mode: filter.match_mode,
            media_type_filter: filter.media_type_filter,
        }
    }

    #[inline]
    pub fn matches_indexed(
        &self,
        item: &IndexedMediaRecord,
        media_tags: &[TagId],
        simd_finder: Option<&memmem::Finder<'_>>,
    ) -> bool {
        // 1. Instant Media Type Filter Check (Zero allocation, 1 CPU instruction)
        if !self.media_type_filter.matches(item.record.media_type) {
            return false;
        }

        let is_rating_active = match self.rating {
            Some(r) => r > 0,
            None => false,
        };
        let rating_matches = if is_rating_active {
            item.record.rating == self.rating.unwrap()
        } else {
            true
        };

        let has_tags = !self.tag_ids.is_empty();
        let item_tags_set: HashSet<TagId> = if has_tags {
            media_tags.iter().copied().collect()
        } else {
            HashSet::new()
        };

        let tag_matches = if has_tags {
            match self.match_mode {
                MatchMode::All => self.tag_ids.is_subset(&item_tags_set),
                MatchMode::Any => !self.tag_ids.is_disjoint(&item_tags_set),
            }
        } else {
            true
        };

        let query_matches = match self.match_mode {
            MatchMode::All => {
                if is_rating_active && has_tags {
                    rating_matches && tag_matches
                } else if is_rating_active {
                    rating_matches
                } else if has_tags {
                    tag_matches
                } else {
                    true
                }
            }
            MatchMode::Any => {
                if is_rating_active && has_tags {
                    rating_matches || tag_matches
                } else if is_rating_active {
                    rating_matches
                } else if has_tags {
                    tag_matches
                } else {
                    true
                }
            }
        };

        if !query_matches {
            return false;
        }

        if !self.query_trimmed.is_empty() {
            if let Some(ref re) = self.regex_matcher
                && !re.is_match(&item.record.filename)
            {
                return false;
            } else if let Some(finder) = simd_finder
                && finder.find(item.filename_lower.as_bytes()).is_none()
            {
                return false;
            }
        }

        true
    }
}

impl QueryFilter {
    pub fn execute_indexed_refs<'a>(
        &self,
        library: &Library,
        all_media: &'a [IndexedMediaRecord],
        candidate_indices: Option<&[usize]>,
    ) -> Vec<&'a IndexedMediaRecord> {
        let t0 = Instant::now();

        if self.is_empty() {
            let filtered: Vec<&'a IndexedMediaRecord> = all_media.iter().collect();
            let elapsed_us = t0.elapsed().as_micros();
            tracing::debug!(
                mode = "Bypass (Unfiltered Reset)",
                input_candidates = all_media.len(),
                matched = filtered.len(),
                elapsed_us = elapsed_us,
                "[SearchFilter: Zero-Copy Bypass] Instant unfiltered reset pass executed"
            );
            return filtered;
        }

        let prepared = PreparedFilter::from_filter(self);

        let simd_finder = if prepared.regex_matcher.is_none() && !prepared.query_lower.is_empty() {
            Some(memmem::Finder::new(prepared.query_lower.as_bytes()))
        } else {
            None
        };

        let has_tag_filter = !self.tag_ids.is_empty();

        let rtxn_opt = if has_tag_filter {
            library.raw_db().env.read_txn().ok()
        } else {
            None
        };

        let filtered: Vec<&'a IndexedMediaRecord> = match candidate_indices {
            Some(indices) => {
                let input_count = indices.len();
                let res: Vec<&'a IndexedMediaRecord> = indices
                    .iter()
                    .filter_map(|&idx| all_media.get(idx))
                    .filter(|m| {
                        if has_tag_filter && let Some(ref rtxn) = rtxn_opt {
                            let tags = library
                                .raw_db()
                                .tags_by_media
                                .get(rtxn, &m.record.id)
                                .ok()
                                .flatten()
                                .unwrap_or_default();
                            prepared.matches_indexed(m, &tags, simd_finder.as_ref())
                        } else {
                            prepared.matches_indexed(m, &[], simd_finder.as_ref())
                        }
                    })
                    .collect();

                let elapsed_us = t0.elapsed().as_micros();
                tracing::debug!(
                    query = %LogObscurer::query(&self.search_query),
                    media_type = ?self.media_type_filter,
                    is_regex = self.is_regex,
                    mode = "Refinement Subset",
                    input_candidates = input_count,
                    matched = res.len(),
                    elapsed_us = elapsed_us,
                    "[SearchFilter: SIMD memmem] Zero-Copy filter executed"
                );
                res
            }
            None => {
                let input_count = all_media.len();
                let res: Vec<&'a IndexedMediaRecord> = all_media
                    .iter()
                    .filter(|m| {
                        if has_tag_filter && let Some(ref rtxn) = rtxn_opt {
                            let tags = library
                                .raw_db()
                                .tags_by_media
                                .get(rtxn, &m.record.id)
                                .ok()
                                .flatten()
                                .unwrap_or_default();
                            prepared.matches_indexed(m, &tags, simd_finder.as_ref())
                        } else {
                            prepared.matches_indexed(m, &[], simd_finder.as_ref())
                        }
                    })
                    .collect();

                let elapsed_us = t0.elapsed().as_micros();
                tracing::debug!(
                    query = %LogObscurer::query(&self.search_query),
                    media_type = ?self.media_type_filter,
                    is_regex = self.is_regex,
                    mode = "Full Set",
                    input_candidates = input_count,
                    matched = res.len(),
                    elapsed_us = elapsed_us,
                    "[SearchFilter: SIMD memmem] Zero-Copy filter executed"
                );
                res
            }
        };

        filtered
    }
}
