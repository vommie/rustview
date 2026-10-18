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
const PROVIDERS: [BadgeProvider; 6] =
    [tag_badge, rating_badge, media_type_badge, duration_badge, folder_status_badge, folder_missing_badge];

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
    Some(ThumbnailBadge::text("media_type", label).at(ThumbnailBadgeOrientation::BottomLeft).tone(tone))
}

/// Compact video duration (bottom-right). Only shown for videos with a known duration.
fn duration_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    (config.show_duration && !item.is_folder && item.is_video && !item.duration_badge_str.is_empty()).then(|| {
        ThumbnailBadge::text("duration", item.duration_badge_str.as_str()).at(ThumbnailBadgeOrientation::BottomRight)
    })
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
    Some(ThumbnailBadge::text("folder_status", label).at(ThumbnailBadgeOrientation::BottomLeft).tone(tone))
}

/// "N MISSING" (bottom-right) on a folder that is there but has subfolders that are gone from the drive.
fn folder_missing_badge(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<ThumbnailBadge> {
    (config.show_media_type && item.is_folder && !item.is_folder_up && !item.is_deleted && item.folder_missing_children > 0).then(|| {
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
            overflow_mode: ThumbnailBadgeOverflowMode::Hide,
        }
    }

    fn video() -> MediaItemView {
        MediaItemView { is_video: true, duration_badge_str: "04:30".into(), ..Default::default() }
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
        let image = MediaItemView { duration_badge_str: "04:30".into(), ..Default::default() };
        assert_eq!(ids(&image, &all_enabled()), ["media_type"]);
    }

    #[test]
    fn videos_without_known_duration_have_no_duration_badge() {
        let item = MediaItemView { is_video: true, ..Default::default() };
        assert_eq!(ids(&item, &all_enabled()), ["media_type"]);
    }

    #[test]
    fn rating_and_tag_badges_follow_item_state() {
        let item = MediaItemView { rating: 3, has_tags: true, ..Default::default() };
        assert_eq!(ids(&item, &all_enabled()), ["tag", "rating", "media_type"]);
        let unrated = MediaItemView::default();
        assert_eq!(ids(&unrated, &all_enabled()), ["media_type"]);
    }

    #[test]
    fn disabled_options_suppress_their_badges() {
        let item = MediaItemView { rating: 5, has_tags: true, is_video: true, duration_badge_str: "5s".into(), ..Default::default() };
        let none = ThumbnailBadgeConfig { show_rating: false, show_tag: false, show_media_type: false, show_duration: false, ..all_enabled() };
        assert!(ids(&item, &none).is_empty());
        let only_duration = ThumbnailBadgeConfig { show_duration: true, ..none };
        assert_eq!(ids(&item, &only_duration), ["duration"]);
    }

    #[test]
    fn status_label_priority_is_pending_deleted_corrupted_type() {
        let label = |item: MediaItemView| match &collect_badges(&item, &all_enabled())[0].content {
            super::super::badge::BadgeContent::Text(t) => t.clone(),
            other => panic!("unexpected content {other:?}"),
        };
        let base = MediaItemView { is_video: true, ..Default::default() };
        assert_eq!(label(MediaItemView { is_pending: true, is_deleted: true, is_corrupted: true, ..base.clone() }), "PENDING");
        assert_eq!(label(MediaItemView { is_deleted: true, is_corrupted: true, ..base.clone() }), "DELETED");
        assert_eq!(label(MediaItemView { is_corrupted: true, ..base.clone() }), "CORRUPT");
        assert_eq!(label(base), "VIDEO");
        assert_eq!(label(MediaItemView { format_str: "gif".into(), ..Default::default() }), "GIF");
        assert_eq!(label(MediaItemView::default()), "IMAGE");
    }

    fn folder() -> MediaItemView {
        MediaItemView { is_folder: true, ..Default::default() }
    }

    fn label(item: &MediaItemView, config: &ThumbnailBadgeConfig) -> Option<String> {
        collect_badges(item, config).into_iter().find(|b| b.id == "folder_status").map(|b| match b.content {
            super::super::badge::BadgeContent::Text(t) => t,
            other => panic!("unexpected content {other:?}"),
        })
    }

    #[test]
    fn folders_show_a_type_label_and_no_media_badges() {
        assert_eq!(ids(&folder(), &all_enabled()), ["folder_status"]);
        assert_eq!(label(&folder(), &all_enabled()).unwrap(), "FOLDER");
        // Ratings, tags and durations do not exist for folders, whatever the item carries.
        let odd = MediaItemView { rating: 5, has_tags: true, is_video: true, duration_badge_str: "1s".into(), ..folder() };
        assert_eq!(ids(&odd, &all_enabled()), ["folder_status"]);
    }

    #[test]
    fn folder_status_priority_is_missing_hidden_empty_folder() {
        let all = MediaItemView { is_deleted: true, folder_hidden: true, folder_empty: true, ..folder() };
        assert_eq!(label(&all, &all_enabled()).unwrap(), "MISSING");
        let hidden = MediaItemView { folder_hidden: true, folder_empty: true, ..folder() };
        assert_eq!(label(&hidden, &all_enabled()).unwrap(), "HIDDEN");
        let empty = MediaItemView { folder_empty: true, ..folder() };
        assert_eq!(label(&empty, &all_enabled()).unwrap(), "EMPTY");
    }

    #[test]
    fn folder_tones_match_the_state() {
        let tone = |item: MediaItemView| collect_badges(&item, &all_enabled()).remove(0).tone;
        assert_eq!(tone(MediaItemView { is_deleted: true, ..folder() }), ThumbnailBadgeTone::Muted);
        assert_eq!(tone(MediaItemView { folder_hidden: true, ..folder() }), ThumbnailBadgeTone::Info);
        assert_eq!(tone(folder()), ThumbnailBadgeTone::Standard);
    }

    #[test]
    fn missing_subfolders_get_a_counter_in_the_other_corner() {
        let item = MediaItemView { folder_missing_children: 3, ..folder() };
        let badges = collect_badges(&item, &all_enabled());
        assert_eq!(badges.iter().map(|b| b.id).collect::<Vec<_>>(), ["folder_status", "folder_missing"]);
        assert_eq!(badges[1].content, super::super::badge::BadgeContent::Text("3 MISSING".into()));
        assert_eq!(badges[1].orientation, ThumbnailBadgeOrientation::BottomRight);
        // A missing folder is missing as a whole; no counter.
        let missing = MediaItemView { is_deleted: true, folder_missing_children: 3, ..folder() };
        assert_eq!(ids(&missing, &all_enabled()), ["folder_status"]);
    }

    #[test]
    fn the_status_option_switches_every_folder_badge_off() {
        let item = MediaItemView { folder_missing_children: 2, folder_hidden: true, ..folder() };
        let off = ThumbnailBadgeConfig { show_media_type: false, ..all_enabled() };
        assert!(ids(&item, &off).is_empty());
        // The other options do not touch folders.
        let only_media_type = ThumbnailBadgeConfig { show_rating: false, show_tag: false, show_duration: false, ..all_enabled() };
        assert_eq!(ids(&item, &only_media_type), ["folder_status", "folder_missing"]);
    }

    #[test]
    fn the_up_entry_has_no_badges() {
        let up = MediaItemView { is_folder_up: true, ..folder() };
        assert!(ids(&up, &all_enabled()).is_empty());
    }

    #[test]
    fn rating_tone_clamps_out_of_range_values() {
        assert_eq!(rating_tone(-3), ThumbnailBadgeTone::Rating0);
        assert_eq!(rating_tone(5), ThumbnailBadgeTone::Rating5);
        assert_eq!(rating_tone(99), ThumbnailBadgeTone::Rating5);
    }
}
