//! What the A-B repeat group of a control bar shows (pure): the five buttons, the strip of saved repeats above the progress
//! bar, the flags of A and B, the label of the repeat under the pointer, the save bubble and the load menu. Everything comes
//! from what the mpv script published (`AbSnapshot`) and the settings; the window only draws it.

use super::ab_menu::{self, MenuPage};
use super::ab_state::{AbBarState, AbPoint, AbPopup, Renaming};
use super::placement::AbPopupSize;
use super::time_text;
use crate::viewer::ab_repeat::colors;
use crate::viewer::ab_repeat::prefs::{AbRepeatPrefs, MenuPaging, RangeColors};
use crate::viewer::ab_repeat::protocol::SavedRange;
use crate::viewer::mpv_engine::ab_repeat::AbSnapshot;
use rustview_core::util::cast::Az;

/// How A or B looks: not set, set while the repeat loops (yellow), set while it does not (blue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointLook {
    Unset,
    Looping,
    Idle,
}

/// How the AB button looks: no valid A-B (disabled, crossed out), a valid A-B that does not loop (blue), looping (yellow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopLook {
    Disabled,
    Ready,
    Looping,
}

impl PointLook {
    pub const fn code(self) -> i32 {
        match self {
            Self::Unset => 0,
            Self::Looping => 1,
            Self::Idle => 2,
        }
    }
}

impl LoopLook {
    pub const fn code(self) -> i32 {
        match self {
            Self::Disabled => 0,
            Self::Ready => 1,
            Self::Looping => 2,
        }
    }
}

/// A saved repeat in the strip: shares of the progress bar and its palette color (`None`: grey).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripSegment {
    pub from: f32,
    pub to: f32,
    pub color: Option<u8>,
}

/// The flag of A or B above the progress bar.
#[derive(Debug, Clone, PartialEq)]
pub struct FlagView {
    pub share: f32,
    pub text: String,
    /// The script set the point by itself (drawn dashed).
    pub auto: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SaveView {
    pub text: String,
    /// The script's default name, shown while the field is empty.
    pub placeholder: String,
    /// The name or the range is saved already.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuRowView {
    pub index: usize,
    pub number: usize,
    pub name: String,
    pub times: String,
    pub length: String,
    pub from: f32,
    pub to: f32,
    pub color: Option<u8>,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MenuView {
    pub query: String,
    pub rows: Vec<MenuRowView>,
    pub page: usize,
    pub pages: usize,
    pub matches: usize,
    pub total: usize,
    pub scroll: bool,
    /// How many rows the menu is high (a scroll list scrolls beyond them).
    pub visible_rows: usize,
    pub renaming: Option<Renaming>,
}

/// Everything the group shows.
#[derive(Debug, Clone, PartialEq)]
pub struct AbBarView {
    pub a: PointLook,
    pub b: PointLook,
    pub loop_look: LoopLook,
    pub a_tip: String,
    pub b_tip: String,
    pub loop_tip: String,
    pub save_enabled: bool,
    pub save_tip: String,
    pub load_count: usize,
    pub load_tip: String,
    pub strip: Vec<StripSegment>,
    /// The current A-B (shares) and whether it loops; only a valid one.
    pub span: Option<(f32, f32, bool)>,
    /// The progress bar outside A-B is darker (only while it loops).
    pub dim: bool,
    pub a_flag: Option<FlagView>,
    pub b_flag: Option<FlagView>,
    /// The saved repeat under the pointer: where (share) and its label.
    pub hover: Option<(f32, String)>,
    pub save: Option<SaveView>,
    pub menu: Option<MenuView>,
}

impl AbBarView {
    /// The open popup, as far as its height is concerned.
    pub fn popup_size(&self) -> Option<AbPopupSize> {
        if let Some(save) = &self.save {
            return Some(AbPopupSize::Save {
                hint: save.hint.is_some(),
            });
        }
        self.menu.as_ref().map(|menu| AbPopupSize::Load {
            rows: menu.visible_rows,
        })
    }
}

/// The group of a bar (`None`: the script is not active in this player, or the bar is the small one of the tab grid).
pub fn view(
    snapshot: &AbSnapshot,
    duration: f64,
    prefs: &AbRepeatPrefs,
    state: &AbBarState,
    clock: (bool, bool),
) -> Option<AbBarView> {
    if !snapshot.active() {
        return None;
    }
    let (hours, ms) = clock;
    let time = |secs: f64| time_text::clock(secs, hours, ms);
    let share = |secs: f64| {
        if duration > 0.0 {
            (secs / duration).clamp(0.0, 1.0).az::<f32>()
        } else {
            0.0
        }
    };
    let script = &snapshot.state;
    let list = &snapshot.ranges.list;
    let palette = match prefs.colors {
        RangeColors::Each => colors::colors(&snapshot.ranges.path, list),
        RangeColors::Grey => Vec::new(),
    };
    let point_look = |set: bool| match (set, script.active) {
        (false, _) => PointLook::Unset,
        (true, true) => PointLook::Looping,
        (true, false) => PointLook::Idle,
    };
    let loop_look = match (script.valid, script.active) {
        (false, _) => LoopLook::Disabled,
        (true, false) => LoopLook::Ready,
        (true, true) => LoopLook::Looping,
    };
    let drag = state.drag();
    let flag = |point: AbPoint, at: Option<f64>, auto: bool| {
        let dragged = drag.filter(|(p, _)| *p == point).map(|(_, s)| s);
        let shown = dragged.or_else(|| at.map(share))?;
        let secs = dragged.map_or_else(|| at.unwrap_or_default(), |s| f64::from(s) * duration);
        let letter = if point == AbPoint::A { "A" } else { "B" };
        Some(FlagView {
            share: shown,
            text: format!("{letter} {}", time(secs)),
            auto: auto && dragged.is_none(),
        })
    };
    let span = match (script.a, script.b) {
        (Some(a), Some(b)) if script.valid => Some((share(a), share(b), script.active)),
        _ => None,
    };
    let hover = state.strip_hover().and_then(|at| {
        let index = segment_at(list, duration, at)?;
        let range = &list[index];
        let mut parts = Vec::new();
        if prefs.hover_name {
            parts.push(range.name.clone());
        }
        if prefs.hover_times {
            parts.push(format!(
                "{}-{} ({})",
                time(range.a_point),
                time(range.b_point),
                time(range.length())
            ));
        }
        (!parts.is_empty()).then(|| (at, parts.join("  ")))
    });
    Some(AbBarView {
        a: point_look(script.a.is_some()),
        b: point_look(script.b.is_some()),
        loop_look,
        a_tip: point_tip("A", "start", script.a.map(time), script.a_auto),
        b_tip: point_tip("B", "end", script.b.map(time), script.b_auto),
        loop_tip: match loop_look {
            LoopLook::Disabled => "A-B repeat: set A and B first".to_owned(),
            LoopLook::Ready => "Play the A-B repeat (hold to clear A and B)".to_owned(),
            LoopLook::Looping => "Stop the A-B repeat (hold to clear A and B)".to_owned(),
        },
        save_enabled: script.valid && !script.busy,
        save_tip: if script.valid {
            "Save this A-B repeat".to_owned()
        } else {
            "Save: set a valid A-B first".to_owned()
        },
        load_count: list.len(),
        load_tip: match list.len() {
            0 => "No saved repeats for this video".to_owned(),
            n => format!("Saved repeats ({n})"),
        },
        strip: list
            .iter()
            .enumerate()
            .map(|(i, range)| StripSegment {
                from: share(range.a_point),
                to: share(range.b_point),
                color: palette.get(i).copied(),
            })
            .collect(),
        span,
        dim: span.is_some_and(|(_, _, looping)| looping),
        a_flag: flag(AbPoint::A, script.a, script.a_auto),
        b_flag: flag(AbPoint::B, script.b, script.b_auto),
        hover,
        save: match &state.popup {
            Some(AbPopup::Save { text }) => Some(save_view(text, list, script.a.zip(script.b))),
            _ => None,
        },
        menu: match &state.popup {
            Some(AbPopup::Load { query, page, renaming }) => {
                let current = script.a.zip(script.b).filter(|_| script.valid);
                let page = ab_menu::page(list, &palette, query, current, prefs.paging, prefs.page_size, *page);
                Some(menu_view(&page, query, renaming.clone(), prefs, &time, &share))
            }
            _ => None,
        },
    })
}

fn point_tip(letter: &str, role: &str, at: Option<String>, auto: bool) -> String {
    match at {
        None => format!("Set {letter} here (the {role} of the repeat)"),
        Some(at) if auto => format!("{letter} at {at}, set by the script; click to set {letter} here"),
        Some(at) => format!("{letter} at {at}; click to set {letter} here"),
    }
}

fn save_view(text: &str, list: &[SavedRange], points: Option<(f64, f64)>) -> SaveView {
    let name = text.trim();
    let same_name = (!name.is_empty())
        .then(|| list.iter().find(|r| r.name.eq_ignore_ascii_case(name)))
        .flatten();
    let same_range = points.and_then(|(a, b)| list.iter().find(|r| r.is_at(a, b)));
    let hint = match (same_name, same_range) {
        (_, Some(range)) => Some(format!("This range is saved already as \"{}\"", range.name)),
        (Some(range), None) => Some(format!("A repeat named \"{}\" exists already", range.name)),
        (None, None) => None,
    };
    SaveView {
        text: text.to_owned(),
        placeholder: format!("Range {}", list.len() + 1),
        hint,
    }
}

fn menu_view(
    page: &MenuPage,
    query: &str,
    renaming: Option<Renaming>,
    prefs: &AbRepeatPrefs,
    time: &dyn Fn(f64) -> String,
    share: &dyn Fn(f64) -> f32,
) -> MenuView {
    let scroll = prefs.paging == MenuPaging::Scroll;
    MenuView {
        query: query.to_owned(),
        rows: page
            .rows
            .iter()
            .map(|row| MenuRowView {
                index: row.index,
                number: row.number,
                name: row.name.clone(),
                times: format!("{} - {}", time(row.from), time(row.to)),
                length: time((row.to - row.from).max(0.0)),
                from: share(row.from),
                to: share(row.to),
                color: row.color,
                current: row.current,
            })
            .collect(),
        page: page.page,
        pages: page.pages,
        matches: page.matches,
        total: page.total,
        scroll,
        visible_rows: if scroll {
            prefs.page_size.min(page.matches.max(1))
        } else {
            page.rows.len().max(1)
        },
        renaming,
    }
}

/// The saved repeat at `share` of the progress bar: the shortest one there (the most specific), the newest of equal ones.
pub fn segment_at(list: &[SavedRange], duration: f64, share: f32) -> Option<usize> {
    if duration <= 0.0 {
        return None;
    }
    let at = f64::from(share) * duration;
    list.iter()
        .enumerate()
        .filter(|(_, r)| r.a_point <= at && at <= r.b_point)
        .min_by(|(i, a), (j, b)| a.length().total_cmp(&b.length()).then(j.cmp(i)))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::ab_repeat::protocol::{RangeList, ScriptState};
    use crate::viewer::ab_repeat::support::Support;
    use std::sync::Arc;

    fn range(name: &str, a: f64, b: f64) -> SavedRange {
        SavedRange {
            name: name.into(),
            a_point: a,
            b_point: b,
        }
    }

    fn snapshot(state: ScriptState) -> AbSnapshot {
        AbSnapshot {
            support: Support::Compatible {
                version: "2.0".into(),
                client: "ab_repeat".into(),
            },
            state,
            ranges: Arc::new(RangeList {
                path: "/test/ab_view.mp4".into(),
                list: vec![
                    range("Intro", 0.0, 10.0),
                    range("Strophe", 30.0, 55.0),
                    range("Refrain", 48.0, 70.0),
                    range("Solo", 52.0, 58.0),
                ],
                ..RangeList::default()
            }),
            watching: true,
        }
    }

    #[test]
    fn nothing_without_the_script() {
        let mut snap = snapshot(ScriptState::default());
        snap.watching = false;
        assert!(
            view(
                &snap,
                128.0,
                &AbRepeatPrefs::DEFAULT,
                &AbBarState::default(),
                (false, false)
            )
            .is_none()
        );
    }

    #[test]
    fn a_looping_repeat_is_yellow_and_dims_the_rest() {
        let state = ScriptState {
            a: Some(48.0),
            b: Some(70.0),
            valid: true,
            loop_on: true,
            active: true,
            ..ScriptState::default()
        };
        let v = view(
            &snapshot(state),
            128.0,
            &AbRepeatPrefs::DEFAULT,
            &AbBarState::default(),
            (false, false),
        )
        .unwrap();
        assert_eq!(
            (v.a, v.b, v.loop_look),
            (PointLook::Looping, PointLook::Looping, LoopLook::Looping)
        );
        assert!(v.dim && v.save_enabled);
        assert_eq!(v.a_flag.as_ref().unwrap().text, "A 0:48");
        assert_eq!(v.strip.len(), 4);
        assert_eq!(v.load_count, 4);
        let (from, to, looping) = v.span.unwrap();
        assert!((from - 0.375).abs() < 1e-6 && (to - 70.0 / 128.0).abs() < 1e-6 && looping);
    }

    #[test]
    fn a_point_the_script_set_is_marked_and_an_invalid_range_cannot_loop() {
        let only_a = ScriptState {
            a: Some(88.0),
            b: Some(127.96),
            b_auto: true,
            valid: true,
            loop_on: true,
            active: true,
            ..ScriptState::default()
        };
        let v = view(
            &snapshot(only_a),
            128.0,
            &AbRepeatPrefs::DEFAULT,
            &AbBarState::default(),
            (false, false),
        )
        .unwrap();
        assert!(v.b_flag.unwrap().auto && !v.a_flag.unwrap().auto);
        let reversed = ScriptState {
            a: Some(30.0),
            b: Some(20.0),
            valid: false,
            loop_on: true,
            ..ScriptState::default()
        };
        let v = view(
            &snapshot(reversed),
            128.0,
            &AbRepeatPrefs::DEFAULT,
            &AbBarState::default(),
            (false, false),
        )
        .unwrap();
        assert_eq!(v.loop_look, LoopLook::Disabled);
        assert!(v.span.is_none() && !v.dim && !v.save_enabled);
        assert_eq!(v.a, PointLook::Idle);
    }

    #[test]
    fn the_shortest_repeat_under_the_pointer_wins() {
        let snap = snapshot(ScriptState::default());
        let list = &snap.ranges.list;
        assert_eq!(
            segment_at(list, 128.0, 53.0 / 128.0),
            Some(3),
            "Solo lies in Strophe and Refrain"
        );
        assert_eq!(
            segment_at(list, 128.0, 50.0 / 128.0),
            Some(2),
            "Refrain is shorter than Strophe"
        );
        assert_eq!(segment_at(list, 128.0, 20.0 / 128.0), None);
        let mut state = AbBarState::default();
        state.set_strip_hover(Some(53.0 / 128.0));
        let prefs = AbRepeatPrefs {
            hover_times: true,
            ..AbRepeatPrefs::DEFAULT
        };
        let v = view(&snap, 128.0, &prefs, &state, (false, false)).unwrap();
        assert_eq!(v.hover.unwrap().1, "Solo  0:52-0:58 (0:06)");
    }

    #[test]
    fn the_save_bubble_names_a_double() {
        let state = ScriptState {
            a: Some(48.0),
            b: Some(70.0),
            valid: true,
            ..ScriptState::default()
        };
        let mut bar = AbBarState::default();
        bar.popup = Some(AbPopup::Save { text: String::new() });
        let v = view(&snapshot(state), 128.0, &AbRepeatPrefs::DEFAULT, &bar, (false, false)).unwrap();
        let save = v.save.unwrap();
        assert_eq!(save.placeholder, "Range 5");
        assert_eq!(save.hint.as_deref(), Some("This range is saved already as \"Refrain\""));
        let fresh = ScriptState {
            a: Some(1.0),
            b: Some(2.0),
            ..state
        };
        bar.popup = Some(AbPopup::Save { text: "intro".into() });
        let v = view(&snapshot(fresh), 128.0, &AbRepeatPrefs::DEFAULT, &bar, (false, false)).unwrap();
        assert_eq!(
            v.save.unwrap().hint.as_deref(),
            Some("A repeat named \"Intro\" exists already")
        );
    }

    #[test]
    fn a_dragged_flag_shows_where_it_is() {
        let state = ScriptState {
            a: Some(48.0),
            b: Some(70.0),
            valid: true,
            ..ScriptState::default()
        };
        let mut bar = AbBarState::default();
        bar.drag_to(AbPoint::B, 0.75);
        let v = view(&snapshot(state), 128.0, &AbRepeatPrefs::DEFAULT, &bar, (false, false)).unwrap();
        let b = v.b_flag.unwrap();
        assert!((b.share - 0.75).abs() < 1e-6);
        assert_eq!(b.text, "B 1:36");
    }
}
