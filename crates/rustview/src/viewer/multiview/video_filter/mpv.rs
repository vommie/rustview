//! The video filters as mpv sees them (pure): properties to set and the filter chains of `vf` and `af`. The rotation is
//! mpv's own `video-rotate` (added to the rotation the file asks for; the displayed size swaps, so the tile is laid out
//! anew), mirroring the libavfilter `hflip` in `vf` (before the rotation), the picture adjustments are properties of the GPU
//! renderer, deinterlacing is mpv's `deinterlace`, and the volume normalization the same `af` graphs as `RustCut`'s player.

use super::{Knob, Normalize, VideoFilters};

/// The labels of `RustView`'s filters in mpv's chains: they are added and removed by label (`vf add`, `vf remove`), so the
/// filters of the user's mpv.conf stay.
pub const VF_LABEL: &str = "@rv-mirror";
pub const AF_LABEL: &str = "@rv-norm";

/// One thing mpv gets.
#[derive(Debug, Clone, PartialEq)]
pub enum MpvSetting {
    Int(&'static str, i64),
    Text(&'static str, String),
    /// The labeled filter of a chain (`vf` or `af`): removed, then added again when there is one.
    Chain {
        chain: &'static str,
        label: &'static str,
        filter: Option<String>,
        /// The chain has `RustView`'s filter now (it is removed first; mpv warns about removing a label it does not have).
        present: bool,
    },
}

impl Normalize {
    /// The libavfilter graph of the normalization (`RustCut`'s `Normalization::graph`).
    pub const fn graph(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Loudness => Some("loudnorm=I=-22:TP=-1.5:LRA=2,aresample=48000"),
            Self::Dynamic => Some("dynaudnorm=f=250:g=11:p=0.9:m=8"),
            Self::Speech => Some("speechnorm=e=6.25:r=0.0001:l=1"),
            Self::Compressor => Some("acompressor=threshold=-24dB:ratio=4:attack=5:release=250:makeup=6dB"),
        }
    }
}

/// The video filter of `RustView` (`None` = none).
pub fn vf(filters: &VideoFilters) -> Option<String> {
    filters.mirror.then(|| "lavfi=[hflip]".to_owned())
}

/// The audio filter of `RustView` (`None` = none).
pub fn af(filters: &VideoFilters) -> Option<String> {
    filters.normalize.graph().map(|graph| format!("lavfi=[{graph}]"))
}

/// Every setting of `filters`.
pub fn settings(filters: &VideoFilters) -> Vec<MpvSetting> {
    let mut all = vec![
        MpvSetting::Int("video-rotate", i64::from(filters.rotation())),
        MpvSetting::Text("deinterlace", if filters.deinterlace { "yes" } else { "no" }.to_owned()),
        MpvSetting::Chain {
            chain: "vf",
            label: VF_LABEL,
            filter: vf(filters),
            present: false,
        },
        MpvSetting::Chain {
            chain: "af",
            label: AF_LABEL,
            filter: af(filters),
            present: false,
        },
    ];
    all.extend(
        Knob::ALL
            .into_iter()
            .map(|knob| MpvSetting::Int(knob.property(), i64::from(filters.knob(knob)))),
    );
    all
}

/// What changed from `before` to `after` (only those settings are sent, so a slider drag sets one property).
pub fn changes(before: &VideoFilters, after: &VideoFilters) -> Vec<MpvSetting> {
    let old = settings(before);
    settings(after)
        .into_iter()
        .zip(old)
        .filter(|(new, old)| new != old)
        .map(|(new, old)| match (new, old) {
            (
                MpvSetting::Chain {
                    chain, label, filter, ..
                },
                MpvSetting::Chain { filter: had, .. },
            ) => MpvSetting::Chain {
                chain,
                label,
                filter,
                present: had.is_some(),
            },
            (new, _) => new,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_and_normalization_are_labeled_chains() {
        let mut filters = VideoFilters::default();
        assert_eq!(vf(&filters), None);
        assert_eq!(af(&filters), None);
        filters.mirror = true;
        filters.normalize = Normalize::Dynamic;
        assert_eq!(vf(&filters).as_deref(), Some("lavfi=[hflip]"));
        assert_eq!(af(&filters).as_deref(), Some("lavfi=[dynaudnorm=f=250:g=11:p=0.9:m=8]"));
    }

    #[test]
    fn only_what_changed_is_sent() {
        let before = VideoFilters::default();
        let mut after = before;
        assert_eq!(changes(&before, &after), []);
        after.rotate(90);
        after.set_knob(Knob::Gamma, 20);
        assert_eq!(
            changes(&before, &after),
            [MpvSetting::Int("video-rotate", 90), MpvSetting::Int("gamma", 20)]
        );
        let mut deint = before;
        deint.deinterlace = true;
        assert_eq!(
            changes(&before, &deint),
            [MpvSetting::Text("deinterlace", "yes".into())]
        );
        let mut mirrored = before;
        mirrored.mirror = true;
        assert_eq!(
            changes(&before, &mirrored),
            [MpvSetting::Chain {
                chain: "vf",
                label: VF_LABEL,
                filter: Some("lavfi=[hflip]".into()),
                present: false
            }]
        );
        assert!(matches!(
            changes(&mirrored, &before)[..],
            [MpvSetting::Chain {
                present: true,
                filter: None,
                ..
            }]
        ));
    }

    #[test]
    fn every_normalization_but_off_has_a_graph() {
        for normalize in Normalize::ALL {
            assert_eq!(normalize.graph().is_none(), normalize == Normalize::Off);
        }
    }
}
