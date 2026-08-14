// Standardized MPV Property Identifiers
pub const PROP_PAUSE: &str = "pause";
pub const PROP_TIME_POS: &str = "time-pos";
pub const PROP_DURATION: &str = "duration";
pub const PROP_VOLUME: &str = "volume";
pub const PROP_MUTE: &str = "mute";
pub const PROP_HWDEC: &str = "hwdec";
pub const PROP_VO: &str = "vo";
pub const PROP_IDLE: &str = "idle";
pub const PROP_KEEP_OPEN: &str = "keep-open";
pub const PROP_MEDIA_TITLE: &str = "media-title";
pub const PROP_EOF_REACHED: &str = "eof-reached";
pub const PROP_PERCENT_POS: &str = "percent-pos";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwDecMode {
    AutoSafe,
    Auto,
    Vaapi,
    Vdpau,
    Nvdec,
    Disabled,
}

impl HwDecMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::AutoSafe => "auto-safe",
            Self::Auto => "auto",
            Self::Vaapi => "vaapi",
            Self::Vdpau => "vdpau",
            Self::Nvdec => "nvdec",
            Self::Disabled => "no",
        }
    }
}
