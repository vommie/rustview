use rustview::settings::{
    SettingsManager, SettingsStore, DEFAULT_SIDEBAR_LEFT_WIDTH, DEFAULT_SIDEBAR_RIGHT_WIDTH,
    MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH,
};
use tempfile::tempdir;

#[test]
fn test_sidebar_widths_default_and_persistence() {
    let tmp_config = tempdir().unwrap();
    let manager = SettingsManager::new(tmp_config.path());

    let initial = manager.get();
    assert_eq!(initial.view.sidebar_left_width, DEFAULT_SIDEBAR_LEFT_WIDTH);
    assert_eq!(initial.view.sidebar_right_width, DEFAULT_SIDEBAR_RIGHT_WIDTH);

    let mut updated = initial;
    updated.view.sidebar_left_width = 340.0;
    updated.view.sidebar_right_width = 410.0;
    manager.update(updated);

    let reloaded_store = SettingsStore::new(tmp_config.path());
    let loaded = reloaded_store.load();
    assert_eq!(loaded.view.sidebar_left_width, 340.0);
    assert_eq!(loaded.view.sidebar_right_width, 410.0);
}

#[test]
fn test_sidebar_widths_clamping_sanitization() {
    let tmp_config = tempdir().unwrap();
    let store = SettingsStore::new(tmp_config.path());

    let mut settings = store.load();
    // Test values outside allowable boundaries
    settings.view.sidebar_left_width = 50.0; // Below MIN_SIDEBAR_WIDTH (150.0)
    settings.view.sidebar_right_width = 1200.0; // Above MAX_SIDEBAR_WIDTH (700.0)
    store.save(&settings).unwrap();

    let sanitized = store.load();
    assert_eq!(sanitized.view.sidebar_left_width, MIN_SIDEBAR_WIDTH);
    assert_eq!(sanitized.view.sidebar_right_width, MAX_SIDEBAR_WIDTH);
}
