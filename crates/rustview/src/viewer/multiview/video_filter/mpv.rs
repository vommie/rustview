//! The video filters as mpv sees them (pure): properties to set and `RustView`'s labeled filters in `vf` and `af`. The
//! rotation is mpv's own `video-rotate` (added to the rotation the file asks for; mpv reports the sum in
//! `video-params/rotate`, so the tile is laid out anew), the picture adjustments are properties of the GPU renderer,
//! deinterlacing is mpv's `deinterlace`, and the volume normalization the same `af` graphs as `RustCut`'s player.
//!
//! Mirroring works like in `RustCut`: a libavfilter flip in `vf`. Such a filter needs decoded frames in memory, so while it is
//! on the player decodes with `hwdec=auto-copy` (copy-back, `RustCut`'s default); without it the faster `auto-safe` (no copy)
//! is back. `vf` sees the frame before any rotation, so the flip is `vflip` when the picture is turned by a quarter: on screen
//! the mirror always swaps left and right.

use super::{Knob, Normalize, VideoFilters};

/// The labels of `RustView`'s filters in mpv's chains: they are added and removed by label (`vf add`, `vf remove`), so the
/// filters of the user's mpv.conf stay.
pub const VF_LABEL: &str = "@rv-mirror";
pub const AF_LABEL: &str = "@rv-norm";

/// The hardware decoding a player starts with (`mpv_engine::options`): no copy back, the fastest.
pub const HWDEC_DEFAULT: &str = "auto-safe";

/// The hardware decoding while a filter of `vf` is on: frames are copied back, so libavfilter can work on them.
pub const HWDEC_FOR_VF: &str = "auto-copy";

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

/// What the settings depend on besides the filters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MpvContext {
    /// The normalization of Settings > Multiview > Player (a video without its own choice plays it).
    pub default_normalize: Normalize,
    /// The rotation the file asks for (mpv's `video-dec-params/rotate`), in degrees.
    pub file_rotation: i64,
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

/// The video filter of `RustView` (`None` = none): the flip that mirrors the picture on screen.
pub fn vf(filters: &VideoFilters, context: MpvContext) -> Option<String> {
    if !filters.mirror {
        return None;
    }
    let turned = (context.file_rotation + i64::from(filters.rotation())).rem_euclid(180) == 90;
    Some(format!("lavfi=[{}]", if turned { "vflip" } else { "hflip" }))
}

/// The audio filter of `RustView` (`None` = none).
pub fn af(filters: &VideoFilters, context: MpvContext) -> Option<String> {
    filters
        .normalize_or(context.default_normalize)
        .graph()
        .map(|graph| format!("lavfi=[{graph}]"))
}

/// Every setting of `filters`, in the order mpv gets them.
pub fn settings(filters: &VideoFilters, context: MpvContext) -> Vec<MpvSetting> {
    let vf = vf(filters, context);
    let mut all = vec![
        MpvSetting::Int("video-rotate", i64::from(filters.rotation())),
        MpvSetting::Text("deinterlace", if filters.deinterlace { "yes" } else { "no" }.to_owned()),
        MpvSetting::Text(
            "hwdec",
            if vf.is_some() { HWDEC_FOR_VF } else { HWDEC_DEFAULT }.to_owned(),
        ),
        MpvSetting::Chain {
            chain: "vf",
            label: VF_LABEL,
            filter: vf,
            present: false,
        },
        MpvSetting::Chain {
            chain: "af",
            label: AF_LABEL,
            filter: af(filters, context),
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

/// What changed from `before` to `after` (only those settings are sent, so a slider drag sets one property). The decoding
/// goes back to the fastest only after the flip left `vf`.
pub fn changes(before: (&VideoFilters, MpvContext), after: (&VideoFilters, MpvContext)) -> Vec<MpvSetting> {
    let old = settings(before.0, before.1);
    let mut changed: Vec<MpvSetting> = settings(after.0, after.1)
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
        .collect();
    if let Some(at) = changed
        .iter()
        .position(|s| *s == MpvSetting::Text("hwdec", HWDEC_DEFAULT.to_owned()))
    {
        let restore = changed.remove(at);
        changed.push(restore);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: MpvContext = MpvContext {
        default_normalize: Normalize::Off,
        file_rotation: 0,
    };

    #[test]
    fn the_mirror_flips_left_and_right_on_screen_and_normalization_follows_the_default() {
        let mut filters = VideoFilters::default();
        assert_eq!(vf(&filters, PLAIN), None);
        assert_eq!(af(&filters, PLAIN), None);
        filters.mirror = true;
        assert_eq!(vf(&filters, PLAIN).as_deref(), Some("lavfi=[hflip]"));
        filters.rotate(90);
        assert_eq!(vf(&filters, PLAIN).as_deref(), Some("lavfi=[vflip]"));
        // A phone video the file turns by a quarter, turned once more: upright again, so left and right.
        let phone = MpvContext {
            file_rotation: 90,
            ..PLAIN
        };
        assert_eq!(vf(&filters, phone).as_deref(), Some("lavfi=[hflip]"));
        let dynamic = MpvContext {
            default_normalize: Normalize::Dynamic,
            ..PLAIN
        };
        assert_eq!(
            af(&filters, dynamic).as_deref(),
            Some("lavfi=[dynaudnorm=f=250:g=11:p=0.9:m=8]")
        );
        filters.normalize = Some(Normalize::Off);
        assert_eq!(af(&filters, dynamic), None, "this video's choice wins");
    }

    #[test]
    fn only_what_changed_is_sent() {
        let before = VideoFilters::default();
        let mut after = before;
        assert_eq!(changes((&before, PLAIN), (&after, PLAIN)), []);
        after.rotate(90);
        after.set_knob(Knob::Gamma, 20);
        assert_eq!(
            changes((&before, PLAIN), (&after, PLAIN)),
            [MpvSetting::Int("video-rotate", 90), MpvSetting::Int("gamma", 20)]
        );
        let mut deint = before;
        deint.deinterlace = true;
        assert_eq!(
            changes((&before, PLAIN), (&deint, PLAIN)),
            [MpvSetting::Text("deinterlace", "yes".into())]
        );
    }

    #[test]
    fn the_flip_copies_frames_back_while_it_is_on() {
        let before = VideoFilters::default();
        let mut mirrored = before;
        mirrored.mirror = true;
        assert_eq!(
            changes((&before, PLAIN), (&mirrored, PLAIN)),
            [
                MpvSetting::Text("hwdec", HWDEC_FOR_VF.into()),
                MpvSetting::Chain {
                    chain: "vf",
                    label: VF_LABEL,
                    filter: Some("lavfi=[hflip]".into()),
                    present: false
                }
            ]
        );
        let back = changes((&mirrored, PLAIN), (&before, PLAIN));
        assert!(matches!(
            back[0],
            MpvSetting::Chain {
                present: true,
                filter: None,
                ..
            }
        ));
        assert_eq!(
            back[1],
            MpvSetting::Text("hwdec", HWDEC_DEFAULT.into()),
            "the fast decoding comes back last"
        );
    }

    #[test]
    fn a_new_default_reaches_videos_without_their_own_choice() {
        let filters = VideoFilters::default();
        let loud = MpvContext {
            default_normalize: Normalize::Loudness,
            ..PLAIN
        };
        assert!(matches!(
            changes((&filters, PLAIN), (&filters, loud))[..],
            [MpvSetting::Chain {
                chain: "af",
                present: false,
                filter: Some(_),
                ..
            }]
        ));
        let mut own = filters;
        own.normalize = Some(Normalize::Speech);
        assert_eq!(changes((&own, PLAIN), (&own, loud)), []);
    }

    #[test]
    fn every_normalization_but_off_has_a_graph() {
        for normalize in Normalize::ALL {
            assert_eq!(normalize.graph().is_none(), normalize == Normalize::Off);
        }
    }
}
