//! The ordered, filterable list of formats behind Settings > File Formats (pure, no UI types).

use super::format_table::{FORMAT_SPECS, FormatSpec};
use super::formats::MediaType;

/// One row of the Settings page: a format and whether it opens a new group heading.
#[derive(Debug, Clone, Copy)]
pub struct CatalogEntry {
    pub spec: &'static FormatSpec,
    pub starts_group: bool,
}

/// Formats RustView cannot show, so the Settings page can say so (title, explanation).
pub const UNSUPPORTED_FORMATS: &[(&str, &str)] = &[
    (
        "Camera RAW (.cr2, .cr3, .nef, .arw, .dng, .orf, .rw2, .raf)",
        "Needs a RAW decoder, which RustView does not include.",
    ),
    (
        "Documents and layered sources (.pdf, .ai, .eps, .xcf, .kra)",
        "These are not raster images that RustView can render.",
    ),
    (
        "Audio files (.mp3, .flac, .m4a, .ogg, .wav)",
        "RustView shows images and videos only. An audio-only .mp4, .mkv or .webm is indexed but marked as damaged.",
    ),
];

/// Extensions of media files RustView does not handle but users expect to see (phone photos, camera RAW). A refresh names
/// the ones it skipped, so a missing picture is explained; other unknown extensions (.txt, .pdf, ...) are not mentioned.
pub const KNOWN_UNSUPPORTED_EXTENSIONS: &[&str] = &[
    "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2", "dng", "orf", "rw2", "raf", "pef", "srw", "x3f", "3fr", "erf", "kdc",
    "mrw", "rwl", "iiq", "jxr", "wdp", "bpg", "flif", "icns", "cur", "xcf", "kra", "psb",
];

/// The formats of `kind` matching `filter`, in Settings order: by group, then by name. A group heading is flagged on the
/// first entry of each group that is left after filtering.
pub fn catalog(kind: MediaType, filter: &str) -> Vec<CatalogEntry> {
    let query = filter.trim().trim_start_matches('.').to_lowercase();
    let mut specs: Vec<&'static FormatSpec> = FORMAT_SPECS
        .iter()
        .filter(|s| s.media_type == Some(kind) && matches_query(s, &query))
        .collect();
    specs.sort_by_key(|s| s.ui_sort_key());

    let mut previous_group = None;
    specs
        .into_iter()
        .map(|spec| {
            let starts_group = previous_group != Some(spec.group);
            previous_group = Some(spec.group);
            CatalogEntry { spec, starts_group }
        })
        .collect()
}

/// `query` is lower case and without a leading dot. It matches names, ids, the decoder label and extensions (also by
/// prefix with the dot, `.jp`), so "heic", "mkv", "raw" or ".jp" all find something useful.
pub fn matches_query(spec: &FormatSpec, query: &str) -> bool {
    query.is_empty()
        || spec.id.contains(query)
        || spec.name.to_lowercase().contains(query)
        || spec.long_name.to_lowercase().contains(query)
        || spec.extensions.iter().any(|e| e.contains(query))
}
