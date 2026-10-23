use crate::media::MediaHasher;
use crate::util::cast::Az;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static OBFUSCATE_LOGS: AtomicBool = AtomicBool::new(crate::settings::schema::DEFAULT_OBFUSCATE_LOGS);

/// High-performance, thread-safe log sanitization and privacy protection subsystem.
/// Masks file names, folder paths, tag labels, and search queries with database IDs or surrogate identifiers when enabled.
pub struct LogObscurer;

impl LogObscurer {
    /// Returns true if log obfuscation / privacy masking is currently active.
    #[inline]
    pub fn is_active() -> bool {
        OBFUSCATE_LOGS.load(Ordering::Acquire)
    }

    /// Sets whether log obfuscation is globally enabled.
    pub fn set_obfuscate(enabled: bool) {
        OBFUSCATE_LOGS.store(enabled, Ordering::Release);
    }

    /// Obfuscates or formats a filesystem path.
    pub fn path(path: impl AsRef<Path>) -> String {
        let p = path.as_ref();
        if Self::is_active() {
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_ascii_lowercase()))
                .unwrap_or_default();
            let hash = MediaHasher::to_hex(&MediaHasher::hash_bytes(p.to_string_lossy().as_bytes()));
            format!("[Path:{hash:.8}{ext}]")
        } else {
            p.display().to_string()
        }
    }

    /// Obfuscates or formats an optional filesystem path.
    pub fn path_opt(path_opt: Option<impl AsRef<Path>>) -> String {
        match path_opt {
            Some(p) => Self::path(p),
            None => "None".to_string(),
        }
    }

    /// Obfuscates or formats a directory / folder record reference.
    pub fn folder(id: impl Into<Option<u64>>, path: impl AsRef<Path>) -> String {
        let p = path.as_ref();
        let id_opt = id.into();
        if Self::is_active() {
            match id_opt {
                Some(i) => format!("Folder#{i}"),
                None => Self::path(p),
            }
        } else {
            match id_opt {
                Some(i) => format!("Folder#{}({})", i, p.display()),
                None => format!("'{}'", p.display()),
            }
        }
    }

    /// Obfuscates or formats a media file reference.
    pub fn media(id: impl Into<Option<u64>>, filename: &str) -> String {
        let id_opt = id.into();
        if Self::is_active() {
            let ext = Path::new(filename)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_ascii_lowercase()))
                .unwrap_or_default();
            match id_opt {
                Some(i) => format!("Media#{i}[*{ext}]"),
                None => format!("[*{ext}]"),
            }
        } else {
            match id_opt {
                Some(i) => format!("Media#{i}({filename})"),
                None => format!("'{filename}'"),
            }
        }
    }

    /// Obfuscates or formats a raw filename.
    pub fn filename(filename: &str) -> String {
        if Self::is_active() {
            let ext = Path::new(filename)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e.to_ascii_lowercase()))
                .unwrap_or_default();
            let hash = MediaHasher::to_hex(&MediaHasher::hash_bytes(filename.as_bytes()));
            format!("[File:{hash:.8}{ext}]")
        } else {
            format!("'{filename}'")
        }
    }

    /// Obfuscates or formats a tag record reference.
    pub fn tag(id: impl Into<Option<u32>>, name: &str) -> String {
        let id_opt = id.into();
        if Self::is_active() {
            match id_opt {
                Some(i) => format!("Tag#{i}"),
                None => "[Tag:masked]".to_string(),
            }
        } else {
            match id_opt {
                Some(i) => format!("Tag#{i}({name})"),
                None => format!("'{name}'"),
            }
        }
    }

    /// Formats a known tag ID.
    pub fn tag_id(id: u32) -> String {
        format!("Tag#{id}")
    }

    /// Obfuscates or formats a search/filter query string.
    pub fn query(query: &str) -> String {
        if Self::is_active() {
            format!("[Query:{} chars]", query.chars().count())
        } else {
            format!("\"{query}\"")
        }
    }

    /// Unified media target descriptor used across image & video thumbnailing pipelines.
    pub fn target(path: &Path, media_id: Option<u64>) -> String {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map_or_else(|| ".unknown".to_string(), |e| format!(".{}", e.to_ascii_lowercase()));

        let size_hint = std::fs::metadata(path)
            .map(|m| format!(", {:.2} KB", m.len().az::<f64>() / 1024.0))
            .unwrap_or_default();

        if Self::is_active() {
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
