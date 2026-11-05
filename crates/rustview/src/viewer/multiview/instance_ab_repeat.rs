//! The A-B repeat group of a video's control bar and the keys of the area "MPV `ab_repeat.lua`" (`viewer::ab_repeat`). Every
//! change of the repeat is a command of the mpv script, so it behaves exactly as in mpv; the bar shows what the script
//! publishes. Only the popups (save bubble, load menu), the strip under the pointer and a dragged flag live here.

use super::command::AbKey;
use super::hint::HintKind;
use super::instance::MultiviewInstance;
use super::instance_commands::CommandResult;
use super::route::UiAction;
use super::video_bar::ab_menu;
use super::video_bar::ab_state::{AbPoint, AbPopup, Renaming};
use super::video_bar::ab_view::segment_at;
use super::video_bar::view::{AbButton, AbInput, DragPhase};
use crate::viewer::ab_repeat::prefs as ab_prefs;
use crate::viewer::ab_repeat::protocol::{Command, RangeRef};
use crate::viewer::mpv_engine::ab_repeat::{AbChange, AbSnapshot};
use crate::viewer::multiview::follow::TileKey;
use rustview_core::settings::schema::MULTIVIEW_VIDEO_BAR_PULSE_MS;
use std::time::{Duration, Instant};

/// What an input asks of the script (in this order), and whether the video plays afterwards.
#[derive(Default)]
struct Outcome {
    commands: Vec<Command>,
    play: bool,
}

impl MultiviewInstance {
    /// Whether the video the keys act on (the lone one, or the selected of several) runs the script: the keys of the area
    /// "MPV `ab_repeat.lua`" apply only then.
    pub fn ab_repeat_active(&self) -> bool {
        let Some(key) = self.focus_target() else {
            return false;
        };
        self.tiles
            .iter()
            .find(|t| t.key == key)
            .and_then(|t| t.video.as_ref())
            .is_some_and(|v| v.player.ab_repeat().active())
    }

    /// The user did something in the A-B group of the bar of tile `index`. Returns whether the view has to be refreshed.
    pub(super) fn ab_input(&mut self, index: usize, input: AbInput) -> bool {
        let prefs = ab_prefs::current();
        let media_id = self.tiles[index].media_id;
        let Some(video) = self.tiles[index].video.as_mut() else {
            return false;
        };
        let snapshot = video.player.ab_repeat();
        if !snapshot.active() {
            video.bar.ab.release();
            return true;
        }
        let duration = video.player.playback().duration;
        let state = &mut video.bar.ab;
        let mut outcome = Outcome::default();
        match input {
            AbInput::Button(button) => outcome.commands = button_commands(button, &snapshot, state),
            AbInput::SaveText(text) => {
                if let Some(AbPopup::Save { text: shown }) = state.popup.as_mut() {
                    *shown = text;
                }
            }
            AbInput::SaveCommit(text) => {
                state.popup = None;
                if snapshot.state.valid {
                    outcome.commands.push(Command::Save(text));
                }
            }
            AbInput::ClosePopup => state.popup = None,
            AbInput::StripHover(share) => state.set_strip_hover(share),
            AbInput::StripClick(share) => {
                if prefs.click_loads
                    && let Some(range) = segment_at(&snapshot.ranges.list, duration, share)
                        .and_then(|i| RangeRef::of(&snapshot.ranges.list, i))
                {
                    outcome = Outcome {
                        commands: vec![Command::Load(range)],
                        play: true,
                    };
                }
            }
            AbInput::Flag { point, share, phase } => {
                if !prefs.drag_flags {
                    return false;
                }
                if phase == DragPhase::Release {
                    let secs = state.drop_flag().map_or(f64::from(share), |(_, s)| f64::from(s)) * duration;
                    outcome.commands.push(match point {
                        AbPoint::A => Command::SetA(Some(secs)),
                        AbPoint::B => Command::SetB(Some(secs)),
                    });
                } else {
                    state.drag_to(point, share);
                }
            }
            menu => outcome = menu_input(menu, &snapshot, state),
        }
        for command in &outcome.commands {
            tracing::info!(media_id, ?command, "[Multiview: AbRepeat] Command from the control bar");
            if let Err(e) = video.player.ab_command(command) {
                tracing::warn!(media_id, error = %e, "[Multiview: AbRepeat] The script did not take it");
            }
        }
        if outcome.play
            && let Err(e) = video.player.play()
        {
            tracing::warn!(media_id, error = %e, "[Multiview: AbRepeat] The video did not start");
        }
        true
    }

    /// A key of the area "MPV `ab_repeat.lua`" on tile `index` (the selected video).
    pub(super) fn ab_key(&mut self, index: usize, key: AbKey) -> CommandResult {
        let media_id = self.tiles[index].media_id;
        let Some(video) = self.tiles[index].video.as_mut() else {
            return Err(HintKind::NotAVideo);
        };
        let snapshot = video.player.ab_repeat();
        if !snapshot.active() {
            return Err(HintKind::AbRepeatOff);
        }
        let command = match key {
            AbKey::SetA => Command::SetA(None),
            AbKey::SetB => Command::SetB(None),
            AbKey::Clear => Command::Clear,
            AbKey::JumpToA => Command::JumpToA,
            AbKey::NextRange | AbKey::PreviousRange if snapshot.ranges.list.is_empty() => {
                return Err(HintKind::AbNoSavedRepeats);
            }
            AbKey::NextRange => Command::NextRange,
            AbKey::PreviousRange => Command::PreviousRange,
            AbKey::Save => {
                if !snapshot.state.valid {
                    return Err(HintKind::AbNoRange);
                }
                video.bar.ab.popup = Some(AbPopup::Save { text: String::new() });
                video.bar.popup = None;
                return Ok(UiAction::None);
            }
            AbKey::LoadMenu => {
                if snapshot.ranges.list.is_empty() {
                    return Err(HintKind::AbNoSavedRepeats);
                }
                video.bar.ab.popup = Some(load_popup(&snapshot));
                video.bar.popup = None;
                return Ok(UiAction::None);
            }
        };
        tracing::info!(media_id, ?key, "[Multiview: AbRepeat] Key");
        if let Err(e) = video.player.ab_command(&command) {
            tracing::warn!(media_id, error = %e, "[Multiview: AbRepeat] The script did not take the key");
        }
        video
            .bar
            .reveal
            .pulse(Instant::now() + Duration::from_millis(MULTIVIEW_VIDEO_BAR_PULSE_MS));
        Ok(UiAction::None)
    }

    /// The script in the player of tile `key` published a change: a popup that has nothing left to show closes.
    pub fn on_ab_repeat_changed(&mut self, key: TileKey, change: AbChange) {
        let Some(index) = self.tile_index(key) else {
            return;
        };
        let Some(video) = self.tiles[index].video.as_mut() else {
            return;
        };
        let snapshot = video.player.ab_repeat();
        let state = &mut video.bar.ab;
        if !snapshot.active() {
            state.release();
        } else if change == AbChange::Ranges
            && let Some(AbPopup::Load { renaming, .. }) = state.popup.as_mut()
            && renaming.as_ref().is_some_and(|r| r.index >= snapshot.ranges.list.len())
        {
            *renaming = None;
        }
        tracing::debug!(tile = key, ?change, "[Multiview: AbRepeat] The script changed");
        self.bump();
    }
}

/// A click on a button of the group: commands, or a popup that opens or closes.
fn button_commands(
    button: AbButton,
    snapshot: &AbSnapshot,
    state: &mut super::video_bar::ab_state::AbBarState,
) -> Vec<Command> {
    match button {
        AbButton::SetA => vec![Command::SetA(None)],
        AbButton::SetB => vec![Command::SetB(None)],
        AbButton::Loop => Vec::from_iter(snapshot.state.valid.then_some(Command::Loop(!snapshot.state.loop_on))),
        AbButton::Clear => vec![Command::Clear],
        AbButton::ClearA => Command::clear_point(true, &snapshot.state),
        AbButton::ClearB => Command::clear_point(false, &snapshot.state),
        AbButton::Save => {
            state.popup = match state.popup {
                Some(AbPopup::Save { .. }) => None,
                _ => snapshot.state.valid.then(|| AbPopup::Save { text: String::new() }),
            };
            Vec::new()
        }
        AbButton::Load => {
            state.popup = match state.popup {
                Some(AbPopup::Load { .. }) => None,
                _ => (!snapshot.ranges.list.is_empty()).then(|| load_popup(snapshot)),
            };
            Vec::new()
        }
    }
}

/// The load menu as it opens: the page it showed last for this file.
fn load_popup(snapshot: &AbSnapshot) -> AbPopup {
    AbPopup::Load {
        query: String::new(),
        page: ab_menu::remembered_page(&snapshot.ranges.path),
        renaming: None,
    }
}

/// What the user did in the load menu.
fn menu_input(input: AbInput, snapshot: &AbSnapshot, state: &mut super::video_bar::ab_state::AbBarState) -> Outcome {
    let list = &snapshot.ranges.list;
    let Some(AbPopup::Load { query, page, renaming }) = state.popup.as_mut() else {
        return Outcome::default();
    };
    let mut outcome = Outcome::default();
    match input {
        AbInput::MenuQuery(text) => {
            *query = text;
            *page = 0;
        }
        AbInput::MenuPage(delta) => {
            *page = if delta < 0 {
                page.saturating_sub(1)
            } else {
                page.saturating_add(1)
            };
            ab_menu::remember_page(&snapshot.ranges.path, *page);
        }
        AbInput::MenuPick(index) => {
            if let Some(range) = RangeRef::of(list, index) {
                outcome = Outcome {
                    commands: vec![Command::Load(range)],
                    play: true,
                };
                state.popup = None;
            }
        }
        AbInput::MenuDelete(index) => outcome.commands.extend(RangeRef::of(list, index).map(Command::Delete)),
        AbInput::MenuDeleteAll => outcome.commands.push(Command::DeleteAll),
        // The script reads its file again and publishes the list (nothing is read before this click).
        AbInput::MenuRefresh => outcome.commands.push(Command::Reload),
        AbInput::MenuRename(index) => {
            *renaming = list.get(index).map(|range| Renaming {
                index,
                text: range.name.clone(),
            });
        }
        AbInput::MenuRenameText(text) => {
            if let Some(renaming) = renaming.as_mut() {
                renaming.text = text;
            }
        }
        AbInput::MenuRenameCommit(text) => {
            if let Some(done) = renaming.take() {
                let name = text.trim();
                if !name.is_empty() {
                    outcome
                        .commands
                        .extend(RangeRef::of(list, done.index).map(|r| Command::Rename(r, name.to_owned())));
                }
            }
        }
        AbInput::MenuRenameCancel => *renaming = None,
        _ => {}
    }
    outcome
}
