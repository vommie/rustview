//! Connects the badge engine to the Slint UI: registers the layout callback and pushes the
//! user configuration (from the Appearance settings) to the main window.

use super::engine::compute_badge_zones;
use crate::ui::{MainWindow, ThumbnailBadgeConfig, ThumbnailBadgeEngine, ThumbnailBadgeOverflowMode};
use rustview_core::settings::ThumbnailBadgeOverflow;
use rustview_core::settings::schema::AppearanceSettings;
use slint::ComponentHandle;

pub struct ThumbnailBadgeBridge;

impl ThumbnailBadgeBridge {
    /// Registers the Rust implementation for `ThumbnailBadgeEngine.collect`.
    pub fn register(window: &MainWindow) {
        window
            .global::<ThumbnailBadgeEngine>()
            .on_collect(|item, config| compute_badge_zones(&item, &config));
        tracing::info!("[ThumbnailBadges] Badge engine registered");
    }

    /// Pushes the badge-related appearance settings to the main window.
    pub fn apply_config(window: &MainWindow, appearance: &AppearanceSettings) {
        let config = Self::config_from_settings(appearance);
        tracing::debug!(
            show_rating = config.show_rating,
            show_tag = config.show_tag,
            show_media_type = config.show_media_type,
            show_duration = config.show_duration,
            show_animated = config.show_animated,
            overflow_mode = ?config.overflow_mode,
            "[ThumbnailBadges] Applying badge configuration"
        );
        window.set_thumbnail_badge_config(config);
    }

    /// Maps the persisted appearance settings to the Slint configuration struct.
    pub fn config_from_settings(appearance: &AppearanceSettings) -> ThumbnailBadgeConfig {
        let overflow_mode = match ThumbnailBadgeOverflow::from_str_name(&appearance.thumbnail_badge_overflow) {
            ThumbnailBadgeOverflow::Hide => ThumbnailBadgeOverflowMode::Hide,
            ThumbnailBadgeOverflow::Wrap => ThumbnailBadgeOverflowMode::Wrap,
        };
        ThumbnailBadgeConfig {
            show_rating: appearance.show_ratings_overlay,
            show_tag: appearance.show_tags_overlay,
            show_media_type: appearance.show_media_type_badges,
            show_duration: appearance.show_duration_badge,
            show_animated: appearance.show_animated_badge,
            overflow_mode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_enable_all_badges_and_hide_on_overflow() {
        let config = ThumbnailBadgeBridge::config_from_settings(&AppearanceSettings::default());
        assert!(config.show_rating && config.show_tag && config.show_media_type && config.show_duration);
        assert_eq!(config.overflow_mode, ThumbnailBadgeOverflowMode::Hide);
    }

    #[test]
    fn wrap_setting_and_unknown_values_are_mapped_safely() {
        let mut appearance = AppearanceSettings {
            thumbnail_badge_overflow: "wrap".into(),
            ..Default::default()
        };
        assert_eq!(
            ThumbnailBadgeBridge::config_from_settings(&appearance).overflow_mode,
            ThumbnailBadgeOverflowMode::Wrap
        );
        appearance.thumbnail_badge_overflow = "bogus".into();
        assert_eq!(
            ThumbnailBadgeBridge::config_from_settings(&appearance).overflow_mode,
            ThumbnailBadgeOverflowMode::Hide
        );
    }
}
