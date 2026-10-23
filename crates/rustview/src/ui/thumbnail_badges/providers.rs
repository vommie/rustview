//! Badge providers: one small function per badge type. A provider looks at a media item and the
//! user configuration and either contributes a badge or returns `None`.
//!
//! To add a new badge, write a provider and append it to `PROVIDERS`. The position in that list is
//! the automatic order of the badges inside their area.

use super::badge::ThumbnailBadge;
use super::color::BadgeColor;
use crate::icons::Icon;
use crate::ui::{MediaItemView, ThumbnailBadgeConfig, ThumbnailBadgeOrientation, ThumbnailBadgeTone};

type BadgeProvider = fn(&MediaItemView, &ThumbnailBadgeConfig) -> Option<ThumbnailBadge>;

/// All badge providers in automatic (registration) order.
const PROVIDERS: [BadgeProvider; 7] = [
    tag_badge,
    rating_badge,
    media_type_badge,
    duration_badge,
    animated_badge,
    folder_status_badge,
    folder_missing_badge,
];

/// Collects the badges of one media item from all providers.
pub fn collect_badges(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Vec<ThumbnailBadge> {
    PROVIDERS.iter().filter_map(|provider| provider(item, config)).collect()
}

/// Orange of the filled tag icon on thumbnails (the only place where the tag icon is filled and colored).
const TAG_BADGE_COLOR: BadgeColor = BadgeColor::rgba(0xe8, 0x95, 0x00, 0xff);

/// Tag indicator icon (top-left) for items with assigned tags.
fn tag_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    (config.show_tag && !item.is_folder && item.has_tags).then(|| {
        ThumbnailBadge::icon("tag", Icon::TagFilled)
            .at(ThumbnailBadgeOrientation::TopLeft)
            .color(TAG_BADGE_COLOR)
    })
}

/// Colored rating circle (top-right) for rated items.
fn rating_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    (config.show_rating && !item.is_folder && item.rating > 0).then(|| {
        ThumbnailBadge::round("rating", item.rating.to_string())
            .at(ThumbnailBadgeOrientation::TopRight)
            .tone(rating_tone(item.rating))
    })
}

fn rating_tone(rating: i32) -> ThumbnailBadgeTone {
    match rating {
        i32::MIN..=0 => ThumbnailBadgeTone::Rating0,
        1 => ThumbnailBadgeTone::Rating1,
        2 => ThumbnailBadgeTone::Rating2,
        3 => ThumbnailBadgeTone::Rating3,
        4 => ThumbnailBadgeTone::Rating4,
        _ => ThumbnailBadgeTone::Rating5,
    }
}

/// Media type / status label (bottom-left).
/// Priority: Pending stub > Deleted > Corrupted > Video > GIF > Image.
fn media_type_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    if !config.show_media_type || item.is_folder {
        return None;
    }
    let (label, tone) = if item.is_pending {
        ("PENDING", ThumbnailBadgeTone::Info)
    } else if item.is_deleted {
        ("DELETED", ThumbnailBadgeTone::Muted)
    } else if item.is_corrupted {
        ("CORRUPT", ThumbnailBadgeTone::Danger)
    } else if item.is_video {
        ("VIDEO", ThumbnailBadgeTone::Standard)
    } else if item.format_str == "gif" {
        ("GIF", ThumbnailBadgeTone::Standard)
    } else {
        ("IMAGE", ThumbnailBadgeTone::Standard)
    };
    Some(
        ThumbnailBadge::text("media_type", label)
            .at(ThumbnailBadgeOrientation::BottomLeft)
            .tone(tone),
    )
}

/// Compact video duration (bottom-right). Only shown for videos with a known duration.
fn duration_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    (config.show_duration && !item.is_folder && item.is_video && !item.duration_badge_str.is_empty()).then(|| {
        ThumbnailBadge::text("duration", item.duration_badge_str.as_str()).at(ThumbnailBadgeOrientation::BottomRight)
    })
}

/// "ANIM" (bottom-right) on pictures that have stored hover preview frames; images have no duration badge, so the corner is free.
fn animated_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    (config.show_animated && !item.is_folder && !item.is_video && item.is_animated)
        .then(|| ThumbnailBadge::text("animated", "ANIM").at(ThumbnailBadgeOrientation::BottomRight))
}

/// Type / status label of a folder row (bottom-left), the folder counterpart of `media_type_badge` and governed by the same option.
/// Priority: Missing > Hidden > Empty > Folder. The navigation entry "..." has none.
fn folder_status_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    if !config.show_media_type || !item.is_folder || item.is_folder_up {
        return None;
    }
    let (label, tone) = if item.is_deleted {
        ("MISSING", ThumbnailBadgeTone::Muted)
    } else if item.folder_hidden {
        ("HIDDEN", ThumbnailBadgeTone::Info)
    } else if item.folder_empty {
        ("EMPTY", ThumbnailBadgeTone::Muted)
    } else {
        ("FOLDER", ThumbnailBadgeTone::Standard)
    };
    Some(
        ThumbnailBadge::text("folder_status", label)
            .at(ThumbnailBadgeOrientation::BottomLeft)
            .tone(tone),
    )
}

/// "N MISSING" (bottom-right) on a folder that is there but has subfolders that are gone from the drive.
fn folder_missing_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    (config.show_media_type
        && item.is_folder
        && !item.is_folder_up
        && !item.is_deleted
        && item.folder_missing_children > 0)
        .then(|| {
            ThumbnailBadge::text("folder_missing", format!("{} MISSING", item.folder_missing_children))
                .at(ThumbnailBadgeOrientation::BottomRight)
                .tone(ThumbnailBadgeTone::Danger)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::ThumbnailBadgeOverflowMode;

    fn all_enabled() -> ThumbnailBadgeConfig {
        ThumbnailBadgeConfig {
            show_rating: true,
            show_tag: true,
            show_media_type: true,
            show_duration: true,
            show_animated: true,
            overflow_mode: ThumbnailBadgeOverflowMode::Hide,
        }
    }

    fn video() -> MediaItemView {
        MediaItemView {
            is_video: true,
            duration_badge_str: "04:30".into(),
            ..Default::default()
        }
    }

    fn ids(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Vec<&'static str> {
        collect_badges(item, config).iter().map(|b| b.id).collect()
    }

    #[test]
    fn video_shows_media_type_and_duration() {
        assert_eq!(ids(&video(), &all_enabled()), ["media_type", "duration"]);
    }

    #[test]
    fn images_never_show_a_duration() {
        let image = MediaItemView {
            duration_badge_str: "04:30".into(),
            ..Default::default()
        };
        assert_eq!(ids(&image, &all_enabled()), ["media_type"]);
    }

    #[test]
    fn videos_without_known_duration_have_no_duration_badge() {
        let item = MediaItemView {
            is_video: true,
            ..Default::default()
        };
        assert_eq!(ids(&item, &all_enabled()), ["media_type"]);
    }

    #[test]
    fn rating_and_tag_badges_follow_item_state() {
        let item = MediaItemView {
            rating: 3,
            has_tags: true,
            ..Default::default()
        };
        assert_eq!(ids(&item, &all_enabled()), ["tag", "rating", "media_type"]);
        let unrated = MediaItemView::default();
        assert_eq!(ids(&unrated, &all_enabled()), ["media_type"]);
    }

    #[test]
    fn disabled_options_suppress_their_badges() {
        let item = MediaItemView {
            rating: 5,
            has_tags: true,
            is_video: true,
            duration_badge_str: "5s".into(),
            ..Default::default()
        };
        let none = ThumbnailBadgeConfig {
            show_rating: false,
            show_tag: false,
            show_media_type: false,
            show_duration: false,
            show_animated: false,
            ..all_enabled()
        };
        assert_eq!(ids(&item, &none), [] as [&str; 0]);
        let only_duration = ThumbnailBadgeConfig {
            show_duration: true,
            ..none
        };
        assert_eq!(ids(&item, &only_duration), ["duration"]);
    }

    #[test]
    fn status_label_priority_is_pending_deleted_corrupted_type() {
        let label = |item: MediaItemView| match &collect_badges(&item, &all_enabled())[0].content {
            super::super::badge::BadgeContent::Text(t) => t.clone(),
            other => panic!("unexpected content {other:?}"),
        };
        let base = MediaItemView {
            is_video: true,
            ..Default::default()
        };
        assert_eq!(
            label(MediaItemView {
                is_pending: true,
                is_deleted: true,
                is_corrupted: true,
                ..base.clone()
            }),
            "PENDING"
        );
        assert_eq!(
            label(MediaItemView {
                is_deleted: true,
                is_corrupted: true,
                ..base.clone()
            }),
            "DELETED"
        );
        assert_eq!(
            label(MediaItemView {
                is_corrupted: true,
                ..base.clone()
            }),
            "CORRUPT"
        );
        assert_eq!(label(base), "VIDEO");
        assert_eq!(
            label(MediaItemView {
                format_str: "gif".into(),
                ..Default::default()
            }),
            "GIF"
        );
        assert_eq!(label(MediaItemView::default()), "IMAGE");
    }

    #[test]
    fn rating_tone_clamps_out_of_range_values() {
        assert_eq!(rating_tone(-3), ThumbnailBadgeTone::Rating0);
        assert_eq!(rating_tone(5), ThumbnailBadgeTone::Rating5);
        assert_eq!(rating_tone(99), ThumbnailBadgeTone::Rating5);
    }
}
