//! The keys the media action bar names in its tooltips. The keymap lives in the UI layer (`ui::hotkeys`), which hands the
//! current keys of the edit actions over as text whenever it changes; the viewer never depends on the UI.

use super::route::HostMode;
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
