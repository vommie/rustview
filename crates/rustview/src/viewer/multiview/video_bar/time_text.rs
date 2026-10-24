//! The two times of the control bar (pure): the current time on the left and the remaining time on the right, mpv style.
//!
//! Short form: "1:23" and "-0:45", hours only for a video of an hour or more ("1:02:03"); a click on a time adds the
//! milliseconds (".456"). Every text comes with a template of the widest text the video can need (all digits "0"), so the
//! bar can give the times a fixed width and the progress bar does not move while the video plays (the digits of the UI fonts
//! all have the same width).

use rustview_core::util::cast::FloatToInt;
use std::fmt::Write;

/// The times of the bar and the templates their width is measured with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TimeTexts {
    pub current: String,
    pub remaining: String,
    pub current_template: String,
    pub remaining_template: String,
}

/// A video of at least this length shows hours.
const HOUR_SECS: f64 = 3600.0;

/// `secs` as "m:ss" ("h:mm:ss" with `hours`), plus ".mmm" with `ms`. Negative and non-finite times count as 0.
pub fn clock(secs: f64, hours: bool, ms: bool) -> String {
    let total_ms = if secs.is_finite() && secs > 0.0 {
        (secs * 1000.0).floor().to_int::<u64>()
    } else {
        0
    };
    let (whole, millis) = (total_ms / 1000, total_ms % 1000);
    let (h, m, s) = (whole / 3600, whole / 60 % 60, whole % 60);
    let mut text = if hours {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{}:{s:02}", whole / 60)
    };
    if ms {
        let _ = write!(text, ".{millis:03}");
    }
    text
}

/// The times for a video of `duration` seconds at `position`. Without milliseconds the remaining time is rounded up, so it
/// reads the whole length at the start and "-0:00" only at the very end.
pub fn texts(position: f64, duration: f64, ms: bool) -> TimeTexts {
    let duration = if duration.is_finite() { duration.max(0.0) } else { 0.0 };
    let position = if position.is_finite() {
        position.clamp(0.0, duration.max(position.max(0.0)))
    } else {
        0.0
    };
    let hours = duration >= HOUR_SECS;
    let left = (duration - position).max(0.0);
    let left = if ms { left } else { left.ceil() };
    let template = zeros(&clock(duration.max(position), hours, ms));
    TimeTexts {
        current: clock(position, hours, ms),
        remaining: format!("-{}", clock(left, hours, ms)),
        remaining_template: format!("-{template}"),
        current_template: template,
    }
}

/// The time badge of a video in the tab grid: "0:03 / 0:08" (hours when the video is that long); `None` while its length is
/// not known.
pub fn badge(position: f64, duration: f64) -> Option<String> {
    if !duration.is_finite() || duration <= 0.0 {
        return None;
    }
    let hours = duration >= HOUR_SECS;
    Some(format!(
        "{} / {}",
        clock(position.min(duration), hours, false),
        clock(duration, hours, false)
    ))
}

/// `text` with every digit replaced by "0".
fn zeros(text: &str) -> String {
    text.chars().map(|c| if c.is_ascii_digit() { '0' } else { c }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_times_without_hours() {
        assert_eq!(clock(83.4, false, false), "1:23");
        assert_eq!(clock(5.0, false, false), "0:05");
        assert_eq!(clock(83.456_7, false, true), "1:23.456");
        assert_eq!(clock(-3.0, false, false), "0:00");
        assert_eq!(clock(f64::NAN, false, true), "0:00.000");
        assert_eq!(clock(754.0, false, false), "12:34");
    }

    #[test]
    fn hours_only_for_long_videos() {
        let short = texts(83.0, 128.0, false);
        assert_eq!(short.current, "1:23");
        assert_eq!(short.remaining, "-0:45");
        assert_eq!(short.current_template, "0:00");
        assert_eq!(short.remaining_template, "-0:00");

        let long = texts(3723.0, 7200.0, false);
        assert_eq!(long.current, "1:02:03");
        assert_eq!(long.remaining, "-0:57:57");
        assert_eq!(long.current_template, "0:00:00");
    }

    #[test]
    fn the_remaining_time_is_rounded_up_without_milliseconds() {
        let start = texts(0.0, 128.4, false);
        assert_eq!(start.remaining, "-2:09");
        assert_eq!(texts(128.4, 128.4, false).remaining, "-0:00");
        assert_eq!(texts(83.25, 128.5, true).remaining, "-0:45.250");
    }

    #[test]
    fn the_badge_names_position_and_length() {
        assert_eq!(badge(3.04, 8.0).as_deref(), Some("0:03 / 0:08"));
        assert_eq!(badge(3723.0, 7200.0).as_deref(), Some("1:02:03 / 2:00:00"));
        assert_eq!(badge(12.0, 8.0).as_deref(), Some("0:08 / 0:08"));
        assert_eq!(badge(1.0, 0.0), None);
    }

    #[test]
    fn the_templates_cover_the_widest_text() {
        let t = texts(5.0, 754.0, true);
        assert_eq!(t.current, "0:05.000");
        assert_eq!(t.current_template, "00:00.000");
        assert_eq!(t.remaining_template, "-00:00.000");
        // A position past a duration that is not known yet does not break the texts.
        let unknown = texts(12.0, 0.0, false);
        assert_eq!(unknown.current, "0:12");
        assert_eq!(unknown.remaining, "-0:00");
    }
}
