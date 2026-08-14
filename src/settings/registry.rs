use super::schema::AppSettings;
use super::store::SettingsStore;
use parking_lot::RwLock;
use std::path::Path;
use std::sync::Arc;

pub type SettingsHook = Box<dyn Fn(&AppSettings) + Send + Sync>;

/// Pure decoupled settings manager and subscriber registry.
pub struct SettingsManager {
    store: SettingsStore,
    current: RwLock<AppSettings>,
    hooks: RwLock<Vec<SettingsHook>>,
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

    /// Updates settings, persists them atomically, and notifies all subscribers.
    pub fn update(&self, new_settings: AppSettings) {
        {
            let mut writer = self.current.write();
            *writer = new_settings.clone();
        }
        let _ = self.store.save(&new_settings);

        let hooks = self.hooks.read();
        for hook in hooks.iter() {
            hook(&new_settings);
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

    /// Registers an event hook that fires whenever settings are explicitly changed.
    pub fn subscribe<F>(&self, hook: F)
    where
        F: Fn(&AppSettings) + Send + Sync + 'static,
    {
        self.hooks.write().push(Box::new(hook));
    }
}
