//! The keys the media action bar names in its tooltips. The keymap lives in the UI layer (`ui::hotkeys`), which hands the
//! current keys of the edit actions over as text whenever it changes; the viewer never depends on the UI.

use super::route::HostMode;
use super::video_filter::menu::FilterKeys;
use parking_lot::RwLock;

/// The first key of each edit action of one area ("" = no key).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditKeys {
    pub undo: String,
    pub redo: String,
    pub save: String,
    pub apply_crop: String,
}

/// (the tab view's keys, the fullscreen's keys)
static LABELS: RwLock<(EditKeys, EditKeys)> = RwLock::new((
    EditKeys {
        undo: String::new(),
        redo: String::new(),
        save: String::new(),
        apply_crop: String::new(),
    },
    EditKeys {
        undo: String::new(),
        redo: String::new(),
        save: String::new(),
        apply_crop: String::new(),
    },
));

/// The first key of the A-B repeat keys the control bar names in its tooltips ("" = no key).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbKeys {
    pub set_a: String,
    pub set_b: String,
    pub clear: String,
}

static AB_LABELS: RwLock<AbKeys> = RwLock::new(AbKeys {
    set_a: String::new(),
    set_b: String::new(),
    clear: String::new(),
});

pub fn set_ab(keys: AbKeys) {
    *AB_LABELS.write() = keys;
}

pub fn ab() -> AbKeys {
    AB_LABELS.read().clone()
}

static FILTER_LABELS: RwLock<Option<FilterKeys>> = RwLock::new(None);

/// The keys of the video filter actions (the filter menus name them).
pub fn set_filters(keys: FilterKeys) {
    *FILTER_LABELS.write() = Some(keys);
}

pub fn filters() -> FilterKeys {
    FILTER_LABELS.read().clone().unwrap_or_default()
}

pub fn set(tab: EditKeys, fullscreen: EditKeys) {
    *LABELS.write() = (tab, fullscreen);
}

/// The keys of the area a host belongs to.
pub fn of(host: HostMode) -> EditKeys {
    let labels = LABELS.read();
    match host {
        HostMode::Fullscreen => labels.1.clone(),
        HostMode::TabView | HostMode::TabGrid | HostMode::Hidden => labels.0.clone(),
    }
}

/// `text (key)`, or `text` alone without a key.
pub fn with_key(text: &str, key: &str) -> String {
    if key.is_empty() {
        text.to_owned()
    } else {
        format!("{text} ({key})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_key_leaves_the_text_alone() {
        assert_eq!(
            with_key("Undo the last change", "Ctrl+Z"),
            "Undo the last change (Ctrl+Z)"
        );
        assert_eq!(with_key("Undo the last change", ""), "Undo the last change");
    }
}
