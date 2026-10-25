//! Settings > Viewer > Videos, card "mpv script `ab_repeat.lua`" (`ui/settings_card_ab_repeat.slint`): its values, the status
//! of the script (what the players or the last check found) and "Check again", which starts a fresh, invisible mpv with the
//! user's configuration on a worker thread (a replaced script is then known without restarting `RustView`).

use crate::ui::SettingsWindow;
use crate::viewer::ab_repeat::support::{self, Support};
use rustview_core::settings::schema::{AbRepeatSettings, MAX_AB_REPEAT_PAGE_SIZE, MIN_AB_REPEAT_PAGE_SIZE};
use rustview_core::util::cast::SaturatingAs;
use slint::ComponentHandle;

pub(super) fn populate(window: &SettingsWindow, settings: &AbRepeatSettings) {
    window.set_ab_enabled(settings.enabled);
    window.set_ab_osd_messages(settings.osd_messages);
    window.set_ab_colors_idx(i32::from(settings.colors == "grey"));
    window.set_ab_click_loads(settings.click_loads);
    window.set_ab_hover_name(settings.hover_name);
    window.set_ab_hover_times(settings.hover_times);
    window.set_ab_drag_flags(settings.drag_flags);
    window.set_ab_menu_idx(i32::from(settings.menu == "scroll"));
    window.set_ab_page_size(settings.page_size.saturating_as::<i32>());
    window.set_ab_checking(false);
    show_status(window, &support::last());
}

pub(super) fn save(window: &SettingsWindow, settings: &mut AbRepeatSettings) {
    settings.enabled = window.get_ab_enabled();
    settings.osd_messages = window.get_ab_osd_messages();
    if window.get_ab_colors_idx() == 1 {
        "grey"
    } else {
        "each"
    }
    .clone_into(&mut settings.colors);
    settings.click_loads = window.get_ab_click_loads();
    settings.hover_name = window.get_ab_hover_name();
    settings.hover_times = window.get_ab_hover_times();
    settings.drag_flags = window.get_ab_drag_flags();
    if window.get_ab_menu_idx() == 1 {
        "scroll"
    } else {
        "pages"
    }
    .clone_into(&mut settings.menu);
    settings.page_size = window
        .get_ab_page_size()
        .clamp(
            MIN_AB_REPEAT_PAGE_SIZE.saturating_as::<i32>(),
            MAX_AB_REPEAT_PAGE_SIZE.saturating_as::<i32>(),
        )
        .saturating_as::<u32>();
}

pub(super) fn install(window: &SettingsWindow) {
    let weak = window.as_weak();
    window.on_ab_check_again(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        window.set_ab_checking(true);
        tracing::info!("[Settings: AbRepeat] Checking the mpv script again");
        let weak = window.as_weak();
        let spawned = std::thread::Builder::new()
            .name("ab-repeat-check".into())
            .spawn(move || {
                let found = crate::viewer::mpv_engine::ab_repeat::probe();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(window) = weak.upgrade() {
                        window.set_ab_checking(false);
                        show_status(&window, &found);
                    }
                });
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[Settings: AbRepeat] The check could not start");
            window.set_ab_checking(false);
        }
    });
}

fn show_status(window: &SettingsWindow, found: &Support) {
    let level = match found {
        Support::Unknown => 0,
        Support::Compatible { .. } => 1,
        Support::Incompatible(_) => 2,
        Support::NotFound => 3,
    };
    window.set_ab_status_level(level);
    window.set_ab_status_text(found.describe().into());
    window.set_ab_status_tooltip(
        match found {
            Support::Incompatible(_) => "RustView needs ab_repeat.lua 2.0 or newer with its interface on (api=yes, the default). Replace the script in mpv's scripts folder, then click Check again: videos opened from then on use it.",
            Support::NotFound => "mpv does not load an ab_repeat.lua from its scripts folder (~/.config/mpv/scripts). The A-B group stays hidden.",
            Support::Compatible { .. } => "Videos whose player runs the script show the A-B group in their control bar.",
            Support::Unknown => "Nothing was checked yet: every video's player looks for the script when it starts.",
        }
        .into(),
    );
}
