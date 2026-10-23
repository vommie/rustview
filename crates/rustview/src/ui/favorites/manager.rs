use rustview_core::library::folders::normalize_path;
use rustview_core::settings::LogObscurer;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Individual favorite record storing a virtual display alias and its filesystem path.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FavoriteEntry {
    pub alias: String,
    pub path: PathBuf,
}

/// High-performance, persistent manager for user favorites with atomic disk synchronization.
pub struct FavoriteManager {
    config_path: PathBuf,
    entries: Vec<FavoriteEntry>,
}

impl FavoriteManager {
    pub fn new(config_dir: &Path) -> Self {
        let config_path = config_dir.join("favorites.json");
        let mut manager = Self {
            config_path,
            entries: Vec::new(),
        };
        manager.load();
        manager
    }

    /// Loads favorites from disk with fault tolerance and schema validation.
    pub fn load(&mut self) {
        if !self.config_path.exists() {
            tracing::debug!(
                config_path = %self.config_path.display(),
                "[FavoriteManager: Load] No favorites configuration file found; starting with empty list"
            );
            self.entries.clear();
            return;
        }

        let t0 = Instant::now();
        match fs::read_to_string(&self.config_path) {
            Ok(data) => match serde_json::from_str::<Vec<FavoriteEntry>>(&data) {
                Ok(items) => {
                    self.entries = items;
                    tracing::info!(
                        loaded_count = self.entries.len(),
                        elapsed_us = t0.elapsed().as_micros(),
                        "[FavoriteManager: Load] Favorites successfully loaded from disk"
                    );
                }
                Err(e) => {
                    tracing::error!(
                        error = %e,
                        "[FavoriteManager: Load] Failed to parse favorites JSON. Backing up corrupted file"
                    );
                    let backup = self.config_path.with_extension("corrupted.bak");
                    let _ = fs::copy(&self.config_path, backup);
                    self.entries.clear();
                }
            },
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "[FavoriteManager: Load] Failed to read favorites configuration file"
                );
                self.entries.clear();
            }
        }
    }

    /// Atomically persists favorites to disk using a temporary sibling file and atomic replacement.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let t0 = Instant::now();
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let temp_path = self.config_path.with_extension("tmp");
        let json_data = serde_json::to_string_pretty(&self.entries)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut file = File::create(&temp_path)?;
        file.write_all(json_data.as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp_path, &self.config_path)?;

        tracing::debug!(
            entries_count = self.entries.len(),
            elapsed_us = t0.elapsed().as_micros(),
            "[FavoriteManager: Save] Favorites atomically saved to disk"
        );
        Ok(())
    }

    /// Checks if a folder path is already registered as a favorite via normalized path comparison.
    pub fn has_path(&self, path: &Path) -> bool {
        let normalized = normalize_path(path);
        self.entries
            .iter()
            .any(|entry| normalize_path(&entry.path) == normalized)
    }

    /// Retrieves an entry matching `path`.
    pub fn get_by_path(&self, path: &Path) -> Option<&FavoriteEntry> {
        let normalized = normalize_path(path);
        self.entries
            .iter()
            .find(|entry| normalize_path(&entry.path) == normalized)
    }

    /// Adds a directory path to favorites with an optional custom alias.
    pub fn add(&mut self, path: &PathBuf, alias: Option<String>) -> Result<FavoriteEntry, String> {
        let normalized = normalize_path(path);
        if self.has_path(&normalized) {
            return Err(format!("Folder '{}' is already in your favorites", path.display()));
        }

        let final_alias = alias.unwrap_or_else(|| {
            normalized.file_name().map_or_else(
                || normalized.to_string_lossy().to_string(),
                |f| f.to_string_lossy().to_string(),
            )
        });

        let entry = FavoriteEntry {
            alias: final_alias,
            path: normalized,
        };

        tracing::info!(
            alias = %entry.alias,
            path = %LogObscurer::path(&entry.path),
            "[FavoriteManager: Add] Registered new favorite entry"
        );

        self.entries.push(entry.clone());
        let _ = self.save();
        Ok(entry)
    }

    /// Renames the virtual display alias of an existing favorite entry.
    pub fn rename(&mut self, path: &Path, new_alias: &str) -> Result<(), String> {
        let trimmed_alias = new_alias.trim();
        if trimmed_alias.is_empty() {
            return Err("Favorite alias cannot be empty".to_string());
        }

        let normalized = normalize_path(path);
        if let Some(entry) = self.entries.iter_mut().find(|e| normalize_path(&e.path) == normalized) {
            tracing::info!(
                old_alias = %entry.alias,
                new_alias = %trimmed_alias,
                path = %LogObscurer::path(&entry.path),
                "[FavoriteManager: Rename] Virtual alias renamed"
            );
            entry.alias = trimmed_alias.to_string();
            let _ = self.save();
            Ok(())
        } else {
            Err("Favorite item not found".to_string())
        }
    }

    /// Removes a favorite entry by its filesystem path.
    pub fn remove_by_path(&mut self, path: &Path) -> Result<FavoriteEntry, String> {
        let normalized = normalize_path(path);
        if let Some(idx) = self.entries.iter().position(|e| normalize_path(&e.path) == normalized) {
            let removed = self.entries.remove(idx);
            tracing::info!(
                alias = %removed.alias,
                path = %LogObscurer::path(&removed.path),
                "[FavoriteManager: Remove] Removed favorite entry"
            );
            let _ = self.save();
            Ok(removed)
        } else {
            Err("Favorite item not found".to_string())
        }
    }

    /// Returns a slice of all stored favorite entries.
    pub fn entries(&self) -> &[FavoriteEntry] {
        &self.entries
    }

    /// Backward-compatibility query returning (alias, path, `is_valid_on_disk`).
    pub fn list(&self) -> Vec<(String, PathBuf, bool)> {
        self.entries
            .iter()
            .map(|e| (e.alias.clone(), e.path.clone(), e.path.is_dir()))
            .collect()
    }
}
