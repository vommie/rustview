//! Which formats the user has switched on in Settings > File Formats.
//!
//! The settings file stores the DISABLED ids, so a format added in a later version is on by default. The process-wide
//! policy is set by the `SettingsManager` whenever settings load or change; code that decides about many files takes one
//! `FormatMask` snapshot and asks that, so one scan or one list never sees two different policies.

use super::formats::MediaFormat;
use parking_lot::RwLock;

/// A set of disabled formats, one bit per `MediaFormat` index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FormatMask {
    disabled: u128,
}

const _: () = assert!(MediaFormat::COUNT <= 128, "FormatMask holds at most 128 formats");

impl FormatMask {
    /// Every known format on.
    pub const ALL_ENABLED: Self = Self { disabled: 0 };

    /// Builds a mask from settings ids; ids that no format has (a hand-edited or newer file) are ignored.
    pub fn from_disabled_ids<I, S>(ids: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut mask = Self::ALL_ENABLED;
        for id in ids {
            if let Some(format) = MediaFormat::from_id(id.as_ref()) {
                mask = mask.with(format, false);
            }
        }
        mask
    }

    /// Unknown is never enabled; every other format is enabled unless its bit is set.
    #[inline]
    pub fn is_enabled(self, format: MediaFormat) -> bool {
        format.is_supported() && (self.disabled >> (format as u32)) & 1 == 0
    }

    /// Whether a library record of this format is shown. Records of `Unknown` (written by old versions or renamed to an
    /// unknown extension) belong to no switch and stay visible as they always were.
    #[inline]
    pub fn shows(self, format: MediaFormat) -> bool {
        !format.is_supported() || self.is_enabled(format)
    }

    #[must_use]
    pub fn with(self, format: MediaFormat, enabled: bool) -> Self {
        let bit = 1u128 << (format as u32);
        Self {
            disabled: if enabled {
                self.disabled & !bit
            } else {
                self.disabled | bit
            },
        }
    }

    pub fn is_all_enabled(self) -> bool {
        self.disabled == 0
    }

    /// The disabled formats in table order.
    pub fn disabled_formats(self) -> Vec<MediaFormat> {
        MediaFormat::all().filter(|f| !self.is_enabled(*f)).collect()
    }

    /// The settings ids of the disabled formats in table order (what is written to the settings file).
    pub fn disabled_ids(self) -> Vec<String> {
        self.disabled_formats()
            .into_iter()
            .map(|f| f.spec().id.to_string())
            .collect()
    }

    /// Formats whose state differs between the two masks, with their state in `self`.
    pub fn changes_to(self, other: Self) -> Vec<(MediaFormat, bool)> {
        MediaFormat::all()
            .filter(|f| self.is_enabled(*f) != other.is_enabled(*f))
            .map(|f| (f, other.is_enabled(f)))
            .collect()
    }
}

static POLICY: RwLock<FormatMask> = RwLock::new(FormatMask::ALL_ENABLED);

/// The process-wide format policy (set from the settings, read everywhere files or records are filtered).
pub struct FormatPolicy;

impl FormatPolicy {
    #[inline]
    pub fn mask() -> FormatMask {
        *POLICY.read()
    }

    #[inline]
    pub fn is_enabled(format: MediaFormat) -> bool {
        POLICY.read().is_enabled(format)
    }

    /// Makes `mask` the policy and returns the previous one.
    pub fn set(mask: FormatMask) -> FormatMask {
        std::mem::replace(&mut *POLICY.write(), mask)
    }

    /// Applies the `formats.disabled_formats` setting.
    pub fn apply_disabled_ids<I, S>(ids: I) -> FormatMask
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self::set(FormatMask::from_disabled_ids(ids))
    }
}
