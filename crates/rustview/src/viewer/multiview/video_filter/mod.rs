//! The video filters of one video tile (pure): rotation in quarter turns, mirroring, the picture adjustments (gamma,
//! brightness, contrast, saturation), deinterlacing and the volume normalization. They change only how `RustView` plays the
//! video, never the file, and live as long as the tile (user decision 2026-11-05, questionnaire
//! `2026-11-05-rustview-videofilter`: nothing is remembered; a duplicated tile takes the filters of its source).
//!
//! `mpv` turns them into mpv properties and filter chains, `menu` into the entries of the filter menu.

pub mod menu;
pub mod mpv;

use std::sync::atomic::{AtomicU8, Ordering};

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

    /// The name in settings.json (`fullscreen.player.normalize`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Loudness => "loudness",
            Self::Dynamic => "dynamic",
            Self::Speech => "speech",
            Self::Compressor => "compressor",
        }
    }

    /// The normalization of a name of settings.json (`None`: unknown).
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|normalize| normalize.name() == name)
    }

    const fn index(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Loudness => 1,
            Self::Dynamic => 2,
            Self::Speech => 3,
            Self::Compressor => 4,
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
    /// Left and right swapped (combines with the rotation into every orientation).
    pub mirror: bool,
    knobs: [i32; 4],
    pub deinterlace: bool,
    /// The normalization chosen for this video (`None`: the default of Settings > Multiview > Player, `default_normalize`).
    pub normalize: Option<Normalize>,
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

    /// The picture turned by `degrees` clockwise from how the file shows it (snapped to quarter turns).
    pub fn set_rotation(&mut self, degrees: u16) {
        self.rotation = 0;
        self.rotate(i32::from(degrees));
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

    /// How many picture adjustments are not 0.
    pub fn adjusted_knobs(&self) -> usize {
        self.knobs.iter().filter(|&&value| value != 0).count()
    }

    /// The normalization that plays: this video's choice, else `default`.
    pub fn normalize_or(&self, default: Normalize) -> Normalize {
        self.normalize.unwrap_or(default)
    }

    /// This video plays another normalization than the default.
    pub fn normalize_overridden(&self, default: Normalize) -> bool {
        self.normalize.is_some_and(|normalize| normalize != default)
    }

    /// How many filters differ from how the video plays without any (each knob counts once, the normalization only when
    /// this video overrides the default): the number on the filter button.
    pub fn active_count(&self, default: Normalize) -> usize {
        usize::from(self.rotation != 0)
            + usize::from(self.mirror)
            + self.adjusted_knobs()
            + usize::from(self.deinterlace)
            + usize::from(self.normalize_overridden(default))
    }

    /// Nothing differs from how the video plays without filters.
    pub fn is_plain(&self, default: Normalize) -> bool {
        self.active_count(default) == 0
    }
}

/// What a reset button (or a key) puts back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterPart {
    Rotation,
    Mirror,
    Deinterlace,
    Knob(Knob),
    /// Every picture adjustment.
    Picture,
    /// The default normalization again.
    Normalize,
    /// Rotation, mirror, deinterlacing and the picture adjustments.
    Image,
    All,
}

/// A change of the filters: from the filter menu, the "..." menu or a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterChange {
    /// Degrees, a multiple of 90 (negative = counterclockwise).
    Rotate(i32),
    /// A rotation entry of the menu: this rotation, or none when it is the one shown.
    ToggleRotation(u16),
    ToggleMirror,
    ToggleDeinterlace,
    SetKnob(Knob, i32),
    /// The gamma key: up by `GAMMA_KEY_STEP`, after the top back to 0.
    CycleGamma,
    Normalize(Normalize),
    Reset(FilterPart),
}

impl FilterChange {
    pub fn apply(self, filters: &mut VideoFilters) {
        match self {
            Self::Rotate(degrees) => filters.rotate(degrees),
            Self::ToggleRotation(degrees) => {
                let shown = filters.rotation() == degrees;
                filters.set_rotation(if shown { 0 } else { degrees });
            }
            Self::ToggleMirror => filters.mirror = !filters.mirror,
            Self::ToggleDeinterlace => filters.deinterlace = !filters.deinterlace,
            Self::SetKnob(knob, value) => filters.set_knob(knob, value),
            Self::CycleGamma => filters.cycle_gamma(),
            Self::Normalize(normalize) => filters.normalize = Some(normalize),
            Self::Reset(part) => part.reset(filters),
        }
    }
}

impl FilterPart {
    fn reset(self, filters: &mut VideoFilters) {
        match self {
            Self::Rotation => filters.set_rotation(0),
            Self::Mirror => filters.mirror = false,
            Self::Deinterlace => filters.deinterlace = false,
            Self::Knob(knob) => filters.set_knob(knob, 0),
            Self::Picture => filters.knobs = [0; 4],
            Self::Normalize => filters.normalize = None,
            Self::Image => {
                let normalize = filters.normalize;
                *filters = VideoFilters {
                    normalize,
                    ..VideoFilters::default()
                };
            }
            Self::All => *filters = VideoFilters::default(),
        }
    }
}

static DEFAULT_NORMALIZE: AtomicU8 = AtomicU8::new(0);

/// The normalization every video starts with (Settings > Multiview > Player), set at startup and when the setting changes.
pub fn set_default_normalize(normalize: Normalize) {
    DEFAULT_NORMALIZE.store(normalize.index(), Ordering::Relaxed);
}

/// Settings > Multiview > Player changed the default normalization to `name` (or the app starts): every video of `instances`
/// without a choice of its own plays the new one at once.
pub fn apply_default_normalize(name: &str, instances: &[super::instance::SharedInstance]) {
    let before = default_normalize();
    let after = Normalize::from_name(name).unwrap_or_default();
    set_default_normalize(after);
    if after == before {
        return;
    }
    tracing::info!(
        from = before.name(),
        to = after.name(),
        "[Multiview: Filters] Default normalization"
    );
    for instance in instances {
        instance.lock().default_normalize_changed(before);
    }
}

pub fn default_normalize() -> Normalize {
    let index = usize::from(DEFAULT_NORMALIZE.load(Ordering::Relaxed));
    Normalize::ALL.get(index).copied().unwrap_or_default()
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
        assert!(filters.is_plain(Normalize::Off));
        assert_eq!(filters.active_count(Normalize::Off), 0);
        filters.set_knob(Knob::Contrast, 250);
        assert_eq!(filters.knob(Knob::Contrast), 100);
        filters.set_knob(Knob::Saturation, -250);
        assert_eq!(filters.knob(Knob::Saturation), -100);
        filters.rotate(90);
        filters.mirror = true;
        filters.deinterlace = true;
        filters.normalize = Some(Normalize::Speech);
        assert_eq!(filters.active_count(Normalize::Off), 6);
        // The default's own choice is no filter of this video.
        assert_eq!(filters.active_count(Normalize::Speech), 5);
        FilterChange::Reset(FilterPart::Picture).apply(&mut filters);
        assert_eq!(filters.active_count(Normalize::Off), 4);
        FilterChange::ToggleMirror.apply(&mut filters);
        assert!(!filters.mirror);
        FilterChange::Reset(FilterPart::Image).apply(&mut filters);
        assert_eq!(
            filters.normalize,
            Some(Normalize::Speech),
            "the image reset keeps the sound"
        );
        FilterChange::Reset(FilterPart::All).apply(&mut filters);
        assert!(filters.is_plain(Normalize::Dynamic));
    }

    #[test]
    fn a_rotation_entry_turns_to_its_rotation_or_back() {
        let mut filters = VideoFilters::default();
        FilterChange::ToggleRotation(180).apply(&mut filters);
        assert_eq!(filters.rotation(), 180);
        FilterChange::ToggleRotation(270).apply(&mut filters);
        assert_eq!(filters.rotation(), 270);
        FilterChange::ToggleRotation(270).apply(&mut filters);
        assert_eq!(filters.rotation(), 0);
        FilterChange::Rotate(90).apply(&mut filters);
        FilterChange::Reset(FilterPart::Rotation).apply(&mut filters);
        assert_eq!(filters.rotation(), 0);
    }

    #[test]
    fn the_default_normalization_has_names_and_is_global() {
        for normalize in Normalize::ALL {
            assert_eq!(Normalize::from_name(normalize.name()), Some(normalize));
        }
        assert_eq!(Normalize::from_name("loud"), None);
        let mut filters = VideoFilters::default();
        assert_eq!(filters.normalize_or(Normalize::Dynamic), Normalize::Dynamic);
        filters.normalize = Some(Normalize::Dynamic);
        assert!(!filters.normalize_overridden(Normalize::Dynamic));
        assert!(filters.normalize_overridden(Normalize::Off));
    }
}
