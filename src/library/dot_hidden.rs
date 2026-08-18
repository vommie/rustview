use crate::settings::LogObscurer;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// High-performance cached parser and evaluator for Linux `.hidden` files (KDE / Dolphin / Nautilus standard).
#[derive(Debug, Default, Clone)]
pub struct DotHiddenCache {
    cache: HashMap<PathBuf, Option<HashSet<String>>>,
}

impl DotHiddenCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Checks whether a directory component name is considered hidden.
    /// Evaluates both standard dot-folders (name starting with '.') and entries in parent directory `.hidden` files.
    pub fn is_hidden(&mut self, folder_path: &Path, name: &str, support_dot_hidden: bool) -> bool {
        // 1. Linux standard dot-folder check (zero syscalls)
        if name.starts_with('.') {
            return true;
        }

        // 2. Extended Linux .hidden file check
        if !support_dot_hidden {
            return false;
        }

        if let Some(parent) = folder_path.parent() {
            let hidden_entries = self.cache.entry(parent.to_path_buf()).or_insert_with(|| {
                let dot_hidden_file = parent.join(".hidden");
                if dot_hidden_file.is_file() {
                    let t0 = Instant::now();
                    if let Ok(content) = std::fs::read_to_string(&dot_hidden_file) {
                        let mut set = HashSet::new();
                        for line in content.lines() {
                            let trimmed = line.trim();
                            if !trimmed.is_empty() && !trimmed.starts_with('#') {
                                set.insert(trimmed.to_string());
                            }
                        }
                        tracing::debug!(
                            parent = %LogObscurer::path(parent),
                            entries_count = set.len(),
                            parse_time_us = t0.elapsed().as_micros(),
                            "[Benchmark: DotHidden] Parsed Linux .hidden file entries"
                        );
                        Some(set)
                    } else {
                        None
                    }
                } else {
                    None
                }
            });

            if let Some(set) = hidden_entries {
                return set.contains(name);
            }
        }

        false
    }
}
