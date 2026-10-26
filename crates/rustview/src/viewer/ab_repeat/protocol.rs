//! What `ab_repeat.lua` publishes and which commands it takes (interface version 1, described at the top of the script).

use serde::Deserialize;

/// The interface version this `RustView` speaks; a script with another one is not compatible.
pub const API_VERSION: u32 = 1;

/// The paths of mpv's `user-data` property the script publishes.
pub const INFO_PATH: &str = "user-data/ab_repeat/info";
pub const STATE_PATH: &str = "user-data/ab_repeat/state";
pub const RANGES_PATH: &str = "user-data/ab_repeat/ranges";
pub const PONG_PATH: &str = "user-data/ab_repeat/pong";

/// Who is running: `user-data/ab_repeat/info`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ScriptInfo {
    pub name: String,
    pub version: String,
    pub api: u32,
    /// The client name to address with `script-message-to` (it follows the script's file name).
    pub client: String,
    /// The JSON file the ranges are saved in.
    pub db: String,
    pub config: ScriptConfig,
}

/// The settings of the script that change what `RustView` shows.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct ScriptConfig {
    pub entries_per_page: u32,
    pub show_osd_messages: bool,
    pub osd_messages: bool,
    pub auto_set_a_point: bool,
    pub auto_set_b_point: bool,
}

impl Default for ScriptConfig {
    fn default() -> Self {
        Self {
            entries_per_page: 10,
            show_osd_messages: true,
            osd_messages: true,
            auto_set_a_point: true,
            auto_set_b_point: true,
        }
    }
}

/// The current points: `user-data/ab_repeat/state`.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct ScriptState {
    pub a: Option<f64>,
    pub b: Option<f64>,
    /// The script set this point by itself (A = 0 when only B was set, B = the end when only A was set).
    pub a_auto: bool,
    pub b_auto: bool,
    /// Both points are set and B is after A.
    pub valid: bool,
    /// The loop is switched on (a valid A-B can be kept while its loop is off).
    #[serde(rename = "loop")]
    pub loop_on: bool,
    /// The script loops between A and B now.
    pub active: bool,
    /// The script's own name prompt or a menu is open (mpv's keys); commands are ignored meanwhile.
    pub busy: bool,
}

/// Why the list of saved ranges was published.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Cause {
    /// A file was loaded.
    #[default]
    Load,
    /// This player changed the saved ranges (the other players showing the file re-read theirs).
    Write,
    /// Re-read on request.
    Reload,
}

/// A saved range, as the script stores it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SavedRange {
    #[serde(default)]
    pub name: String,
    pub a_point: f64,
    pub b_point: f64,
}

impl SavedRange {
    pub fn length(&self) -> f64 {
        (self.b_point - self.a_point).max(0.0)
    }

    /// Whether this is the range from `a` to `b` (as precise as the script compares them).
    pub fn is_at(&self, a: f64, b: f64) -> bool {
        (self.a_point - a).abs() < 0.0005 && (self.b_point - b).abs() < 0.0005
    }
}

/// The saved ranges of the file a player shows: `user-data/ab_repeat/ranges`.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct RangeList {
    /// The file (mpv's `path`, the key of the script's JSON).
    pub path: String,
    pub revision: u64,
    pub cause: Cause,
    #[serde(deserialize_with = "list_or_empty_map")]
    pub list: Vec<SavedRange>,
}

/// mpv turns an empty Lua table into an empty map (`{}`) unless the script marks it as an array.
fn list_or_empty_map<'de, D>(deserializer: D) -> Result<Vec<SavedRange>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum ListOrMap {
        List(Vec<SavedRange>),
        Map(serde::de::IgnoredAny),
    }
    Ok(match ListOrMap::deserialize(deserializer)? {
        ListOrMap::List(list) => list,
        ListOrMap::Map(_) => Vec::new(),
    })
}

/// Parses the JSON mpv gives for an observed `user-data` path (`None` when the path does not exist or is not what it should be).
pub fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> Option<T> {
    let json = json.trim();
    if json.is_empty() || json == "null" {
        return None;
    }
    serde_json::from_str(json).ok()
}

/// A saved range named the way the script checks it: its 1-based index plus its points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeRef {
    pub index: usize,
    pub a: f64,
    pub b: f64,
}

impl RangeRef {
    /// The range at `index` (0-based) of `list`.
    pub fn of(list: &[SavedRange], index: usize) -> Option<Self> {
        list.get(index).map(|range| Self {
            index: index + 1,
            a: range.a_point,
            b: range.b_point,
        })
    }
}

/// A command of the script's interface.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Set A (or B) at the current position, or at a time (a dragged flag).
    SetA(Option<f64>),
    SetB(Option<f64>),
    Clear,
    /// The loop of a valid A-B on or off (the points stay).
    Loop(bool),
    JumpToA,
    Save(String),
    Load(RangeRef),
    Delete(RangeRef),
    DeleteAll,
    Rename(RangeRef, String),
    NextRange,
    PreviousRange,
    /// Publish the saved ranges again (another player changed the file).
    Reload,
    /// The saved ranges of a renamed or moved file go to its new path.
    MovePath(String, String),
    /// `user-data/ab_repeat/pong` becomes the token once every command sent before it is done.
    Ping(String),
}

impl Command {
    /// Clears only A (`a`) or only B. The script can only clear both, so the other point is set again where it was, and a loop
    /// that was off stays off (setting a point switches it on). Nothing when the point is not set by the user (a point the
    /// script set by itself only stands for "not set"); a plain clear when the other one is not set by the user either.
    pub fn clear_point(a: bool, state: &ScriptState) -> Vec<Self> {
        let (this, this_auto, other, other_auto) = if a {
            (state.a, state.a_auto, state.b, state.b_auto)
        } else {
            (state.b, state.b_auto, state.a, state.a_auto)
        };
        if this.is_none() || this_auto {
            return Vec::new();
        }
        let mut commands = vec![Self::Clear];
        if let Some(other) = other.filter(|_| !other_auto) {
            commands.push(if a {
                Self::SetB(Some(other))
            } else {
                Self::SetA(Some(other))
            });
            if !state.loop_on {
                commands.push(Self::Loop(false));
            }
        }
        commands
    }

    /// The arguments of `script-message-to` for the script running as `client`.
    pub fn args(&self, client: &str) -> Vec<String> {
        let time = |secs: f64| format!("{secs:.3}");
        let range = |r: &RangeRef| vec![r.index.to_string(), format!("{:.6}", r.a), format!("{:.6}", r.b)];
        let (name, rest): (&str, Vec<String>) = match self {
            Self::SetA(at) => ("set-a", at.map(time).into_iter().collect()),
            Self::SetB(at) => ("set-b", at.map(time).into_iter().collect()),
            Self::Clear => ("clear", Vec::new()),
            Self::Loop(on) => ("loop", vec![if *on { "on" } else { "off" }.to_owned()]),
            Self::JumpToA => ("jump-a", Vec::new()),
            Self::Save(name) => ("save", vec![name.clone()]),
            Self::Load(r) => ("load", range(r)),
            Self::Delete(r) => ("delete", range(r)),
            Self::DeleteAll => ("delete-all", Vec::new()),
            Self::Rename(r, name) => {
                let mut args = range(r);
                args.push(name.clone());
                ("rename", args)
            }
            Self::NextRange => ("next-range", Vec::new()),
            Self::PreviousRange => ("prev-range", Vec::new()),
            Self::Reload => ("reload", Vec::new()),
            Self::MovePath(from, to) => ("move-path", vec![from.clone(), to.clone()]),
            Self::Ping(token) => ("ping", vec![token.clone()]),
        };
        let mut args = vec![client.to_owned(), name.to_owned()];
        args.extend(rest);
        args
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_values_parse() {
        let info: ScriptInfo = parse(
            r#"{"name":"ab_repeat","version":"2.0","api":1,"client":"ab_repeat","db":"/x/ab_repeat.json",
                "config":{"entries_per_page":10,"show_osd_messages":true,"osd_messages":false,"b_point_tolerance":0.1}}"#,
        )
        .unwrap();
        assert_eq!(info.api, 1);
        assert_eq!(info.client, "ab_repeat");
        assert!(!info.config.osd_messages);
        let state: ScriptState = parse(
            r#"{"a":30,"b":89.96,"a_auto":false,"b_auto":true,"valid":true,"loop":true,"active":true,"busy":false}"#,
        )
        .unwrap();
        assert_eq!(state.a, Some(30.0));
        assert!(state.b_auto && state.loop_on && state.active);
        let empty: ScriptState = parse(r#"{"valid":false,"loop":true}"#).unwrap();
        assert_eq!(empty.a, None);
        let ranges: RangeList = parse(
            r#"{"path":"/v.mp4","revision":3,"cause":"write","list":[{"name":"Intro","a_point":0,"b_point":6}]}"#,
        )
        .unwrap();
        assert_eq!(ranges.cause, Cause::Write);
        assert_eq!(ranges.list[0].name, "Intro");
        assert!((ranges.list[0].length() - 6.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_list_may_come_as_an_empty_map() {
        let ranges: RangeList = parse(r#"{"path":"/v.mp4","revision":1,"cause":"load","list":{}}"#).unwrap();
        assert_eq!(ranges.list, Vec::<SavedRange>::new());
        assert_eq!(parse::<ScriptInfo>(""), None);
        assert_eq!(parse::<ScriptInfo>("null"), None);
    }

    #[test]
    fn commands_name_ranges_by_index_and_points() {
        let list = vec![SavedRange {
            name: "Intro".into(),
            a_point: 1.5,
            b_point: 6.0,
        }];
        let r = RangeRef::of(&list, 0).unwrap();
        assert_eq!(r.index, 1);
        assert_eq!(
            Command::Rename(r, "Über – neu".into()).args("ab_repeat"),
            vec!["ab_repeat", "rename", "1", "1.500000", "6.000000", "Über – neu"]
        );
        assert_eq!(Command::SetA(Some(12.3456)).args("abr"), vec!["abr", "set-a", "12.346"]);
        assert_eq!(Command::SetB(None).args("abr"), vec!["abr", "set-b"]);
        assert_eq!(Command::Loop(false).args("abr"), vec!["abr", "loop", "off"]);
        assert!(RangeRef::of(&list, 1).is_none());
    }

    #[test]
    fn one_point_is_cleared_by_clearing_both_and_setting_the_other_again() {
        let both = ScriptState {
            a: Some(10.0),
            b: Some(20.0),
            valid: true,
            loop_on: true,
            ..ScriptState::default()
        };
        assert_eq!(
            Command::clear_point(true, &both),
            vec![Command::Clear, Command::SetB(Some(20.0))]
        );
        assert_eq!(
            Command::clear_point(false, &ScriptState { loop_on: false, ..both }),
            vec![Command::Clear, Command::SetA(Some(10.0)), Command::Loop(false)],
            "a loop that was off stays off"
        );
        let only_a = ScriptState {
            b: Some(89.9),
            b_auto: true,
            ..both
        };
        assert_eq!(
            Command::clear_point(true, &only_a),
            vec![Command::Clear],
            "the script's B goes too"
        );
        assert_eq!(
            Command::clear_point(false, &only_a),
            Vec::new(),
            "B set by the script is no point of the user"
        );
        assert_eq!(Command::clear_point(true, &ScriptState::default()), Vec::new());
    }
}
