//! The one table of every media format `RustView` knows: stable id, names, extensions, kind, decoder and the order of the
//! Settings page. Everything else (scanner, tree walker, detector, Settings page) reads this table.

use super::formats::{MediaFormat, MediaType};

/// How a format is decoded; shown on the Settings page so users know what a format depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FormatDecoder {
    /// Decoded in-process by the `image` crate (thumbnails and viewer); `FFmpeg` is the fallback for damaged variants.
    Native,
    /// Decoded through `FFmpeg`'s libavcodec (thumbnails and viewer).
    FFmpeg,
    /// Thumbnails and metadata through `FFmpeg` (or mpv), playback through mpv.
    Player,
}

impl FormatDecoder {
    pub fn label(self) -> &'static str {
        match self {
            Self::Native => "Built-in decoder",
            Self::FFmpeg => "FFmpeg",
            Self::Player => "FFmpeg + mpv",
        }
    }
}

/// How common a format is; only used to order and group the Settings page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FormatGroup {
    Common,
    Specialized,
    Legacy,
}

impl FormatGroup {
    pub fn title(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Specialized => "Professional and specialized",
            Self::Legacy => "Legacy and rare",
        }
    }
}

#[derive(Debug)]
pub struct FormatSpec {
    pub format: MediaFormat,
    /// Stable key stored in the settings file (`formats.disabled_formats`); never rename one.
    pub id: &'static str,
    /// Short uppercase name for columns and badges.
    pub name: &'static str,
    /// Full name for the Settings page.
    pub long_name: &'static str,
    pub media_type: Option<MediaType>,
    /// Lower-case extensions that select this format.
    pub extensions: &'static [&'static str],
    /// Extension appended to extensionless files of this format and shown in the Format column.
    pub default_extension: &'static str,
    pub group: FormatGroup,
    pub decoder: FormatDecoder,
    pub animated: bool,
    /// The extension is also used by unrelated files, so a file is only taken when its header matches
    /// (`MediaDetector::confirms`).
    pub needs_header_check: bool,
    /// Short hint for the Settings page; empty when there is nothing to add.
    pub note: &'static str,
    /// The MIME type desktop applications know the format by ("Open With", clipboard).
    pub mime: &'static str,
}

// A table constructor: every column of a row is one argument.
#[allow(clippy::too_many_arguments)]
const fn spec(
    format: MediaFormat,
    id: &'static str,
    name: &'static str,
    long_name: &'static str,
    media_type: Option<MediaType>,
    extensions: &'static [&'static str],
    default_extension: &'static str,
    group: FormatGroup,
    decoder: FormatDecoder,
) -> FormatSpec {
    FormatSpec {
        format,
        id,
        name,
        long_name,
        media_type,
        extensions,
        default_extension,
        group,
        decoder,
        animated: false,
        needs_header_check: false,
        note: "",
        mime: "application/octet-stream",
    }
}

const fn animated(mut s: FormatSpec) -> FormatSpec {
    s.animated = true;
    s
}

const fn header_checked(mut s: FormatSpec) -> FormatSpec {
    s.needs_header_check = true;
    s
}

const fn mime(mut s: FormatSpec, mime: &'static str) -> FormatSpec {
    s.mime = mime;
    s
}

const fn noted(mut s: FormatSpec, note: &'static str) -> FormatSpec {
    s.note = note;
    s
}

use FormatDecoder::{FFmpeg, Native, Player};
use FormatGroup::{Common, Legacy, Specialized};
use MediaFormat as F;

const IMG: Option<MediaType> = Some(MediaType::Image);
const VID: Option<MediaType> = Some(MediaType::Video);

/// Indexed by `MediaFormat as usize`: the order MUST match the enum (`table_follows_enum_order` checks it). New
/// formats are appended to the enum and to the end of this table, never inserted: the library stores the variant index.
pub static FORMAT_SPECS: [FormatSpec; MediaFormat::COUNT] = [
    mime(
        spec(
            F::Jpeg,
            "jpeg",
            "JPEG",
            "JPEG",
            IMG,
            &["jpg", "jpeg", "jpe", "jfif", "jif"],
            "jpg",
            Common,
            Native,
        ),
        "image/jpeg",
    ),
    mime(
        spec(
            F::Png,
            "png",
            "PNG",
            "PNG",
            IMG,
            &["png", "apng"],
            "png",
            Common,
            Native,
        ),
        "image/png",
    ),
    noted(
        mime(
            spec(F::Webp, "webp", "WEBP", "WebP", IMG, &["webp"], "webp", Common, Native),
            "image/webp",
        ),
        "Animated files show their first frame.",
    ),
    noted(
        mime(
            spec(
                F::Avif,
                "avif",
                "AVIF",
                "AVIF",
                IMG,
                &["avif", "avifs"],
                "avif",
                Common,
                FFmpeg,
            ),
            "image/avif",
        ),
        "Decoded by FFmpeg (libdav1d). Tiled pictures are put together and the stored rotation is applied; animated files show their first frame.",
    ),
    noted(
        animated(mime(
            spec(F::Gif, "gif", "GIF", "GIF", IMG, &["gif"], "gif", Common, Native),
            "image/gif",
        )),
        "Animated: hover previews play the animation.",
    ),
    mime(
        spec(
            F::Bmp,
            "bmp",
            "BMP",
            "Windows Bitmap",
            IMG,
            &["bmp", "dib"],
            "bmp",
            Common,
            Native,
        ),
        "image/bmp",
    ),
    noted(
        mime(
            spec(
                F::Tiff,
                "tiff",
                "TIFF",
                "TIFF",
                IMG,
                &["tif", "tiff"],
                "tiff",
                Common,
                Native,
            ),
            "image/tiff",
        ),
        "Multi-page files show their first page.",
    ),
    noted(
        mime(
            spec(
                F::Psd,
                "psd",
                "PSD",
                "Photoshop Document",
                IMG,
                &["psd"],
                "psd",
                Specialized,
                FFmpeg,
            ),
            "image/vnd.adobe.photoshop",
        ),
        "Shows the flattened composite only.",
    ),
    mime(
        spec(
            F::Mp4,
            "mp4",
            "MP4",
            "MP4",
            VID,
            &["mp4", "m4v", "f4v"],
            "mp4",
            Common,
            Player,
        ),
        "video/mp4",
    ),
    mime(
        spec(
            F::Mkv,
            "mkv",
            "MKV",
            "Matroska",
            VID,
            &["mkv", "mk3d"],
            "mkv",
            Common,
            Player,
        ),
        "video/x-matroska",
    ),
    mime(
        spec(F::Webm, "webm", "WEBM", "WebM", VID, &["webm"], "webm", Common, Player),
        "video/webm",
    ),
    mime(
        spec(
            F::Mov,
            "mov",
            "MOV",
            "QuickTime",
            VID,
            &["mov", "qt"],
            "mov",
            Common,
            Player,
        ),
        "video/quicktime",
    ),
    mime(
        spec(
            F::Avi,
            "avi",
            "AVI",
            "AVI",
            VID,
            &["avi", "divx"],
            "avi",
            Common,
            Player,
        ),
        "video/x-msvideo",
    ),
    mime(
        spec(
            F::Mpeg,
            "mpeg",
            "MPEG",
            "MPEG-1/2",
            VID,
            &["mpg", "mpeg", "mpe", "vob", "m1v", "m2v"],
            "mpg",
            Common,
            Player,
        ),
        "video/mpeg",
    ),
    mime(
        spec(
            F::Flv,
            "flv",
            "FLV",
            "Flash Video",
            VID,
            &["flv"],
            "flv",
            Common,
            Player,
        ),
        "video/x-flv",
    ),
    // `Unknown` has no extensions: it only keeps the table aligned with the enum.
    spec(
        F::Unknown,
        "unknown",
        "UNKNOWN",
        "Unknown",
        None,
        &[],
        "",
        Legacy,
        Native,
    ),
    // ----- appended formats (images) -----
    mime(
        spec(
            F::Ico,
            "ico",
            "ICO",
            "Windows Icon",
            IMG,
            &["ico"],
            "ico",
            Common,
            Native,
        ),
        "image/vnd.microsoft.icon",
    ),
    mime(
        spec(F::Tga, "tga", "TGA", "Targa", IMG, &["tga"], "tga", Specialized, Native),
        "image/x-tga",
    ),
    mime(
        spec(
            F::Pnm,
            "pnm",
            "PNM",
            "Portable anymap (PBM, PGM, PPM, PAM, PFM)",
            IMG,
            &["pnm", "ppm", "pgm", "pbm", "pam", "pfm"],
            "ppm",
            Specialized,
            Native,
        ),
        "image/x-portable-anymap",
    ),
    mime(
        spec(
            F::Qoi,
            "qoi",
            "QOI",
            "Quite OK Image",
            IMG,
            &["qoi"],
            "qoi",
            Specialized,
            Native,
        ),
        "image/x-qoi",
    ),
    mime(
        spec(
            F::Hdr,
            "hdr",
            "HDR",
            "Radiance HDR",
            IMG,
            &["hdr"],
            "hdr",
            Specialized,
            Native,
        ),
        "image/vnd.radiance",
    ),
    mime(
        spec(
            F::Exr,
            "exr",
            "EXR",
            "OpenEXR",
            IMG,
            &["exr"],
            "exr",
            Specialized,
            Native,
        ),
        "image/x-exr",
    ),
    mime(
        spec(
            F::Dds,
            "dds",
            "DDS",
            "DirectDraw Surface",
            IMG,
            &["dds"],
            "dds",
            Specialized,
            Native,
        ),
        "image/vnd-ms.dds",
    ),
    mime(
        spec(
            F::Farbfeld,
            "farbfeld",
            "FF",
            "Farbfeld",
            IMG,
            &["ff"],
            "ff",
            Legacy,
            Native,
        ),
        "image/x-farbfeld",
    ),
    noted(
        mime(
            spec(
                F::Jxl,
                "jxl",
                "JXL",
                "JPEG XL",
                IMG,
                &["jxl"],
                "jxl",
                Specialized,
                FFmpeg,
            ),
            "image/jxl",
        ),
        "Needs FFmpeg built with libjxl (the Arch and Manjaro package is).",
    ),
    mime(
        spec(
            F::Jp2,
            "jp2",
            "JP2",
            "JPEG 2000",
            IMG,
            &["jp2", "j2k", "j2c", "jpc", "jpf", "jpx"],
            "jp2",
            Specialized,
            FFmpeg,
        ),
        "image/jp2",
    ),
    noted(
        mime(
            spec(F::Svg, "svg", "SVG", "SVG", IMG, &["svg"], "svg", Common, Native),
            "image/svg+xml",
        ),
        "Drawn by the built-in SVG renderer with the installed fonts. Files an SVG points to are not loaded.",
    ),
    mime(
        spec(
            F::Dpx,
            "dpx",
            "DPX",
            "Digital Picture Exchange",
            IMG,
            &["dpx"],
            "dpx",
            Specialized,
            FFmpeg,
        ),
        "image/x-dpx",
    ),
    mime(
        spec(
            F::Pcx,
            "pcx",
            "PCX",
            "PC Paintbrush",
            IMG,
            &["pcx"],
            "pcx",
            Legacy,
            FFmpeg,
        ),
        "image/vnd.zbrush.pcx",
    ),
    mime(
        spec(
            F::Sgi,
            "sgi",
            "SGI",
            "Silicon Graphics Image",
            IMG,
            &["sgi", "rgb", "bw"],
            "sgi",
            Legacy,
            FFmpeg,
        ),
        "image/x-sgi",
    ),
    mime(
        spec(
            F::SunRaster,
            "sunraster",
            "RAS",
            "Sun Raster",
            IMG,
            &["ras", "sun"],
            "ras",
            Legacy,
            FFmpeg,
        ),
        "image/x-sun-raster",
    ),
    mime(
        spec(F::Xbm, "xbm", "XBM", "X BitMap", IMG, &["xbm"], "xbm", Legacy, FFmpeg),
        "image/x-xbitmap",
    ),
    mime(
        spec(F::Xpm, "xpm", "XPM", "X PixMap", IMG, &["xpm"], "xpm", Legacy, FFmpeg),
        "image/x-xpixmap",
    ),
    mime(
        spec(
            F::Xwd,
            "xwd",
            "XWD",
            "X Window Dump",
            IMG,
            &["xwd"],
            "xwd",
            Legacy,
            FFmpeg,
        ),
        "image/x-xwindowdump",
    ),
    mime(
        spec(
            F::Wbmp,
            "wbmp",
            "WBMP",
            "Wireless Bitmap",
            IMG,
            &["wbmp"],
            "wbmp",
            Legacy,
            FFmpeg,
        ),
        "image/vnd.wap.wbmp",
    ),
    // ----- appended formats (videos) -----
    mime(
        spec(
            F::Wmv,
            "wmv",
            "WMV",
            "Windows Media Video (ASF)",
            VID,
            &["wmv", "asf"],
            "wmv",
            Common,
            Player,
        ),
        "video/x-ms-wmv",
    ),
    noted(
        header_checked(mime(
            spec(
                F::Ts,
                "ts",
                "TS",
                "MPEG Transport Stream",
                VID,
                &["ts", "m2t"],
                "ts",
                Common,
                Player,
            ),
            "video/mp2t",
        )),
        "Only files with a transport stream signature are taken, so TypeScript sources named .ts are ignored.",
    ),
    noted(
        header_checked(mime(
            spec(
                F::M2ts,
                "m2ts",
                "M2TS",
                "AVCHD / Blu-ray (M2TS)",
                VID,
                &["m2ts", "mts"],
                "m2ts",
                Common,
                Player,
            ),
            "video/mp2t",
        )),
        "Only files with a 192-byte packet signature are taken.",
    ),
    mime(
        spec(
            F::Ogv,
            "ogv",
            "OGV",
            "Ogg Video",
            VID,
            &["ogv", "ogm"],
            "ogv",
            Common,
            Player,
        ),
        "video/ogg",
    ),
    mime(
        spec(
            F::ThreeGp,
            "3gp",
            "3GP",
            "3GPP",
            VID,
            &["3gp", "3g2", "3gpp"],
            "3gp",
            Common,
            Player,
        ),
        "video/3gpp",
    ),
    mime(
        spec(
            F::Mxf,
            "mxf",
            "MXF",
            "Material Exchange Format",
            VID,
            &["mxf"],
            "mxf",
            Specialized,
            Player,
        ),
        "application/mxf",
    ),
    mime(
        spec(
            F::Dv,
            "dv",
            "DV",
            "Digital Video",
            VID,
            &["dv", "dif"],
            "dv",
            Specialized,
            Player,
        ),
        "video/x-dv",
    ),
    mime(
        spec(
            F::Rm,
            "rm",
            "RM",
            "RealMedia",
            VID,
            &["rm", "rmvb"],
            "rm",
            Legacy,
            Player,
        ),
        "application/vnd.rn-realmedia",
    ),
    mime(
        spec(
            F::Y4m,
            "y4m",
            "Y4M",
            "YUV4MPEG2",
            VID,
            &["y4m"],
            "y4m",
            Specialized,
            Player,
        ),
        "video/x-yuv4mpeg",
    ),
    mime(
        spec(
            F::Ivf,
            "ivf",
            "IVF",
            "IVF (VP8, VP9, AV1)",
            VID,
            &["ivf"],
            "ivf",
            Specialized,
            Player,
        ),
        "video/x-ivf",
    ),
    mime(
        spec(F::Nut, "nut", "NUT", "NUT", VID, &["nut"], "nut", Specialized, Player),
        "video/x-nut",
    ),
    mime(
        spec(
            F::H264,
            "h264",
            "H264",
            "H.264 elementary stream",
            VID,
            &["h264", "264"],
            "h264",
            Specialized,
            Player,
        ),
        "video/h264",
    ),
    mime(
        spec(
            F::Hevc,
            "hevc",
            "HEVC",
            "H.265 elementary stream",
            VID,
            &["hevc", "h265"],
            "hevc",
            Specialized,
            Player,
        ),
        "video/h265",
    ),
    mime(
        spec(
            F::Gxf,
            "gxf",
            "GXF",
            "General Exchange Format",
            VID,
            &["gxf"],
            "gxf",
            Specialized,
            Player,
        ),
        "application/gxf",
    ),
    mime(
        spec(
            F::Wtv,
            "wtv",
            "WTV",
            "Windows TV recording",
            VID,
            &["wtv", "dvr-ms"],
            "wtv",
            Legacy,
            Player,
        ),
        "video/x-ms-wtv",
    ),
    noted(
        mime(
            spec(
                F::Heic,
                "heic",
                "HEIC",
                "HEIC / HEIF",
                IMG,
                &["heic", "heif", "hif"],
                "heic",
                Common,
                FFmpeg,
            ),
            "image/heic",
        ),
        "Phone photos: tiled pictures are put together and the stored rotation is applied. HDR gain maps, depth and alpha channels are ignored.",
    ),
];

impl FormatSpec {
    /// The `(media type, group, short name)` order of the Settings page, the name compared without case.
    pub fn ui_sort_key(&self) -> (u8, FormatGroup, String) {
        let kind = match self.media_type {
            Some(MediaType::Image) => 0,
            Some(MediaType::Video) => 1,
            None => 2,
        };
        (kind, self.group, self.name.to_lowercase())
    }
}
