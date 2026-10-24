//! Settings > Hotkeys: the settings window's copy of the keymap and the page that edits it (`ui/settings_page_hotkeys.slint`,
//! pure logic in `page`). Save writes the keys that differ from the defaults into `settings.json` (`hotkeys`); Cancel drops
//! the copy. The filter, "Only changed" and the collapsed sections are kept at once (`view.hotkeys_page`), whatever the
//! window's buttons do, like every other state of the UI.

mod page;

use crate::ui::keymap::{Action, Area, KeyPress, Keymap, table_from_json, table_to_json};
use crate::ui::{HotkeyChipView, HotkeyRowView, HotkeySectionView, HotkeysUi, SettingsWindow};
use page::{HotkeysPage, Recorded, Row};
use rustview_core::settings::registry::SettingsManager;
use rustview_core::settings::schema::{AppSettings, HotkeysPageState};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

/// The state of the page while the settings window exists (UI thread only).
pub struct HotkeysPageController {
    keymap: RefCell<Keymap>,
    page: RefCell<HotkeysPage>,
    manager: Arc<SettingsManager>,
    window: RefCell<Option<slint::Weak<SettingsWindow>>>,
}

impl HotkeysPageController {
    pub fn new(manager: Arc<SettingsManager>) -> Rc<Self> {
        Rc::new(Self {
            keymap: RefCell::new(Keymap::default()),
            page: RefCell::new(HotkeysPage::default()),
            manager,
            window: RefCell::new(None),
        })
    }

    /// Connects the page's callbacks (once, when the settings window is made).
    pub fn install(self: &Rc<Self>, window: &SettingsWindow) {
        *self.window.borrow_mut() = Some(window.as_weak());
        let ui = window.global::<HotkeysUi>();
        let this = Rc::downgrade(self);
        let with = move |f: &dyn Fn(&Self)| {
            if let Some(this) = this.upgrade() {
                f(&this);
            }
        };
        let w = with.clone();
        ui.on_filter_edited(move |text| {
            w(&|c| {
                c.page.borrow_mut().filter = text.to_string();
                c.remember_page_state();
                c.refresh();
            });
        });
        let w = with.clone();
        ui.on_only_changed_toggled(move |on| {
            w(&|c| {
                c.page.borrow_mut().only_changed = on;
                c.remember_page_state();
                c.refresh();
            });
        });
        let w = with.clone();
        ui.on_section_toggled(move |id| {
            w(&|c| {
                if let Some(area) = Area::ALL.into_iter().find(|a| a.id() == id.as_str()) {
                    c.page.borrow_mut().toggle_section(area);
                    c.remember_page_state();
                    c.refresh();
                }
            });
        });
        let w = with.clone();
        ui.on_record(move |id| {
            w(&|c| {
                if let Some(action) = Action::from_id(&id) {
                    c.page.borrow_mut().start(action);
                    c.refresh();
                }
            });
        });
        let w = with.clone();
        ui.on_cancel_recording(move || {
            w(&|c| {
                c.page.borrow_mut().cancel();
                c.refresh();
            });
        });
        let this = Rc::downgrade(self);
        ui.on_recorded(move |text, control, alt, shift, meta| {
            let Some(c) = this.upgrade() else {
                return false;
            };
            let press = KeyPress {
                text: text.as_str(),
                control,
                alt,
                shift,
                meta,
                repeat: false,
            };
            let result = c.page.borrow_mut().press(&mut c.keymap.borrow_mut(), &press);
            tracing::debug!(?result, "[Settings: Hotkeys] Key pressed while recording");
            c.refresh();
            result != Recorded::Waiting
        });
        let w = with.clone();
        ui.on_unbind(move |id, text| {
            w(&|c| {
                if let Some(action) = Action::from_id(&id) {
                    c.page.borrow_mut().unbind(&mut c.keymap.borrow_mut(), action, &text);
                    c.refresh();
                }
            });
        });
        let w = with.clone();
        ui.on_reset(move |id| {
            w(&|c| {
                if let Some(action) = Action::from_id(&id) {
                    c.page.borrow_mut().reset(&mut c.keymap.borrow_mut(), action);
                    c.refresh();
                }
            });
        });
        ui.on_reset_all(move || {
            with(&|c| {
                c.page.borrow_mut().reset_all(&mut c.keymap.borrow_mut());
                tracing::info!("[Settings: Hotkeys] Every key back to its default (until Save or Cancel)");
                c.refresh();
            });
        });
    }

    /// Fills the page from the settings when the window opens: the stored keys and the page's own state.
    pub fn populate(&self, settings: &AppSettings) {
        let (keymap, _) = Keymap::from_table(&table_from_json(&settings.hotkeys));
        *self.keymap.borrow_mut() = keymap;
        let state = &settings.view.hotkeys_page;
        *self.page.borrow_mut() =
            HotkeysPage::new(state.filter.clone(), state.only_changed, state.collapsed_areas.clone());
        if let Some(window) = self.window() {
            let ui = window.global::<HotkeysUi>();
            ui.set_filter(state.filter.clone().into());
            ui.set_only_changed(state.only_changed);
        }
        self.refresh();
    }

    /// Writes the keys that differ from the defaults into `current` (Save).
    pub fn save(&self, current: &mut AppSettings) {
        self.page.borrow_mut().cancel();
        current.hotkeys = table_to_json(&page::table(&self.keymap.borrow()));
    }

    fn window(&self) -> Option<SettingsWindow> {
        self.window.borrow().as_ref().and_then(slint::Weak::upgrade)
    }

    /// Keeps the filter, "Only changed" and the collapsed sections (without waiting for Save).
    fn remember_page_state(&self) {
        let page = self.page.borrow();
        let state = HotkeysPageState {
            filter: page.filter.clone(),
            only_changed: page.only_changed,
            collapsed_areas: page.collapsed.clone(),
        };
        let mut settings = self.manager.get();
        if settings.view.hotkeys_page != state {
            settings.view.hotkeys_page = state;
            self.manager.save_quiet_async(&settings);
        }
    }

    /// Shows the page as it is now.
    fn refresh(&self) {
        let Some(window) = self.window() else {
            return;
        };
        let page = self.page.borrow();
        let keymap = self.keymap.borrow();
        let sections: Vec<HotkeySectionView> = page
            .sections(&keymap)
            .into_iter()
            .map(|section| HotkeySectionView {
                id: section.area.id().into(),
                title: section.area.title().into(),
                description: section.area.description().into(),
                open: section.open,
                rows: ModelRc::new(VecModel::from(
                    section.rows.iter().map(|row| row_view(row, &page)).collect::<Vec<_>>(),
                )),
            })
            .collect();
        let ui = window.global::<HotkeysUi>();
        ui.set_sections(ModelRc::new(VecModel::from(sections)));
        ui.set_recording(page.recording.map_or("", Action::id).into());
        ui.set_narrowing(!page.filter.trim().is_empty() || page.only_changed);
    }
}

fn row_view(row: &Row, page: &HotkeysPage) -> HotkeyRowView {
    let note = page.note.as_ref().filter(|n| n.action == row.action);
    HotkeyRowView {
        id: row.action.id().into(),
        title: row.action.title().into(),
        keys: ModelRc::new(VecModel::from(
            row.keys
                .iter()
                .map(|(text, label)| HotkeyChipView {
                    text: text.into(),
                    label: label.into(),
                })
                .collect::<Vec<_>>(),
        )),
        changed: row.changed,
        fixed: row.action.is_fixed(),
        fixed_reason: fixed_reason(row.action).into(),
        defaults: row.defaults.clone().into(),
        note: note.map_or_else(SharedString::default, |n| n.text.clone().into()),
        note_error: note.is_some_and(|n| n.error),
    }
}

/// Why a fixed row cannot be changed.
fn fixed_reason(action: Action) -> &'static str {
    match action {
        Action::BrowseCursor | Action::BrowsePage | Action::BrowseFirstLast => {
            "The cursor keys work like in every list: they move the cursor, and with Shift they extend the selection."
        }
        Action::BrowseTypeAhead => {
            "Typing a name jumps to it (type-ahead find, Settings > Appearance & Grid). That is why Browse keeps every key that \
             types a character."
        }
        Action::BrowseEscape | Action::TabEscape | Action::FullscreenEscape => {
            "Esc always cancels or closes. It also cancels the recording of a key on this page."
        }
        _ => "",
    }
}
