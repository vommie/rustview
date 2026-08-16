use rustview::settings::{
    AppSettings, GridAspectRatio, SettingsManager, SettingsStore, DEFAULT_THUMBNAIL_ASPECT_RATIO,
    DEFAULT_THUMBNAIL_WIDTH,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tempfile::tempdir;

#[test]
fn test_settings_persistence_sanitization_and_hooks() {
    let tmp_config = tempdir().unwrap();

    let manager = SettingsManager::new(tmp_config.path());
    let mut initial = manager.get();

    // 1. Verify default values from single central point of truth
    assert_eq!(initial.appearance.default_thumbnail_size, DEFAULT_THUMBNAIL_WIDTH);
    assert_eq!(
        initial.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
    assert_eq!(
        GridAspectRatio::from_str_name(&initial.appearance.thumbnail_aspect_ratio),
        GridAspectRatio::Ratio4x3
    );
    assert!(initial.general.remember_last_folder);
    assert_eq!(initial.general.root_folder, None);

    // 2. Test event hook subscription
    let hook_fired = Arc::new(AtomicBool::new(false));
    let hook_flag = hook_fired.clone();

    manager.subscribe(move |s| {
        if s.appearance.default_thumbnail_size == 240 {
            hook_flag.store(true, Ordering::SeqCst);
        }
    });

    // 3. Mutate and apply settings
    initial.appearance.default_thumbnail_size = 240;
    initial.appearance.thumbnail_aspect_ratio = "16:9".to_string();
    initial.appearance.show_filenames = false;
    manager.update(initial);

    assert!(hook_fired.load(Ordering::SeqCst));

    // 4. Test persistent reload from disk
    let reloaded_store = SettingsStore::new(tmp_config.path());
    let loaded = reloaded_store.load();
    assert_eq!(loaded.appearance.default_thumbnail_size, 240);
    assert_eq!(loaded.appearance.thumbnail_aspect_ratio, "16:9");
    assert!(!loaded.appearance.show_filenames);

    // 5. Test validation & sanitization (non-existent root is cleared to None)
    let mut invalid_settings = AppSettings::default();
    invalid_settings.general.root_folder = Some(tmp_config.path().join("does_not_exist"));
    invalid_settings.appearance.thumbnail_aspect_ratio = "invalid_ratio".to_string();
    reloaded_store.save(&invalid_settings).unwrap();

    let sanitized = reloaded_store.load();
    assert_eq!(sanitized.general.root_folder, None);
    assert_eq!(
        sanitized.appearance.thumbnail_aspect_ratio,
        DEFAULT_THUMBNAIL_ASPECT_RATIO
    );
}
