//! The progress bar of the control bar as an open model (pure).
//!
//! The bar draws a track and, above it, layers of spans: stretches of the video, each with a kind that decides its color in
//! `ui/multiview_video_bar_window.slint`. The played part is just the first span. A new kind of stretch (an A-B repeat loop,
//! chapters, what is buffered, ...) is a `SpanKind` variant, its color in the window, and the code that adds its spans to the
//! track with `ProgressTrack::with_span`; nothing else of the bar changes. Spans are given in seconds and turned into shares
//! of the bar here, so they stay right when the length of the video becomes known or changes.

use rustview_core::util::cast::Az;

/// What a stretch of the progress bar means. The code is what the window draws by (keep it in sync with the window).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// From the start to the current position.
    Played,
}

impl SpanKind {
    pub const fn code(self) -> i32 {
        match self {
            Self::Played => 0,
        }
    }
}

/// A stretch of the video, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressSpan {
    pub from_secs: f64,
    pub to_secs: f64,
    pub kind: SpanKind,
}

/// A span as the window draws it: shares of the bar's width (0..=1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpanShare {
    pub from: f32,
    pub to: f32,
    pub kind: SpanKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgressTrack {
    duration_secs: f64,
    position_secs: f64,
    spans: Vec<ProgressSpan>,
}

impl ProgressTrack {
    /// The track of a video of `duration_secs` (0 while unknown) at `position_secs`, with its played part.
    pub fn new(duration_secs: f64, position_secs: f64) -> Self {
        let duration_secs = if duration_secs.is_finite() {
            duration_secs.max(0.0)
        } else {
            0.0
        };
        let position_secs = if position_secs.is_finite() {
            position_secs.clamp(0.0, duration_secs)
        } else {
            0.0
        };
        Self {
            duration_secs,
            position_secs,
            spans: vec![ProgressSpan {
                from_secs: 0.0,
                to_secs: position_secs,
                kind: SpanKind::Played,
            }],
        }
    }

    /// The track with one more span on top of the others (drawn above the spans added before it).
    pub fn with_span(mut self, span: ProgressSpan) -> Self {
        self.spans.push(span);
        self
    }

    pub fn duration_secs(&self) -> f64 {
        self.duration_secs
    }

    /// Where the position is, as a share of the bar.
    pub fn fraction(&self) -> f32 {
        self.share_of(self.position_secs)
    }

    /// The time at `fraction` of the bar (a click or a drag on it); 0 while the length is unknown.
    pub fn time_at(&self, fraction: f32) -> f64 {
        f64::from(fraction.clamp(0.0, 1.0)) * self.duration_secs
    }

    /// The spans as shares of the bar, in drawing order; empty stretches are left out.
    pub fn shares(&self) -> Vec<SpanShare> {
        self.spans
            .iter()
            .filter_map(|span| {
                let from = self.share_of(span.from_secs.min(span.to_secs));
                let to = self.share_of(span.from_secs.max(span.to_secs));
                (to > from).then_some(SpanShare {
                    from,
                    to,
                    kind: span.kind,
                })
            })
            .collect()
    }

    fn share_of(&self, secs: f64) -> f32 {
        if self.duration_secs <= 0.0 || !secs.is_finite() {
            return 0.0;
        }
        (secs / self.duration_secs).clamp(0.0, 1.0).az::<f32>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_played_part_is_the_first_span() {
        let track = ProgressTrack::new(100.0, 25.0);
        assert!((track.fraction() - 0.25).abs() < 1e-6);
        assert_eq!(
            track.shares(),
            vec![SpanShare {
                from: 0.0,
                to: 0.25,
                kind: SpanKind::Played
            }]
        );
        assert!((track.time_at(0.5) - 50.0).abs() < 1e-9);
        assert!((track.time_at(1.7) - 100.0).abs() < 1e-9, "clamped to the end");
    }

    #[test]
    fn an_unknown_length_shows_nothing_and_seeks_to_the_start() {
        let track = ProgressTrack::new(0.0, 12.0);
        assert!(track.fraction().abs() < f32::EPSILON);
        assert_eq!(track.shares(), Vec::<SpanShare>::new());
        assert!(track.time_at(0.5).abs() < f64::EPSILON);
        assert_eq!(
            ProgressTrack::new(f64::NAN, f64::INFINITY).shares(),
            Vec::<SpanShare>::new()
        );
    }

    #[test]
    fn added_spans_are_clamped_ordered_and_drawn_above() {
        let track = ProgressTrack::new(200.0, 50.0)
            .with_span(ProgressSpan {
                from_secs: 150.0,
                to_secs: 100.0,
                kind: SpanKind::Played,
            })
            .with_span(ProgressSpan {
                from_secs: 180.0,
                to_secs: 400.0,
                kind: SpanKind::Played,
            })
            .with_span(ProgressSpan {
                from_secs: 30.0,
                to_secs: 30.0,
                kind: SpanKind::Played,
            });
        let shares = track.shares();
        assert_eq!(shares.len(), 3, "the empty span is left out");
        assert!((shares[1].from - 0.5).abs() < 1e-6 && (shares[1].to - 0.75).abs() < 1e-6);
        assert!((shares[2].from - 0.9).abs() < 1e-6 && (shares[2].to - 1.0).abs() < 1e-6);
    }
}
