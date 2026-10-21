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
    let p = if position.is_finite() { position.clamp(0.0, 1.0) } else { position_of(1.0, limits) };
    limits.min * (limits.max / limits.min).powf(p)
}

/// The marks of the slider, ascending; `width_px` is the length of the track.
pub fn ticks(limits: Limits, width_px: f64) -> Vec<Tick> {
    let width = width_px.max(1.0);
    let at = |zoom: f64| position_of(zoom, limits) * width;
    // (mark, priority): the reset first, then the ends, then the steps by their rank.
    let mut candidates = vec![(Tick { zoom: 1.0, rank: 0 }, 0u8), (Tick { zoom: limits.min, rank: 1 }, 1), (Tick { zoom: limits.max, rank: 1 }, 1)];
    // The steps of 25% above the reset. A step belongs to a grid (25, 50 or 100%) and is drawn while that grid still has room: the
    // distance to the next step of the same grid is what decides, so a grid never ends in the middle of a gap.
    let mut d = 25u32;
    while 1.0 + f64::from(d) / 100.0 < limits.max * 0.999 {
        let (grid, rank) = if d.is_multiple_of(100) { (100, 1) } else if d.is_multiple_of(50) { (50, 2) } else { (25, 3) };
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
    let p = if position.is_finite() { position.clamp(0.0, 1.0) } else { position_of(1.0, limits) };
    let width = width_px.max(1.0);
    let mut best: Option<(f64, f64)> = None;
    for zoom in snap_zooms(limits, width) {
        let target = position_of(zoom, limits);
        let reach = if (zoom - 1.0).abs() < 1e-9 { SNAP_RESET_PX } else { SNAP_PX };
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
    (zoom * 1000.0).round().clamp(0.0, 1_000_000.0) as i32
}

pub fn from_permille(permille: i32) -> f64 {
    f64::from(permille.max(0)) / 1000.0
}

/// "150%" - what the tooltip of the slider and the status show.
pub fn percent_text(zoom: f64) -> String {
    format!("{}%", (zoom * 100.0).round() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: Limits = Limits::DEFAULT;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_ends_are_the_limits_and_the_reset_is_where_its_factor_puts_it() {
        assert!(close(position_of(L.min, L), 0.0));
        assert!(close(position_of(L.max, L), 1.0));
        assert!(close(zoom_at(0.0, L), L.min));
        assert!(close(zoom_at(1.0, L), L.max));
        // 10% .. 1000%: the reset is exactly in the middle; with 33% .. 1000% it is far to the left.
        assert!(close(position_of(1.0, L), 0.5));
        let wide = Limits::new(1.0 / 3.0, 10.0);
        let reset = position_of(1.0, wide);
        assert!(reset > 0.2 && reset < 0.4, "{reset}");
        assert!(close(zoom_at(reset, wide), 1.0));
        // The same distance is the same factor on both sides of the reset.
        assert!(close(position_of(2.0, wide) - reset, reset - position_of(0.5, wide)));
    }

    #[test]
    fn without_zooming_out_the_reset_is_the_left_end() {
        let l = Limits::new(1.0, 10.0);
        assert!(!l.can_zoom_out());
        assert!(close(position_of(1.0, l), 0.0));
        assert!(close(position_of(10.0, l), 1.0));
        assert!(close(position_of(0.3, l), 0.0), "smaller than the fit counts as the fit");
        assert!(close(zoom_at(0.0, l), 1.0) && close(zoom_at(1.0, l), 10.0));
        assert!(close(zoom_at(f64::NAN, l), 1.0));
        let points = snap_zooms(l, 172.0);
        assert!(close(points[0], 1.0) && points.iter().all(|p| *p >= 1.0));
        assert!(close(snap_position(4.0 / 172.0, 172.0, l), 0.0), "the reset pulls at the left end");
        assert!(close(step(1.0, false, 0.1, l), 1.0), "no step below the fit");
        assert!(step(1.0, true, 0.1, l) > 1.0);
    }

    #[test]
    fn position_and_zoom_are_inverse_and_monotonic() {
        for limits in [L, Limits::new(1.0 / 3.0, 10.0), Limits::new(1.0, 10.0), Limits::new(0.09, 64.0)] {
            let mut last = 0.0;
            for i in 0..=100 {
                let p = f64::from(i) / 100.0;
                let z = zoom_at(p, limits);
                assert!(z > last, "monotonic at {p}");
                last = z;
                assert!((position_of(z, limits) - p).abs() < 1e-9, "round trip at {p}");
            }
        }
        assert!(close(zoom_at(f64::NAN, L), 1.0));
        assert!(close(zoom_at(7.0, L), L.max));
        assert!(close(zoom_at(-7.0, L), L.min));
    }

    #[test]
    fn the_marks_are_regular_where_there_is_room_and_the_round_numbers_where_there_is_not() {
        let marks = ticks(Limits::new(1.0 / 3.0, 10.0), 172.0);
        let has = |z: f64| marks.iter().any(|t| close(t.zoom, z));
        let rank = |z: f64| marks.iter().find(|t| close(t.zoom, z)).map(|t| t.rank);
        // The reset is the strongest mark; the steps next to it are all there and drawn by size.
        assert_eq!(rank(1.0), Some(0));
        assert_eq!(rank(2.0), Some(1));
        for (z, r) in [(1.25, 3), (1.5, 2), (1.75, 3), (0.75, 3), (0.5, 2)] {
            assert_eq!(rank(z), Some(r), "{z}");
        }
        // Far out the 25% steps are left out, the whole steps are not.
        assert!(!has(8.25) && !has(8.5), "no crowd near the end");
        assert!(has(3.0));
        // Whatever is kept is spaced out.
        let width = 172.0;
        for pair in marks.windows(2) {
            let gap = (position_of(pair[1].zoom, Limits::new(1.0 / 3.0, 10.0)) - position_of(pair[0].zoom, Limits::new(1.0 / 3.0, 10.0))) * width;
            assert!(gap >= GAP_DOT_PX - 1e-9, "{pair:?} {gap}");
        }
        // Ascending, with both ends.
        assert!(marks.windows(2).all(|w| w[0].zoom < w[1].zoom));
        assert!(has(1.0 / 3.0) && has(10.0));
        // Nothing above the limit, and the marks below the fit go as far as the limit allows.
        assert!(ticks(Limits::new(0.1, 3.0), 172.0).iter().all(|t| t.zoom <= 3.0 + 1e-9));
        assert!(!ticks(Limits::new(0.6, 3.0), 172.0).iter().any(|t| t.zoom < 0.6 - 1e-9));
        // A narrow track keeps fewer marks, never none.
        assert!(ticks(L, 40.0).len() >= 3);
    }

    #[test]
    fn the_handle_is_pulled_to_the_reset_and_to_the_other_marks_only_when_close() {
        let width = 172.0;
        let limits = Limits::new(1.0 / 3.0, 10.0);
        let reset = position_of(1.0, limits);
        assert!(close(snap_position(reset + 4.0 / width, width, limits), reset));
        assert!(close(snap_position(reset - 6.0 / width, width, limits), reset));
        let at_150 = position_of(1.5, limits);
        assert!(close(snap_position(at_150 + 2.0 / width, width, limits), at_150));
        let at_50 = position_of(0.5, limits);
        assert!(close(snap_position(at_50 - 3.0 / width, width, limits), at_50));
        // Between two marks, far from both, the handle stays where it is.
        let p = (position_of(4.0, limits) + position_of(5.0, limits)) / 2.0;
        assert!(close(snap_position(p, width, limits), p));
        assert!(close(snap_position(0.999, width, limits), 1.0));
        assert!(close(snap_position(f64::NAN, width, limits), reset));
    }

    #[test]
    fn steps_go_up_and_down_the_ladder_and_come_back() {
        let mut z = 1.0;
        let mut seen = Vec::new();
        for _ in 0..4 {
            z = step(z, true, 0.10, L);
            seen.push(z);
        }
        assert_eq!(seen, vec![1.1, 1.21, 1.331, 1.4641]);
        for expected in [1.331, 1.21, 1.1, 1.0] {
            z = step(z, false, 0.10, L);
            assert!(close(z, expected), "{z} {expected}");
        }
        // Below 100% the rungs are additive and also come back.
        let mut z = 1.0;
        for expected in [0.95, 0.90, 0.85] {
            z = step(z, false, 0.05, L);
            assert!(close(z, expected), "{z}");
        }
        for expected in [0.90, 0.95, 1.0] {
            z = step(z, true, 0.05, L);
            assert!(close(z, expected), "{z}");
        }
    }

    #[test]
    fn a_step_never_jumps_over_the_reset() {
        // From 108% down with 25% steps: the next rung is 100%, not 86%.
        assert!(close(step(1.08, false, 0.25, L), 1.0));
        // From 96% up: 100%.
        assert!(close(step(0.96, true, 0.25, L), 1.0));
        assert!(close(step(0.999, true, 0.05, L), 1.0));
        assert!(close(step(1.001, false, 0.05, L), 1.0));
    }

    #[test]
    fn off_ladder_values_land_on_the_next_rung_in_the_direction_of_the_step() {
        // The slider left the zoom at 133%; one step up with 10% goes to the rung above (1.1^3 = 133.1%, which is on it; use 135%).
        assert!(step(1.35, true, 0.10, L) > 1.35);
        assert!(step(1.35, false, 0.10, L) < 1.35);
        assert!(step(0.37, true, 0.05, L) > 0.37);
        assert!(step(0.37, false, 0.05, L) < 0.37);
    }

    #[test]
    fn steps_stop_at_the_limits() {
        let mut z = 1.0;
        for _ in 0..500 {
            z = step(z, true, 0.25, L);
        }
        assert!(close(z, L.max));
        assert!(close(step(z, true, 0.25, L), L.max));
        let mut z = 1.0;
        for _ in 0..500 {
            z = step(z, false, 0.25, L);
        }
        assert!(close(z, L.min));
        assert!(close(step(z, false, 0.25, L), L.min));
    }

    #[test]
    fn broken_input_is_harmless() {
        assert!(step(f64::NAN, true, 0.1, L).is_finite());
        assert!(step(1.0, true, f64::NAN, L) > 1.0);
        assert!(step(1.0, true, 0.0, L) > 1.0);
        assert!(step(1.0, true, 99.0, L) > 1.0);
    }

    #[test]
    fn the_wheel_over_the_slider_has_the_steps_the_user_asked_for() {
        assert!(close(slider_wheel_step(false, false), 0.05));
        assert!(close(slider_wheel_step(true, false), 0.10));
        assert!(close(slider_wheel_step(false, true), 0.25));
        assert!(close(slider_wheel_step(true, true), 0.25));
    }

    #[test]
    fn permille_and_text() {
        assert_eq!(to_permille(1.5), 1500);
        assert!(close(from_permille(1500), 1.5));
        assert_eq!(to_permille(-3.0), 0);
        assert_eq!(percent_text(1.0), "100%");
        assert_eq!(percent_text(0.254), "25%");
        assert_eq!(percent_text(1.256), "126%");
    }
}
