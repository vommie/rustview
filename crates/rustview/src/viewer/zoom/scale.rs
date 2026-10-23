//! The scale of the zoom slider and the steps of the mouse wheel (pure).
//!
//! The slider is one logarithmic range from the smallest to the largest zoom, so the same distance is the same factor everywhere
//! and the reset (100% = the fitted picture) sits where its factor puts it: far to the left when the picture can be enlarged much
//! more than shrunk, at the left end when it cannot be shrunk at all. Moving the handle snaps ("gravity") to the marks of the
//! slider: the reset, the steps of 25% above it (125, 150, 175, 200, ...) and 75/50/25% below it.
//!
//! The marks are thinned out by the room there is: the more important ones (reset, whole 100% steps, 50% steps, then 25% steps)
//! are placed first and a lesser one is left out where it would sit closer than a few pixels to one already placed. That gives
//! a regular rhythm where there is room (near the reset) and only the round numbers where the scale is compressed.
//!
//! A wheel step goes to the next rung of a ladder instead of adding a fixed amount, so the steps feel the same at every zoom
//! and a step up followed by a step down returns to where it started: below 100% the rungs are `100% - k * step` (5% steps:
//! 95, 90, ...), above they are `100% * (1 + step)^k` (5%: 105, 110, 116, ...). The ladder always contains 100%, and a step
//! never jumps over it.

use super::geometry::Limits;
use rustview_core::util::cast::FloatToInt;

/// How far (in slider pixels) the handle is pulled to a snap point.
const SNAP_RESET_PX: f64 = 9.0;
const SNAP_PX: f64 = 5.0;
/// The least distance (slider pixels) between two marks: lines, and dots.
const GAP_LINE_PX: f64 = 6.0;
const GAP_DOT_PX: f64 = 5.0;
/// A zoom within this many rungs of a rung is on it (the stored zoom is rounded, so it is never exactly on one).
const ON_RUNG: f64 = 1e-4;

/// A mark of the slider. `rank` decides how it is drawn: 0 = the reset (thickest), 1 = a whole 100% step or an end (a line),
/// 2 = a 50% step (a shorter line), 3 = a 25% / 75% step (a dot).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    pub zoom: f64,
    pub rank: u8,
}

/// Position on the slider (0..1) of a zoom factor: the logarithm of the zoom between the two limits.
pub fn position_of(zoom: f64, limits: Limits) -> f64 {
    let zoom = limits.clamp(zoom);
    (zoom / limits.min).ln() / (limits.max / limits.min).ln()
}

/// The zoom factor at a position of the slider (0..1).
pub fn zoom_at(position: f64, limits: Limits) -> f64 {
    let p = if position.is_finite() {
        position.clamp(0.0, 1.0)
    } else {
        position_of(1.0, limits)
    };
    limits.min * (limits.max / limits.min).powf(p)
}

/// The marks of the slider, ascending; `width_px` is the length of the track.
pub fn ticks(limits: Limits, width_px: f64) -> Vec<Tick> {
    let width = width_px.max(1.0);
    let at = |zoom: f64| position_of(zoom, limits) * width;
    // (mark, priority): the reset first, then the ends, then the steps by their rank.
    let mut candidates = vec![
        (Tick { zoom: 1.0, rank: 0 }, 0u8),
        (
            Tick {
                zoom: limits.min,
                rank: 1,
            },
            1,
        ),
        (
            Tick {
                zoom: limits.max,
                rank: 1,
            },
            1,
        ),
    ];
    // The steps of 25% above the reset. A step belongs to a grid (25, 50 or 100%) and is drawn while that grid still has room: the
    // distance to the next step of the same grid is what decides, so a grid never ends in the middle of a gap.
    let mut d = 25u32;
    while 1.0 + f64::from(d) / 100.0 < limits.max * 0.999 {
        let (grid, rank) = if d.is_multiple_of(100) {
            (100, 1)
        } else if d.is_multiple_of(50) {
            (50, 2)
        } else {
            (25, 3)
        };
        let zoom = 1.0 + f64::from(d) / 100.0;
        let gap = if rank == 3 { GAP_DOT_PX } else { GAP_LINE_PX };
        if at(zoom + f64::from(grid) / 100.0) - at(zoom) >= gap {
            candidates.push((Tick { zoom, rank }, rank + 1));
        }
        d += 25;
    }
    // Where even the whole steps are too close, the doubling goes on: 200%, 400%, 800%, ...
    let mut doubled = 2.0;
    while doubled < limits.max * 0.999 {
        candidates.push((Tick { zoom: doubled, rank: 1 }, 2));
        doubled *= 2.0;
    }
    for (zoom, rank) in [(0.75, 3), (0.5, 2), (0.25, 3), (0.1, 1)] {
        if zoom > limits.min * 1.001 {
            candidates.push((Tick { zoom, rank }, rank + 1));
        }
    }
    candidates.retain(|(t, _)| t.zoom >= limits.min * 0.999 && t.zoom <= limits.max * 1.001);
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.zoom.total_cmp(&b.0.zoom)));
    let mut placed: Vec<Tick> = Vec::new();
    for (candidate, _) in candidates {
        let gap = if candidate.rank == 3 { GAP_DOT_PX } else { GAP_LINE_PX };
        if placed.iter().all(|t| (at(t.zoom) - at(candidate.zoom)).abs() >= gap) {
            placed.push(candidate);
        }
    }
    placed.sort_by(|a, b| a.zoom.total_cmp(&b.zoom));
    placed
}

/// The zoom factors the handle snaps to, ascending: the marks.
pub fn snap_zooms(limits: Limits, width_px: f64) -> Vec<f64> {
    ticks(limits, width_px).into_iter().map(|t| t.zoom).collect()
}

/// Pulls a handle position to the nearest snap point that is within a few pixels of it. `width_px` is the length of the track.
pub fn snap_position(position: f64, width_px: f64, limits: Limits) -> f64 {
    let p = if position.is_finite() {
        position.clamp(0.0, 1.0)
    } else {
        position_of(1.0, limits)
    };
    let width = width_px.max(1.0);
    let mut best: Option<(f64, f64)> = None;
    for zoom in snap_zooms(limits, width) {
        let target = position_of(zoom, limits);
        let reach = if (zoom - 1.0).abs() < 1e-9 {
            SNAP_RESET_PX
        } else {
            SNAP_PX
        };
        let distance = (target - p).abs() * width;
        if distance <= reach && best.is_none_or(|(_, d)| distance < d) {
            best = Some((target, distance));
        }
    }
    best.map_or(p, |(target, _)| target)
}

/// The zoom of the next rung of the ladder with `step` (0.05 = 5%) above (`up`) or below the current zoom.
pub fn step(zoom: f64, up: bool, step: f64, limits: Limits) -> f64 {
    let b = if step.is_finite() { step.clamp(0.01, 1.0) } else { 0.1 };
    let zoom = limits.clamp(zoom);
    let next = if zoom < 1.0 - 1e-9 {
        // Additive rungs below 100%: k steps of `b` below the fit.
        let k = (1.0 - zoom) / b;
        let on_rung = (k - k.round()).abs() < ON_RUNG;
        let k_new = match (up, on_rung) {
            (true, true) => k.round() - 1.0,
            (true, false) => k.floor(),
            (false, true) => k.round() + 1.0,
            (false, false) => k.ceil(),
        };
        if k_new <= 0.0 { 1.0 } else { 1.0 - k_new * b }
    } else {
        // Geometric rungs from 100% up: (1 + b)^j.
        let base = 1.0 + b;
        let j = zoom.ln() / base.ln();
        let on_rung = (j - j.round()).abs() < ON_RUNG;
        let j_new = match (up, on_rung) {
            (true, true) => j.round() + 1.0,
            (true, false) => j.ceil(),
            (false, true) => j.round() - 1.0,
            (false, false) => j.floor(),
        };
        if j_new <= 0.0 {
            // Going down from above 100% stops at 100%, and from 100% continues below it.
            if (zoom - 1.0).abs() < 1e-9 && !up { 1.0 - b } else { 1.0 }
        } else {
            base.powf(j_new)
        }
    };
    // Rounded to a ten-thousandth of a percent: no float noise, and a rung is a rung however it was reached.
    limits.clamp((next * 1_000_000.0).round() / 1_000_000.0)
}

/// The step of the wheel over the slider: 5% by default, 10% with Shift, 25% with Ctrl.
pub fn slider_wheel_step(shift: bool, ctrl: bool) -> f64 {
    if ctrl {
        0.25
    } else if shift {
        0.10
    } else {
        0.05
    }
}

/// How the zoom is told to the outside (the slider and the status): tenths of a percent as a whole number.
pub fn to_permille(zoom: f64) -> i32 {
    (zoom * 1000.0).round().clamp(0.0, 1_000_000.0).to_int::<i32>()
}

pub fn from_permille(permille: i32) -> f64 {
    f64::from(permille.max(0)) / 1000.0
}

/// "150%" - what the tooltip of the slider and the status show.
pub fn percent_text(zoom: f64) -> String {
    format!("{}%", (zoom * 100.0).round().to_int::<i64>())
}
