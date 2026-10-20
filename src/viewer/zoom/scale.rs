//! The scale of the zoom slider and the steps of the mouse wheel (pure).
//!
//! The slider has the reset (100% = the fitted picture) in its middle. Both halves are logarithmic, so the same distance on the
//! slider is the same factor, and the end of each half is a limit: the left end is the smallest zoom, the right end the
//! largest. Moving the handle snaps ("gravity") to the points that matter: the reset, 25/50/75% below it, 125/150/175/200%
//! and the whole numbers above it.
//!
//! A wheel step goes to the next rung of a ladder instead of adding a fixed amount, so the steps feel the same at every zoom
//! and a step up followed by a step down returns to where it started: below 100% the rungs are `100% - k * step` (5% steps:
//! 95, 90, ...), above they are `100% * (1 + step)^k` (5%: 105, 110, 116, ...). The ladder always contains 100%, and a step
//! never jumps over it.

use super::geometry::Limits;

/// How far (in slider pixels) the handle is pulled to a snap point.
const SNAP_RESET_PX: f64 = 9.0;
const SNAP_PX: f64 = 5.0;
/// A zoom within this many rungs of a rung is on it (the stored zoom is rounded, so it is never exactly on one).
const ON_RUNG: f64 = 1e-4;

/// Position on the slider (0..1, 0.5 = 100%) of a zoom factor. Without zooming out the reset is at the left end and the whole
/// slider is the way in.
pub fn position_of(zoom: f64, limits: Limits) -> f64 {
    let zoom = limits.clamp(zoom);
    if !limits.can_zoom_out() {
        zoom.ln() / limits.max.ln()
    } else if zoom >= 1.0 {
        0.5 + 0.5 * zoom.ln() / limits.max.ln()
    } else {
        0.5 - 0.5 * zoom.ln() / limits.min.ln()
    }
}

/// The zoom factor at a position of the slider (0..1).
pub fn zoom_at(position: f64, limits: Limits) -> f64 {
    let p = if position.is_finite() { position.clamp(0.0, 1.0) } else { 0.5 };
    if !limits.can_zoom_out() {
        if position.is_finite() { limits.max.powf(p) } else { 1.0 }
    } else if p >= 0.5 {
        limits.max.powf((p - 0.5) * 2.0)
    } else {
        limits.min.powf((0.5 - p) * 2.0)
    }
}

/// The zoom factors the handle snaps to, ascending: the two limits, 100%, 25/50/75% and the round numbers above 100%.
pub fn snap_zooms(limits: Limits) -> Vec<f64> {
    let mut points = vec![limits.min, limits.max, 1.0];
    for candidate in [0.25, 0.5, 0.75, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 16.0, 20.0, 32.0, 50.0] {
        if candidate > limits.min * 1.05 && candidate < limits.max / 1.05 {
            points.push(candidate);
        }
    }
    points.sort_by(|a, b| a.total_cmp(b));
    points.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    points
}

/// Pulls a handle position to the nearest snap point that is within a few pixels of it. `width_px` is the length of the track.
pub fn snap_position(position: f64, width_px: f64, limits: Limits) -> f64 {
    let p = if position.is_finite() { position.clamp(0.0, 1.0) } else if limits.can_zoom_out() { 0.5 } else { 0.0 };
    let width = width_px.max(1.0);
    let mut best: Option<(f64, f64)> = None;
    for zoom in snap_zooms(limits) {
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
    fn the_middle_is_the_reset_and_the_ends_are_the_limits() {
        assert!(close(position_of(1.0, L), 0.5));
        assert!(close(position_of(L.min, L), 0.0));
        assert!(close(position_of(L.max, L), 1.0));
        assert!(close(zoom_at(0.5, L), 1.0));
        assert!(close(zoom_at(0.0, L), L.min));
        assert!(close(zoom_at(1.0, L), L.max));
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
        for i in 0..=50 {
            let p = f64::from(i) / 50.0;
            assert!((position_of(zoom_at(p, l), l) - p).abs() < 1e-9);
        }
        let points = snap_zooms(l);
        assert!(close(points[0], 1.0) && points.iter().all(|p| *p >= 1.0));
        assert!(close(snap_position(4.0 / 200.0, 200.0, l), 0.0), "the reset pulls at the left end");
        assert!(close(step(1.0, false, 0.1, l), 1.0), "no step below the fit");
        assert!(step(1.0, true, 0.1, l) > 1.0);
    }

    #[test]
    fn position_and_zoom_are_inverse_and_monotonic() {
        let mut last = -1.0;
        for i in 0..=100 {
            let p = f64::from(i) / 100.0;
            let z = zoom_at(p, L);
            assert!(z > last, "monotonic at {p}");
            last = z;
            assert!((position_of(z, L) - p).abs() < 1e-9, "round trip at {p}");
        }
        assert!(close(zoom_at(f64::NAN, L), 1.0));
        assert!(close(zoom_at(7.0, L), L.max));
        assert!(close(zoom_at(-7.0, L), L.min));
    }

    #[test]
    fn other_limits_keep_the_reset_in_the_middle() {
        let l = Limits::new(0.05, 32.0);
        assert!(close(position_of(1.0, l), 0.5));
        assert!(close(zoom_at(1.0, l), l.max));
        assert!(snap_zooms(l).contains(&32.0));
    }

    #[test]
    fn the_snap_points_are_what_the_user_expects() {
        let points = snap_zooms(L);
        for expected in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0, 3.0, 4.0, 5.0, 8.0] {
            assert!(points.iter().any(|p| close(*p, expected)), "{expected}");
        }
        assert!(points.windows(2).all(|w| w[0] < w[1]));
        assert!(close(points[0], L.min) && close(*points.last().unwrap(), L.max));
        // Nothing above the limit.
        assert!(snap_zooms(Limits::new(0.1, 3.0)).iter().all(|p| *p <= 3.0 + 1e-9));
    }

    #[test]
    fn the_handle_is_pulled_to_the_reset_and_to_the_other_points_only_when_close() {
        let width = 200.0;
        // 4 px next to the middle snaps to the reset.
        assert!(close(snap_position(0.5 + 4.0 / width, width, L), 0.5));
        assert!(close(snap_position(0.5 - 6.0 / width, width, L), 0.5));
        // Between the reset and 75% the nearer point wins.
        let near_75 = position_of(0.75, L) + 2.0 / width;
        assert!(close(snap_position(near_75, width, L), position_of(0.75, L)));
        // 15 px away does not.
        let free = 0.5 + 15.0 / width;
        assert!(close(snap_position(free, width, L), free) || snap_zooms(L).iter().any(|z| (position_of(*z, L) - free).abs() * width < 5.0));
        // 50% is a snap point too.
        let at_50 = position_of(0.5, L);
        assert!(close(snap_position(at_50 + 3.0 / width, width, L), at_50));
        // A position far from every point is left alone.
        let p = (position_of(2.0, L) + position_of(3.0, L)) / 2.0;
        assert!(close(snap_position(p, width, L), p));
        // The ends snap, a broken value does not break.
        assert!(close(snap_position(0.999, width, L), 1.0));
        assert!(close(snap_position(f64::NAN, width, L), 0.5));
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
