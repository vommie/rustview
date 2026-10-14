use super::diff::SettingsDiff;
use super::obscurer::LogObscurer;
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
    current: Arc<RwLock<AppSettings>>,
    hooks: RwLock<Vec<SettingsChangeHook>>,
}

impl SettingsManager {
    pub fn new(config_dir: &Path) -> Arc<Self> {
        let store = SettingsStore::new(config_dir);
        let loaded = store.load();
        LogObscurer::set_obfuscate(loaded.debug.obfuscate_logs);
        let current = Arc::new(RwLock::new(loaded));

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

        // Live update log obfuscator state before emitting detailed summary
        LogObscurer::set_obfuscate(new_settings.debug.obfuscate_logs);

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

        if let Err(e) = self.persist() {
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
        LogObscurer::set_obfuscate(settings.debug.obfuscate_logs);
        {
            let mut writer = self.current.write();
            *writer = settings.clone();
        }
        if let Err(e) = self.persist() {
            tracing::error!(error = %e, "[SettingsManager] Failed to persist settings to disk");
        }
    }

    /// Writes the current in-memory settings to disk. Writers take turns and each one writes whatever is current once it is
    /// its turn, so the file never ends up older than the state in memory.
    fn persist(&self) -> Result<(), std::io::Error> {
        self.store.save_latest(|| self.current.read().clone())
    }

    /// Makes sure the file on disk holds the current settings: waits for a background saver that is writing right now and
    /// writes once more. Called when the application exits.
    pub fn flush(&self) {
        match self.persist() {
            Ok(()) => tracing::info!("[SettingsManager: Flush] Settings written to disk"),
            Err(e) => tracing::error!(error = %e, "[SettingsManager: Flush] Failed to write the settings to disk"),
        }
    }

    /// Persists settings in-memory immediately and flushes JSON to disk in a non-blocking background thread.
    pub fn save_quiet_async(&self, settings: &AppSettings) {
        LogObscurer::set_obfuscate(settings.debug.obfuscate_logs);
        {
            let mut writer = self.current.write();
            *writer = settings.clone();
        }
        let store = self.store.clone();
        let current = self.current.clone();
        std::thread::Builder::new()
            .name("settings-async-saver".into())
            .spawn(move || {
                let _ = store.save_latest(|| current.read().clone());
            })
            .ok();
    }

    /// Registers an event hook that receives the previous settings, new settings, and the computed granular diff.
    pub fn subscribe<F>(&self, hook: F)
    where
        F: Fn(&AppSettings, &AppSettings, &SettingsDiff) + Send + Sync + 'static,
    {
        self.hooks.write().push(Box::new(hook));
    }
}
