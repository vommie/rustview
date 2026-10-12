//! Composition of the badge pipeline: collect -> group by corner -> order -> convert to Slint views.
//!
//! Sizing, wrapping and clipping are intentionally *not* done here: the Slint layout measures the
//! real rendered content, so no font-dependent estimates are needed.

use super::badge::{BadgeContent, ThumbnailBadge};
use super::ordering::order_badges;
use super::providers::collect_badges;
use crate::ui::{
    MediaItemView, ThumbnailBadgeConfig, ThumbnailBadgeKind, ThumbnailBadgeOrientation, ThumbnailBadgeView,
    ThumbnailBadgeZones,
};
use slint::{Color, ModelRc, SharedString, VecModel};

/// Collects and orders all badges of one media item per thumbnail corner.
pub fn compute_badge_zones(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> ThumbnailBadgeZones {
    let badges = collect_badges(item, config);
    let collected = badges.len();

    let mut top_left = Vec::new();
    let mut top_right = Vec::new();
    let mut bottom_left = Vec::new();
    let mut bottom_right = Vec::new();
    for badge in badges {
        match badge.orientation {
            ThumbnailBadgeOrientation::TopLeft => top_left.push(badge),
            ThumbnailBadgeOrientation::TopRight => top_right.push(badge),
            ThumbnailBadgeOrientation::BottomLeft => bottom_left.push(badge),
            ThumbnailBadgeOrientation::BottomRight => bottom_right.push(badge),
        }
    }

    tracing::trace!(media_id = item.id, collected, "[ThumbnailBadges] Badges collected");

    ThumbnailBadgeZones {
        top_left: zone_model(top_left),
        top_right: zone_model(top_right),
        bottom_left: zone_model(bottom_left),
        bottom_right: zone_model(bottom_right),
    }
}

fn zone_model(badges: Vec<ThumbnailBadge>) -> ModelRc<ThumbnailBadgeView> {
    let views: Vec<ThumbnailBadgeView> = order_badges(badges).iter().map(to_view).collect();
    ModelRc::new(VecModel::from(views))
}

/// Converts a badge spec to the Slint view struct.
fn to_view(badge: &ThumbnailBadge) -> ThumbnailBadgeView {
    let (kind, text, icon_name) = match &badge.content {
        BadgeContent::Text(t) => (ThumbnailBadgeKind::Text, t.as_str(), ""),
        BadgeContent::Round(t) => (ThumbnailBadgeKind::Round, t.as_str(), ""),
        BadgeContent::Icon(name) => (ThumbnailBadgeKind::Icon, "", name.as_str()),
    };
    let transparent = Color::from_argb_u8(0, 0, 0, 0);

    ThumbnailBadgeView {
        id: badge.id.into(),
        orientation: badge.orientation,
        kind,
        tone: badge.tone,
        text: SharedString::from(text),
        icon_name: SharedString::from(icon_name),
        has_custom_color: badge.color.is_some(),
        custom_color: badge.color.map(|c| c.to_slint()).unwrap_or(transparent),
        custom_text_color: badge.color.map(|c| c.contrast_text()).unwrap_or(transparent),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ThumbnailBadgeOverflowMode;
    use slint::Model;

    fn config() -> ThumbnailBadgeConfig {
        ThumbnailBadgeConfig {
            show_rating: true,
            show_tag: true,
            show_media_type: true,
            show_duration: true,
            overflow_mode: ThumbnailBadgeOverflowMode::Hide,
        }
    }

    fn ids(zone: &ModelRc<ThumbnailBadgeView>) -> Vec<String> {
        zone.iter().map(|v| v.id.to_string()).collect()
    }

    #[test]
    fn badges_are_grouped_into_their_corners() {
        let item = MediaItemView {
            is_video: true,
            duration_badge_str: "04:30".into(),
            rating: 4,
            has_tags: true,
            ..Default::default()
        };
        let zones = compute_badge_zones(&item, &config());
        assert_eq!(ids(&zones.top_left), ["tag"]);
        assert_eq!(ids(&zones.top_right), ["rating"]);
        assert_eq!(ids(&zones.bottom_left), ["media_type"]);
        assert_eq!(ids(&zones.bottom_right), ["duration"]);
    }

    #[test]
    fn duration_view_carries_text_and_kind() {
        let item = MediaItemView { is_video: true, duration_badge_str: "04:30".into(), ..Default::default() };
        let zones = compute_badge_zones(&item, &config());
        let duration = zones.bottom_right.row_data(0).unwrap();
        assert_eq!(duration.kind, ThumbnailBadgeKind::Text);
        assert_eq!(duration.text, "04:30");
        assert!(!duration.has_custom_color);
    }

    #[test]
    fn item_without_applicable_badges_yields_empty_zones() {
        let none = ThumbnailBadgeConfig { show_media_type: false, ..config() };
        let zones = compute_badge_zones(&MediaItemView::default(), &none);
        assert!(ids(&zones.top_left).is_empty() && ids(&zones.top_right).is_empty());
        assert!(ids(&zones.bottom_left).is_empty() && ids(&zones.bottom_right).is_empty());
    }
}
