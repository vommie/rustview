//! A value of a log line that prints itself hidden or shown, depending on the obscurer's mode at the moment it is formatted.

use super::LogObscurer;
use crate::media::MediaHasher;
use std::fmt;
use std::path::{Path, PathBuf};

/// A name or path in a log line, made by the helpers of [`LogObscurer`]. `Display` (and `Debug`, quoted like a string) render
/// it with the mode of the formatting thread.
#[derive(Clone, PartialEq, Eq)]
pub struct Obscured(Kind);

#[derive(Clone, PartialEq, Eq)]
enum Kind {
    Path(PathBuf),
    OptPath(Option<PathBuf>),
    Folder(Option<u64>, PathBuf),
    Media(Option<u64>, String),
    Filename(String),
    Tag(Option<u32>, String),
    Query(String),
    ForeignText(String, Option<PathBuf>),
    Target {
        path: PathBuf,
        media_id: Option<u64>,
        size_hint: String,
    },
}

/// `.ext` in lower case, or an empty string.
fn lower_extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{}", e.to_ascii_lowercase()))
        .unwrap_or_default()
}

fn hidden_path(path: &Path) -> String {
    let hash = MediaHasher::to_hex(&MediaHasher::hash_bytes(path.to_string_lossy().as_bytes()));
    format!("[Path:{hash:.8}{}]", lower_extension(path))
}

fn hidden_filename(filename: &str) -> String {
    let hash = MediaHasher::to_hex(&MediaHasher::hash_bytes(filename.as_bytes()));
    format!("[File:{hash:.8}{}]", lower_extension(Path::new(filename)))
}

impl Obscured {
    pub(super) fn path(path: PathBuf) -> Self {
        Self(Kind::Path(path))
    }

    pub(super) fn path_opt(path: Option<PathBuf>) -> Self {
        Self(Kind::OptPath(path))
    }

    pub(super) fn folder(id: Option<u64>, path: PathBuf) -> Self {
        Self(Kind::Folder(id, path))
    }

    pub(super) fn media(id: Option<u64>, filename: String) -> Self {
        Self(Kind::Media(id, filename))
    }

    pub(super) fn filename(filename: String) -> Self {
        Self(Kind::Filename(filename))
    }

    pub(super) fn tag(id: Option<u32>, name: String) -> Self {
        Self(Kind::Tag(id, name))
    }

    pub(super) fn query(query: String) -> Self {
        Self(Kind::Query(query))
    }

    pub(super) fn foreign_text(text: String, known_path: Option<PathBuf>) -> Self {
        Self(Kind::ForeignText(text, known_path))
    }

    pub(super) fn target(path: PathBuf, media_id: Option<u64>, size_hint: String) -> Self {
        Self(Kind::Target {
            path,
            media_id,
            size_hint,
        })
    }

    /// The text in the given mode (`hide` = obfuscated).
    pub fn render(&self, hide: bool) -> String {
        match &self.0 {
            Kind::Path(path) => {
                if hide {
                    hidden_path(path)
                } else {
                    path.display().to_string()
                }
            }
            Kind::OptPath(path) => path
                .as_ref()
                .map_or_else(|| "None".to_owned(), |p| Self::path(p.clone()).render(hide)),
            Kind::Folder(id, path) => match (hide, id) {
                (true, Some(i)) => format!("Folder#{i}"),
                (true, None) => hidden_path(path),
                (false, Some(i)) => format!("Folder#{}({})", i, path.display()),
                (false, None) => format!("'{}'", path.display()),
            },
            Kind::Media(id, filename) => match (hide, id) {
                (true, Some(i)) => format!("Media#{i}[*{}]", lower_extension(Path::new(filename))),
                (true, None) => format!("[*{}]", lower_extension(Path::new(filename))),
                (false, Some(i)) => format!("Media#{i}({filename})"),
                (false, None) => format!("'{filename}'"),
            },
            Kind::Filename(filename) => {
                if hide {
                    hidden_filename(filename)
                } else {
                    format!("'{filename}'")
                }
            }
            Kind::Tag(id, name) => match (hide, id) {
                (true, Some(i)) => format!("Tag#{i}"),
                (true, None) => "[Tag:masked]".to_owned(),
                (false, Some(i)) => format!("Tag#{i}({name})"),
                (false, None) => format!("'{name}'"),
            },
            Kind::Query(query) => {
                if hide {
                    format!("[Query:{} chars]", query.chars().count())
                } else {
                    format!("\"{query}\"")
                }
            }
            Kind::ForeignText(text, known_path) => match known_path {
                Some(path) if hide => hide_known_path(text, path),
                _ => text.clone(),
            },
            Kind::Target {
                path,
                media_id,
                size_hint,
            } => {
                if hide {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map_or_else(|| ".unknown".to_owned(), |e| format!(".{}", e.to_ascii_lowercase()));
                    match media_id {
                        Some(id) => format!("MediaId({id}) [*{ext}{size_hint}]"),
                        None => format!("Anonymous [*{ext}{size_hint}]"),
                    }
                } else {
                    match media_id {
                        Some(id) => format!("\"{}\" (MediaId({}){})", path.display(), id, size_hint),
                        None => format!("\"{}\" ({})", path.display(), size_hint.trim_start_matches(", ")),
                    }
                }
            }
        }
    }
}

/// Replaces the path, then its folder and its file name, by their hidden forms.
fn hide_known_path(text: &str, path: &Path) -> String {
    let mut out = text.replace(&*path.to_string_lossy(), &hidden_path(path));
    if let Some(parent) = path.parent().filter(|p| p.as_os_str().len() > 1) {
        out = out.replace(&*parent.to_string_lossy(), &hidden_path(parent));
    }
    if let Some(name) = path.file_name().map(|n| n.to_string_lossy()) {
        out = out.replace(&*name, &hidden_filename(&name));
    }
    out
}

impl fmt::Display for Obscured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.render(LogObscurer::is_active()))
    }
}

impl fmt::Debug for Obscured {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.render(LogObscurer::is_active()), f)
    }
}
