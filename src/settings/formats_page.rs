//! Settings > File Formats: builds the rows of the page and keeps the switches the user is editing until "Save".
//!
//! The switches work on a copy of the policy (`PageState::mask`); nothing reaches the library or the settings before the
//! dialog is saved. The numbers of indexed files per format are read from the library on a worker thread when the page
//! is opened (database only, never the drive).

use crate::media::{FormatMask, MediaFormat, MediaType, UNSUPPORTED_FORMATS, catalog};
use crate::ui::models::format_count_with_locale;
use crate::ui::{FormatRowView, SettingsWindow, UnsupportedFormatView};
use parking_lot::Mutex;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::collections::HashMap;
use std::sync::Arc;

/// Reads how many active records each format owns (set by the owner of the library).
pub type FormatCounter = Arc<dyn Fn() -> HashMap<MediaFormat, u64> + Send + Sync>;

type FormatCounts = HashMap<MediaFormat, u64>;

#[derive(Default)]
struct PageState {
    mask: FormatMask,
    filter: String,
    counts: Option<Arc<FormatCounts>>,
    counting: bool,
}

type SharedState = Arc<Mutex<PageState>>;

#[derive(Default)]
pub struct FormatsPage {
    state: SharedState,
    counter: Mutex<Option<FormatCounter>>,
}

impl FormatsPage {
    pub fn set_counter(&self, counter: FormatCounter) {
        *self.counter.lock() = Some(counter);
    }

    /// Registers the callbacks of the page on a freshly created settings window.
    pub fn install(&self, window: &SettingsWindow) {
        let weak = window.as_weak();
        let state = self.state.clone();
        window.on_format_toggled(move |id, enabled| {
            if let (Some(format), Some(w)) = (MediaFormat::from_id(&id), weak.upgrade()) {
                {
                    let mut s = state.lock();
                    s.mask = s.mask.with(format, enabled);
                }
                Self::render(&w, &state);
            }
        });

        let weak = window.as_weak();
        let state = self.state.clone();
        window.on_formats_set_all(move |kind, enabled| {
            let Some(w) = weak.upgrade() else { return };
            let media_type = if kind == 0 { MediaType::Image } else { MediaType::Video };
            {
                // "All on / all off" applies to what the search shows, so a filtered list can be switched at once.
                let mut s = state.lock();
                for entry in catalog(media_type, &s.filter) {
                    s.mask = s.mask.with(entry.spec.format, enabled);
                }
            }
            Self::render(&w, &state);
        });

        let weak = window.as_weak();
        let state = self.state.clone();
        window.on_formats_filter_edited(move |text| {
            if let Some(w) = weak.upgrade() {
                state.lock().filter = text.to_string();
                Self::render(&w, &state);
            }
        });

        let weak = window.as_weak();
        let state = self.state.clone();
        let counter = self.counter.lock().clone();
        window.on_formats_page_shown(move || Self::start_counting(&weak, &state, counter.clone()));
    }

    /// Loads the saved policy into the page (called every time the dialog is shown, so unsaved edits are dropped).
    pub fn populate(&self, window: &SettingsWindow, saved: FormatMask) {
        {
            let mut s = self.state.lock();
            s.mask = saved;
            s.filter.clear();
            s.counts = None;
        }
        window.set_formats_filter_text(SharedString::new());
        Self::render(window, &self.state);
        if window.get_active_category() == 4 {
            Self::start_counting(&window.as_weak(), &self.state, self.counter.lock().clone());
        }
    }

    /// The formats the user has switched off, as stored in the settings.
    pub fn disabled_ids(&self) -> Vec<String> {
        self.state.lock().mask.disabled_ids()
    }

    /// Reads the counts on a worker thread; the page shows them as soon as they are in.
    fn start_counting(weak: &slint::Weak<SettingsWindow>, state: &SharedState, counter: Option<FormatCounter>) {
        let Some(counter) = counter else { return };
        {
            let mut s = state.lock();
            if s.counting || s.counts.is_some() {
                return;
            }
            s.counting = true;
        }
        let weak = weak.clone();
        let worker_state = state.clone();
        let spawned = std::thread::Builder::new().name("format-counts".into()).spawn(move || {
            let counts = Arc::new(counter());
            let state_for_ui = worker_state.clone();
            let delivered = weak.upgrade_in_event_loop(move |w| {
                {
                    let mut s = state_for_ui.lock();
                    s.counts = Some(counts);
                    s.counting = false;
                }
                Self::render(&w, &state_for_ui);
            });
            if delivered.is_err() {
                worker_state.lock().counting = false;
            }
        });
        if spawned.is_err() {
            state.lock().counting = false;
        }
    }

    fn render(window: &SettingsWindow, state: &SharedState) {
        let (mask, filter, counts) = {
            let s = state.lock();
            (s.mask, s.filter.clone(), s.counts.clone())
        };
        let rows = PageRows::build(mask, &filter, counts.as_deref());
        window.set_format_image_rows(ModelRc::new(VecModel::from(rows.images)));
        window.set_format_video_rows(ModelRc::new(VecModel::from(rows.videos)));
        window.set_formats_image_summary(rows.image_summary.into());
        window.set_formats_video_summary(rows.video_summary.into());
        window.set_formats_intro(rows.intro.into());
        window.set_formats_hidden_notice(rows.hidden_notice.into());
        window.set_format_unsupported(ModelRc::new(VecModel::from(
            UNSUPPORTED_FORMATS
                .iter()
                .map(|(title, text)| UnsupportedFormatView {
                    title: (*title).into(),
                    text: (*text).into(),
                })
                .collect::<Vec<_>>(),
        )));
    }
}

/// Everything the page shows, computed from the policy, the search and the counts.
struct PageRows {
    images: Vec<FormatRowView>,
    videos: Vec<FormatRowView>,
    image_summary: String,
    video_summary: String,
    intro: String,
    hidden_notice: String,
}

impl PageRows {
    fn build(mask: FormatMask, filter: &str, counts: Option<&FormatCounts>) -> Self {
        let images = Self::rows(MediaType::Image, mask, filter, counts);
        let videos = Self::rows(MediaType::Video, mask, filter, counts);
        let (image_on, image_total) = Self::totals(MediaType::Image, mask);
        let (video_on, video_total) = Self::totals(MediaType::Video, mask);

        let hidden_files: u64 = counts
            .map(|c| mask.disabled_formats().iter().map(|f| c.get(f).copied().unwrap_or(0)).sum())
            .unwrap_or(0);
        let hidden_notice = match hidden_files {
            0 => String::new(),
            1 => "1 indexed file belongs to a switched-off format. It stays in the library and is hidden everywhere until the format is switched on again.".to_string(),
            n => format!("{} indexed files belong to switched-off formats. They stay in the library and are hidden everywhere until the formats are switched on again.", format_count_with_locale(n)),
        };

        Self {
            images,
            videos,
            image_summary: format!("{image_on} of {image_total} on"),
            video_summary: format!("{video_on} of {video_total} on"),
            intro: format!("{} of {} formats are on.", image_on + video_on, image_total + video_total),
            hidden_notice,
        }
    }

    fn totals(kind: MediaType, mask: FormatMask) -> (usize, usize) {
        let all = catalog(kind, "");
        (all.iter().filter(|e| mask.is_enabled(e.spec.format)).count(), all.len())
    }

    fn rows(kind: MediaType, mask: FormatMask, filter: &str, counts: Option<&FormatCounts>) -> Vec<FormatRowView> {
        catalog(kind, filter)
            .into_iter()
            .map(|entry| {
                let spec = entry.spec;
                let enabled = mask.is_enabled(spec.format);
                let indexed = counts.map(|c| c.get(&spec.format).copied().unwrap_or(0)).unwrap_or(0);
                let count_text = match indexed {
                    0 => String::new(),
                    1 if enabled => "1 file".to_string(),
                    1 => "1 file hidden".to_string(),
                    n if enabled => format!("{} files", format_count_with_locale(n)),
                    n => format!("{} files hidden", format_count_with_locale(n)),
                };
                FormatRowView {
                    id: spec.id.into(),
                    name: spec.name.into(),
                    long_name: spec.long_name.into(),
                    extensions: ModelRc::new(VecModel::from(
                        spec.extensions.iter().map(|e| SharedString::from(*e)).collect::<Vec<_>>(),
                    )),
                    decoder: spec.decoder.label().into(),
                    note: spec.note.into(),
                    count_text: count_text.into(),
                    count_is_hidden: !enabled && indexed > 0,
                    group_title: if entry.starts_group { spec.group.title().into() } else { SharedString::new() },
                    enabled,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(pairs: &[(MediaFormat, u64)]) -> FormatCounts {
        pairs.iter().copied().collect()
    }

    #[test]
    fn disabled_formats_with_indexed_files_are_flagged_and_summed() {
        let mask = FormatMask::ALL_ENABLED.with(MediaFormat::Png, false).with(MediaFormat::Gif, false);
        let c = counts(&[(MediaFormat::Png, 1200), (MediaFormat::Gif, 1), (MediaFormat::Jpeg, 5)]);
        let rows = PageRows::build(mask, "", Some(&c));
        let png = rows.images.iter().find(|r| r.id == "png").unwrap();
        assert!(png.count_is_hidden);
        assert!(png.count_text.ends_with("files hidden"));
        assert!(!png.enabled);
        let gif = rows.images.iter().find(|r| r.id == "gif").unwrap();
        assert_eq!(gif.count_text, "1 file hidden");
        let jpeg = rows.images.iter().find(|r| r.id == "jpeg").unwrap();
        assert!(!jpeg.count_is_hidden);
        assert_eq!(jpeg.count_text, "5 files");
        assert!(rows.hidden_notice.contains("1,201") || rows.hidden_notice.contains("1.201") || rows.hidden_notice.contains("1201"));
    }

    #[test]
    fn nothing_is_flagged_before_the_counts_arrive() {
        let mask = FormatMask::ALL_ENABLED.with(MediaFormat::Png, false);
        let rows = PageRows::build(mask, "", None);
        assert!(rows.hidden_notice.is_empty());
        assert!(rows.images.iter().all(|r| r.count_text.is_empty()));
    }

    #[test]
    fn summaries_count_the_formats_that_are_on() {
        let mask = FormatMask::ALL_ENABLED.with(MediaFormat::Mp4, false);
        let rows = PageRows::build(mask, "", None);
        let video_total = catalog(MediaType::Video, "").len();
        assert_eq!(rows.video_summary, format!("{} of {} on", video_total - 1, video_total));
        assert_eq!(rows.image_summary, format!("{0} of {0} on", catalog(MediaType::Image, "").len()));
    }

    #[test]
    fn the_search_narrows_the_rows_but_not_the_totals() {
        let rows = PageRows::build(FormatMask::ALL_ENABLED, "mkv", None);
        assert!(rows.images.is_empty());
        assert_eq!(rows.videos.len(), 1);
        assert_eq!(rows.videos[0].group_title, "Common");
        assert!(rows.image_summary.starts_with(&catalog(MediaType::Image, "").len().to_string()));
    }
}
