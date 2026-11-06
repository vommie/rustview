//! "Reset this page" of the Settings window (held, `RustCut`'s page header, added in 0.6.14): the options of one page go
//! back to their defaults, while what the app remembers as data stays (the root folder, the last folder, expanded
//! folders and tags, the tag history, the last export).

use super::schema::AppSettings;

/// The pages of the Settings window, in the order of its sidebar (`ui/settings_sidebar.slint`).
pub const PAGE_COUNT: usize = 12;
/// The Hotkeys page has its own held "reset all keys" button and no page reset.
pub const HOTKEYS_PAGE: usize = 9;

/// Whether `page` has a reset button.
pub fn page_can_reset(page: usize) -> bool {
    page < PAGE_COUNT && page != HOTKEYS_PAGE
}

/// Puts the options of `page` back to their defaults; returns whether it is a page that can be reset.
pub fn reset_page(settings: &mut AppSettings, page: usize) -> bool {
    let defaults = AppSettings::default();
    match page {
        0 => {
            let root = settings.general.root_folder.take();
            let last = settings.general.last_folder_path.take();
            settings.general = defaults.general;
            settings.general.root_folder = root;
            settings.general.last_folder_path = last;
        }
        1 => {
            let expanded = std::mem::take(&mut settings.folders.expanded_folder_ids);
            settings.folders = defaults.folders;
            settings.folders.expanded_folder_ids = expanded;
        }
        2 => settings.appearance = defaults.appearance,
        3 => settings.performance = defaults.performance,
        4 => settings.formats = defaults.formats,
        // Viewer: how media open, the tabs and the background; the video player's options belong to Multiview.
        5 => {
            settings.viewer.images = defaults.viewer.images;
            settings.viewer.tabs = defaults.viewer.tabs;
            settings.viewer.background_mode = defaults.viewer.background_mode;
        }
        6 => {
            settings.fullscreen = defaults.fullscreen;
            settings.viewer.videos = defaults.viewer.videos;
        }
        7 => {
            let last_export = std::mem::take(&mut settings.image_edit.last_export);
            settings.image_edit = defaults.image_edit;
            settings.image_edit.last_export = last_export;
        }
        8 => {
            let expanded = std::mem::take(&mut settings.tags.expanded_tag_ids);
            let history = std::mem::take(&mut settings.tags.history_tag_ids);
            let (collapsed, height) = (settings.tags.is_history_collapsed, settings.tags.history_height);
            settings.tags = defaults.tags;
            settings.tags.expanded_tag_ids = expanded;
            settings.tags.history_tag_ids = history;
            settings.tags.is_history_collapsed = collapsed;
            settings.tags.history_height = height;
        }
        10 => settings.api = defaults.api,
        11 => settings.debug = defaults.debug,
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn general_keeps_the_root_folder_and_resets_the_options() {
        let mut s = AppSettings::default();
        s.general.root_folder = Some(PathBuf::from("/media"));
        s.general.last_folder_path = Some(PathBuf::from("/media/a"));
        s.general.confirm_open_file_count = 99;
        assert!(reset_page(&mut s, 0));
        assert_eq!(s.general.root_folder, Some(PathBuf::from("/media")));
        assert_eq!(s.general.last_folder_path, Some(PathBuf::from("/media/a")));
        assert_eq!(
            s.general.confirm_open_file_count,
            AppSettings::default().general.confirm_open_file_count
        );
    }

    #[test]
    fn tags_keep_the_history_and_the_open_branches() {
        let mut s = AppSettings::default();
        s.tags.history_tag_ids = vec![3, 1];
        s.tags.expanded_tag_ids = vec![7];
        s.tags.compact_tree = !s.tags.compact_tree;
        assert!(reset_page(&mut s, 8));
        assert_eq!(s.tags.history_tag_ids, [3, 1]);
        assert_eq!(s.tags.expanded_tag_ids, [7]);
        assert_eq!(s.tags.compact_tree, AppSettings::default().tags.compact_tree);
    }

    #[test]
    fn a_page_reset_touches_no_other_page() {
        let mut s = AppSettings::default();
        s.appearance.show_filenames = !s.appearance.show_filenames;
        s.api.auto_complete_external_stubs = !s.api.auto_complete_external_stubs;
        let before = s.clone();
        assert!(reset_page(&mut s, 10));
        assert_eq!(s.appearance, before.appearance, "Appearance stays");
        assert_ne!(s.api, before.api);
    }

    #[test]
    fn hotkeys_and_unknown_pages_have_no_reset() {
        let mut s = AppSettings::default();
        assert!(!page_can_reset(HOTKEYS_PAGE));
        assert!(!reset_page(&mut s, HOTKEYS_PAGE));
        assert!(!reset_page(&mut s, 99));
        assert!(page_can_reset(0) && page_can_reset(11) && !page_can_reset(12));
    }
}
