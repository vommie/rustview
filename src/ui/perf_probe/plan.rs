//! The scenario list of the performance probe, parsed from `RUSTVIEW_PERF` (pure, no UI).
//!
//! Syntax: comma separated entries.
//! - `grid:6000` / `table:6000` scrolls the view from the top to the bottom and back at that many pixels per second (a hard
//!   wheel flick is about 10,000 px/s); the speed is wall-clock based, so slow ticks do not change the distance covered.
//! - `jump:20` jumps 20 times to pseudo-random positions and measures until the tile pool is complete again.
//! - `switch:<folder name>` selects that folder in the folder tree and measures until its first screen is complete.
//! - `query:<text>` enters a search query and measures the same.

use crate::settings::LogObscurer;
use std::fmt;

pub const ENV_PLAN: &str = "RUSTVIEW_PERF";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeView {
    Grid,
    Table,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Scenario {
    Scroll { view: ProbeView, pixels_per_second: f32 },
    Jump { count: u32 },
    Switch { folder: String },
    Query { text: String },
}

impl fmt::Display for Scenario {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scenario::Scroll { view, pixels_per_second } => {
                let view = match view {
                    ProbeView::Grid => "grid",
                    ProbeView::Table => "table",
                };
                write!(f, "{view}:{pixels_per_second}")
            }
            Scenario::Jump { count } => write!(f, "jump:{count}"),
            // Folder names and queries are user data: the log text follows the log obfuscation setting.
            Scenario::Switch { folder } => write!(f, "switch:{}", LogObscurer::filename(folder)),
            Scenario::Query { text } => write!(f, "query:{}", LogObscurer::query(text)),
        }
    }
}

fn parse_entry(entry: &str) -> Option<Scenario> {
    let (kind, arg) = entry.trim().split_once(':')?;
    let arg = arg.trim();
    match kind.trim() {
        "grid" | "table" => {
            let pixels_per_second: f32 = arg.parse().ok()?;
            let view = if kind.trim() == "grid" { ProbeView::Grid } else { ProbeView::Table };
            (pixels_per_second.is_finite() && pixels_per_second > 0.0).then_some(Scenario::Scroll { view, pixels_per_second })
        }
        "jump" => arg.parse().ok().filter(|&count| count > 0).map(|count| Scenario::Jump { count }),
        "switch" => (!arg.is_empty()).then(|| Scenario::Switch { folder: arg.to_string() }),
        "query" => Some(Scenario::Query { text: arg.to_string() }),
        _ => None,
    }
}

/// Parses the plan; entries that do not parse are skipped (a typo must not abort the application).
pub fn parse_plan(spec: &str) -> Vec<Scenario> {
    spec.split(',').filter_map(parse_entry).collect()
}

pub fn plan_from_env() -> Vec<Scenario> {
    std::env::var(ENV_PLAN).map(|v| parse_plan(&v)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scroll_switch_and_query_entries() {
        let plan = parse_plan("grid:48, table:160.5,switch:small_100,query:holiday,jump:12");
        assert_eq!(plan.len(), 5);
        assert_eq!(plan[4], Scenario::Jump { count: 12 });
        assert_eq!(plan[0], Scenario::Scroll { view: ProbeView::Grid, pixels_per_second: 48.0 });
        assert_eq!(plan[1], Scenario::Scroll { view: ProbeView::Table, pixels_per_second: 160.5 });
        assert_eq!(plan[2], Scenario::Switch { folder: "small_100".into() });
        assert_eq!(plan[3], Scenario::Query { text: "holiday".into() });
        assert_eq!(plan[0].to_string(), "grid:48");
    }

    #[test]
    fn invalid_entries_are_skipped() {
        assert!(parse_plan("").is_empty());
        assert!(parse_plan("list:10,grid:abc,grid:-5,grid,switch:,jump:0,jump:x").is_empty());
        assert_eq!(parse_plan("grid:0,grid:10").len(), 1);
    }

    #[test]
    fn an_empty_query_is_allowed_to_clear_the_filter() {
        assert_eq!(parse_plan("query:"), vec![Scenario::Query { text: String::new() }]);
    }
}
