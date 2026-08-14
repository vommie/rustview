use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FavoriteEntry {
    pub alias: String,
    pub path: PathBuf,
}

pub struct FavoriteManager {
    config_path: PathBuf,
    favorites: HashMap<String, PathBuf>,
}

impl FavoriteManager {
    pub fn new(config_dir: &Path) -> Self {
        let config_path = config_dir.join("favorites.json");
        let mut manager = Self {
            config_path,
            favorites: HashMap::new(),
        };
        manager.load();
        manager
    }

    pub fn load(&mut self) {
        if self.config_path.exists()
            && let Ok(data) = fs::read_to_string(&self.config_path)
            && let Ok(favs) = serde_json::from_str::<Vec<FavoriteEntry>>(&data)
        {
            self.favorites = favs.into_iter().map(|f| (f.alias, f.path)).collect();
        }
    }

    pub fn save(&self) {
        let list: Vec<FavoriteEntry> = self
            .favorites
            .iter()
            .map(|(a, p)| FavoriteEntry {
                alias: a.clone(),
                path: p.clone(),
            })
            .collect();
        if let Ok(data) = serde_json::to_string_pretty(&list) {
            let _ = fs::write(&self.config_path, data);
        }
    }

    pub fn add(&mut self, path: PathBuf, alias: Option<String>) {
        let final_alias = alias.unwrap_or_else(|| {
            path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "Folder".to_string())
        });
        self.favorites.insert(final_alias, path);
        self.save();
    }

    pub fn remove(&mut self, alias: &str) {
        self.favorites.remove(alias);
        self.save();
    }

    pub fn list(&self) -> Vec<(String, PathBuf, bool)> {
        let mut items = Vec::new();
        for (alias, path) in &self.favorites {
            items.push((alias.clone(), path.clone(), path.is_dir()));
        }
        items.sort_by_key(|a| a.0.to_lowercase());
        items
    }
}
