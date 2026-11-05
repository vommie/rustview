//! The video filters of one video tile (pure): rotation in quarter turns, mirroring, the picture adjustments (gamma,
//! brightness, contrast, saturation), deinterlacing and the volume normalization. They change only how `RustView` plays the
//! video, never the file, and live as long as the tile (user decision 2026-11-05, questionnaire
//! `2026-11-05-rustview-videofilter`: nothing is remembered; a duplicated tile takes the filters of its source).
//!
//! `mpv` turns them into mpv properties and filter chains, `menu` into the entries of the filter menu.

pub mod menu;
pub mod mpv;

/// The volume normalization (the four kinds of `RustCut`, plus off).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Normalize {
    #[default]
    Off,
    /// Every video equally loud (EBU R128).
    Loudness,
    /// Quiet parts louder.
    Dynamic,
    /// Voices clear and even.
    Speech,
    /// Peaks quieter.
    Compressor,
}

impl Normalize {
    pub const ALL: [Self; 5] = [Self::Off, Self::Loudness, Self::Dynamic, Self::Speech, Self::Compressor];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Loudness => "Loudness",
            Self::Dynamic => "Dynamic",
            Self::Speech => "Speech",
            Self::Compressor => "Compressor",
        }
    }

    pub const fn tip(self) -> &'static str {
        match self {
            Self::Off => "The sound as it is in the file",
            Self::Loudness => "Every video equally loud (EBU R128)",
            Self::Dynamic => "Quiet parts louder, loud parts as they are",
            Self::Speech => "Voices clear and even",
            Self::Compressor => "Loud peaks quieter",
        }
    }
}

/// The picture adjustments, each from -100 to 100 (0 = as in the file), as mpv takes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Knob {
    Gamma,
    Brightness,
    Contrast,
    Saturation,
}

impl Knob {
    pub const ALL: [Self; 4] = [Self::Gamma, Self::Brightness, Self::Contrast, Self::Saturation];

    pub const fn title(self) -> &'static str {
        match self {
            Self::Gamma => "Gamma",
            Self::Brightness => "Brightness",
            Self::Contrast => "Contrast",
            Self::Saturation => "Saturation",
        }
    }

    /// The mpv property of the knob.
    pub const fn property(self) -> &'static str {
        match self {
            Self::Gamma => "gamma",
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
            Self::Saturation => "saturation",
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::Gamma => 0,
            Self::Brightness => 1,
            Self::Contrast => 2,
            Self::Saturation => 3,
        }
    }
}

/// The lowest and highest value of a knob.
pub const KNOB_RANGE: i32 = 100;

/// One press of the gamma key raises gamma by this much (and wraps to 0 after the top).
pub const GAMMA_KEY_STEP: i32 = 10;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct VideoFilters {
    /// Clockwise, in degrees: 0, 90, 180 or 270 (added to the rotation the file asks for).
    rotation: u16,
    /// Left and right swapped (before the rotation, so both combine into every orientation).
    pub mirror: bool,
    knobs: [i32; 4],
    pub deinterlace: bool,
    pub normalize: Normalize,
}

impl VideoFilters {
    pub fn rotation(&self) -> u16 {
        self.rotation
    }

    /// Turns the picture by `degrees` (a multiple of 90; negative = counterclockwise).
    pub fn rotate(&mut self, degrees: i32) {
        let turned = (i32::from(self.rotation) + degrees).rem_euclid(360);
        // Quarter turns only: anything else snaps to the quarter below.
        self.rotation = u16::try_from(turned - turned % 90).unwrap_or(0);
    }

    pub fn knob(&self, knob: Knob) -> i32 {
        self.knobs[knob.index()]
    }

    pub fn set_knob(&mut self, knob: Knob, value: i32) {
        self.knobs[knob.index()] = value.clamp(-KNOB_RANGE, KNOB_RANGE);
    }

    /// The gamma key: up by `GAMMA_KEY_STEP` to the next step, after the top back to 0 (a value between steps goes to the
    /// next step, a negative one to 0).
    pub fn cycle_gamma(&mut self) {
        let gamma = self.knob(Knob::Gamma);
        let next = if (0..KNOB_RANGE).contains(&gamma) {
            (gamma / GAMMA_KEY_STEP + 1) * GAMMA_KEY_STEP
        } else {
            0
        };
        self.set_knob(Knob::Gamma, next);
    }

    pub fn reset_picture(&mut self) {
        self.knobs = [0; 4];
    }

    /// How many filters are on (each knob that is not 0 counts once): the number on the filter button.
    pub fn active_count(&self) -> usize {
        usize::from(self.rotation != 0)
            + usize::from(self.mirror)
            + self.knobs.iter().filter(|&&value| value != 0).count()
            + usize::from(self.deinterlace)
            + usize::from(self.normalize != Normalize::Off)
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// A change of the filters: from the filter menu, the "..." menu or a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterChange {
    /// Degrees, a multiple of 90 (negative = counterclockwise).
    Rotate(i32),
    ToggleMirror,
    ToggleDeinterlace,
    SetKnob(Knob, i32),
    /// The gamma key: up by `GAMMA_KEY_STEP`, after the top back to 0.
    CycleGamma,
    ResetPicture,
    Normalize(Normalize),
    AllOff,
}

impl FilterChange {
    pub fn apply(self, filters: &mut VideoFilters) {
        match self {
            Self::Rotate(degrees) => filters.rotate(degrees),
            Self::ToggleMirror => filters.mirror = !filters.mirror,
            Self::ToggleDeinterlace => filters.deinterlace = !filters.deinterlace,
            Self::SetKnob(knob, value) => filters.set_knob(knob, value),
            Self::CycleGamma => filters.cycle_gamma(),
            Self::ResetPicture => filters.reset_picture(),
            Self::Normalize(normalize) => filters.normalize = normalize,
            Self::AllOff => *filters = VideoFilters::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_goes_round_in_quarter_turns() {
        let mut filters = VideoFilters::default();
        let mut seen = Vec::new();
        for _ in 0..5 {
            filters.rotate(90);
            seen.push(filters.rotation());
        }
        assert_eq!(seen, [90, 180, 270, 0, 90]);
        filters.rotate(-90);
        assert_eq!(filters.rotation(), 0);
        filters.rotate(-90);
        assert_eq!(filters.rotation(), 270);
        filters.rotate(180);
        assert_eq!(filters.rotation(), 90);
    }

    #[test]
    fn the_gamma_key_climbs_in_steps_and_wraps_to_zero() {
        let mut filters = VideoFilters::default();
        let mut seen = Vec::new();
        for _ in 0..12 {
            filters.cycle_gamma();
            seen.push(filters.knob(Knob::Gamma));
        }
        assert_eq!(seen, [10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 0, 10]);
        filters.set_knob(Knob::Gamma, 37);
        filters.cycle_gamma();
        assert_eq!(filters.knob(Knob::Gamma), 40);
        filters.set_knob(Knob::Gamma, -20);
        filters.cycle_gamma();
        assert_eq!(filters.knob(Knob::Gamma), 0);
    }

    #[test]
    fn knobs_stay_in_range_and_every_filter_counts_once() {
        let mut filters = VideoFilters::default();
        assert!(filters.is_default());
        assert_eq!(filters.active_count(), 0);
        filters.set_knob(Knob::Contrast, 250);
        assert_eq!(filters.knob(Knob::Contrast), 100);
        filters.set_knob(Knob::Saturation, -250);
        assert_eq!(filters.knob(Knob::Saturation), -100);
        filters.rotate(90);
        filters.mirror = true;
        filters.deinterlace = true;
        filters.normalize = Normalize::Speech;
        assert_eq!(filters.active_count(), 6);
        filters.reset_picture();
        assert_eq!(filters.active_count(), 4);
        FilterChange::ToggleMirror.apply(&mut filters);
        assert!(!filters.mirror);
        FilterChange::AllOff.apply(&mut filters);
        assert!(filters.is_default());
    }
}
