use super::diff::SettingsDiff;
use super::schema::AppSettings;
use super::store::SettingsStore;
use parking_lot::RwLock;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

pub type SettingsChangeHook = Box<dyn Fn(&AppSettings, &AppSettings, &SettingsDiff) + Send + Sync>;

/// Pure decoupled settings manager and intelligent delta subscriber registry.
pub struct SettingsManager {
    store: SettingsStore,
    current: RwLock<AppSettings>,
    hooks: RwLock<Vec<SettingsChangeHook>>,
}

impl SettingsManager {
    pub fn new(config_dir: &Path) -> Arc<Self> {
        let store = SettingsStore::new(config_dir);
        let current = RwLock::new(store.load());

        Arc::new(Self {
            store,
            current,
            hooks: RwLock::new(Vec::new()),
        })
    }

    pub fn get(&self) -> AppSettings {
        self.current.read().clone()
    }

    /// Evaluates settings deltas, persists changes atomically, and selectively notifies subscribers.
    /// If no actual settings were changed, all costly view resets and background reloads are bypassed.
    pub fn update(&self, new_settings: AppSettings) {
        let t0 = Instant::now();
        let (old_settings, diff) = {
            let reader = self.current.read();
            let old = reader.clone();
            let diff = SettingsDiff::compute(&old, &new_settings);
            (old, diff)
        };

        if !diff.has_changes {
            tracing::info!(
                "[SettingsManager: Diff] Settings save requested with ZERO changes. Bypassing disk I/O and all UI view resets."
            );
            return;
        }

        let summary = diff.detailed_summary(&old_settings, &new_settings);
        tracing::info!(
            changed_items_count = summary.len(),
            changes = ?summary,
            "[SettingsManager: Diff] Granular settings modification detected"
        );

        {
            let mut writer = self.current.write();
            *writer = new_settings.clone();
        }

        if let Err(e) = self.store.save(&new_settings) {
            tracing::error!(error = %e, "[SettingsManager] Failed to persist settings to disk");
        } else {
            tracing::debug!(
                elapsed_us = t0.elapsed().as_micros(),
                "[SettingsManager] Settings JSON file atomically written to disk"
            );
        }

        let hooks = self.hooks.read();
        for hook in hooks.iter() {
            hook(&old_settings, &new_settings, &diff);
        }
    }

    /// Persists settings quietly without triggering subscriber reload loops.
    pub fn save_quiet(&self, settings: &AppSettings) {
        {
            let mut writer = self.current.write();
            *writer = settings.clone();
        }
        let _ = self.store.save(settings);
    }

    /// Registers an event hook that receives the previous settings, new settings, and the computed granular diff.
    pub fn subscribe<F>(&self, hook: F)
    where
        F: Fn(&AppSettings, &AppSettings, &SettingsDiff) + Send + Sync + 'static,
    {
        self.hooks.write().push(Box::new(hook));
    }
}
