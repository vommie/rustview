use super::formats::{MediaFormat, MediaType};
use memchr::memmem;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Magic-byte sniffer. Only signatures strong enough to rename or classify a file by are recognized: formats that have no
/// reliable header (TGA, XBM, XWD, WBMP, raw H.264/HEVC streams) are never detected from content.
pub struct MediaDetector;

/// How many leading bytes the detector looks at (a transport stream needs four 188-byte packets).
pub const HEADER_LEN: usize = 1024;

impl MediaDetector {
    /// Reads the first `HEADER_LEN` bytes of a file.
    pub fn read_header(path: &Path) -> Option<Vec<u8>> {
        let mut file = File::open(path).ok()?;
        let mut buffer = vec![0u8; HEADER_LEN];
        let mut filled = 0;
        while filled < buffer.len() {
            match file.read(&mut buffer[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(_) => return None,
            }
        }
        buffer.truncate(filled);
        Some(buffer)
    }

    /// Inspects the header of a physical file to detect its media format and recommended extension.
    pub fn detect_format(path: &Path) -> Option<(MediaFormat, &'static str)> {
        Self::detect_from_bytes(&Self::read_header(path)?)
    }

    /// Detects format and standard extension directly from a byte slice header.
    pub fn detect_from_bytes(bytes: &[u8]) -> Option<(MediaFormat, &'static str)> {
        if bytes.len() < 3 {
            return None;
        }
        Self::detect_image(bytes)
            .or_else(|| Self::detect_riff(bytes))
            .or_else(|| Self::detect_iso_media(bytes))
            .or_else(|| Self::detect_video(bytes))
            .map(|format| (format, format.default_extension()))
    }

    /// The kind of content the header shows: a file called `.mp4` that holds an HEIC photo is an image whatever its name
    /// says.
    pub fn content_media_type(bytes: &[u8]) -> Option<MediaType> {
        Self::detect_from_bytes(bytes).and_then(|(format, _)| format.media_type())
    }

    /// For formats whose extension also names unrelated files (`.ts` is TypeScript too): does the header look like the
    /// format? Every other format is taken by its extension alone.
    pub fn confirms(format: MediaFormat, header: &[u8]) -> bool {
        match format {
            MediaFormat::Ts => is_transport_stream(header, 188, 0),
            MediaFormat::M2ts => is_transport_stream(header, 192, 4),
            _ => true,
        }
    }

    fn detect_image(b: &[u8]) -> Option<MediaFormat> {
        if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(MediaFormat::Jpeg);
        }
        if b.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(MediaFormat::Png);
        }
        if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
            return Some(MediaFormat::Gif);
        }
        if is_bmp(b) {
            return Some(MediaFormat::Bmp);
        }
        if b.starts_with(b"8BPS") && at(b, 4, &[0, 1]) {
            return Some(MediaFormat::Psd);
        }
        if b.starts_with(&[0x49, 0x49, 0x2A, 0x00])
            || b.starts_with(&[0x4D, 0x4D, 0x00, 0x2A])
            || b.starts_with(&[0x49, 0x49, 0x2B, 0x00])
            || b.starts_with(&[0x4D, 0x4D, 0x00, 0x2B])
        {
            return Some(MediaFormat::Tiff);
        }
        if b.starts_with(b"qoif") {
            return Some(MediaFormat::Qoi);
        }
        if b.starts_with(b"farbfeld") {
            return Some(MediaFormat::Farbfeld);
        }
        if b.starts_with(&[0x76, 0x2F, 0x31, 0x01]) {
            return Some(MediaFormat::Exr);
        }
        if b.starts_with(b"DDS ") && at(b, 4, &[0x7C, 0, 0, 0]) {
            return Some(MediaFormat::Dds);
        }
        if b.starts_with(b"#?RADIANCE\n") || b.starts_with(b"#?RGBE\n") {
            return Some(MediaFormat::Hdr);
        }
        if b.starts_with(&[0xFF, 0x0A])
            || b.starts_with(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A])
        {
            return Some(MediaFormat::Jxl);
        }
        if b.starts_with(&[0, 0, 0, 0x0C, b'j', b'P', b' ', b' ', 0x0D, 0x0A, 0x87, 0x0A])
            || b.starts_with(&[0xFF, 0x4F, 0xFF, 0x51])
        {
            return Some(MediaFormat::Jp2);
        }
        if b.starts_with(b"SDPX") || b.starts_with(b"XPDS") {
            return Some(MediaFormat::Dpx);
        }
        if b.starts_with(&[0x59, 0xA6, 0x6A, 0x95]) {
            return Some(MediaFormat::SunRaster);
        }
        if b.starts_with(b"/* XPM */") {
            return Some(MediaFormat::Xpm);
        }
        if is_sgi(b) {
            return Some(MediaFormat::Sgi);
        }
        if is_pcx(b) {
            return Some(MediaFormat::Pcx);
        }
        if is_ico(b) {
            return Some(MediaFormat::Ico);
        }
        if is_pnm(b) {
            return Some(MediaFormat::Pnm);
        }
        if is_svg(b) {
            return Some(MediaFormat::Svg);
        }
        None
    }

    fn detect_riff(b: &[u8]) -> Option<MediaFormat> {
        if b.len() < 12 || &b[0..4] != b"RIFF" {
            return None;
        }
        match &b[8..12] {
            b"WEBP" => Some(MediaFormat::Webp),
            b"AVI " | b"AVIX" => Some(MediaFormat::Avi),
            _ => None,
        }
    }

    /// MP4, `QuickTime`, 3GPP and AVIF share the ISO base media container; the `ftyp` brands tell them apart.
    fn detect_iso_media(b: &[u8]) -> Option<MediaFormat> {
        if let Some(brands) = IsoBrands::parse(b) {
            if brands.has(*b"avif") || brands.has(*b"avis") {
                return Some(MediaFormat::Avif);
            }
            if brands.has_any(HEIF_BRANDS) {
                return Some(MediaFormat::Heic);
            }
            if brands.major.starts_with(b"M4A") || brands.major.starts_with(b"M4B") || brands.major.starts_with(b"M4P")
            {
                return None;
            }
            return Some(match brands.major {
                b"qt  " => MediaFormat::Mov,
                m if m.starts_with(b"3g") => MediaFormat::ThreeGp,
                _ => MediaFormat::Mp4,
            });
        }
        // QuickTime files without an ftyp box start with another top-level box.
        let size_ok = b.len() >= 8 && u32::from_be_bytes([b[0], b[1], b[2], b[3]]) >= 8;
        if size_ok && matches!(&b[4..8], b"moov" | b"mdat" | b"free" | b"wide") {
            return Some(MediaFormat::Mov);
        }
        None
    }

    fn detect_video(b: &[u8]) -> Option<MediaFormat> {
        if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            let window = &b[..b.len().min(128)];
            return Some(if memmem::find(window, b"webm").is_some() {
                MediaFormat::Webm
            } else {
                MediaFormat::Mkv
            });
        }
        if b.starts_with(b"FLV\x01") {
            return Some(MediaFormat::Flv);
        }
        if b.starts_with(&[0, 0, 1, 0xBA]) || b.starts_with(&[0, 0, 1, 0xB3]) {
            return Some(MediaFormat::Mpeg);
        }
        if b.starts_with(&WTV_HEADER_GUID) {
            return Some(MediaFormat::Wtv);
        }
        if b.starts_with(&ASF_HEADER_GUID) {
            return memmem::find(b, &ASF_VIDEO_STREAM_GUID).map(|_| MediaFormat::Wmv);
        }
        if b.starts_with(b"OggS") {
            let has_video = memmem::find(b, b"\x80theora").is_some() || memmem::find(b, b"\x01video").is_some();
            return has_video.then_some(MediaFormat::Ogv);
        }
        if b.starts_with(&[0x06, 0x0E, 0x2B, 0x34, 0x02, 0x05, 0x01, 0x01, 0x0D, 0x01, 0x02, 0x01]) {
            return Some(MediaFormat::Mxf);
        }
        if b.starts_with(&[0x1F, 0x07, 0x00]) {
            return Some(MediaFormat::Dv);
        }
        if b.starts_with(b".RMF") {
            return Some(MediaFormat::Rm);
        }
        if b.starts_with(b"YUV4MPEG2 ") {
            return Some(MediaFormat::Y4m);
        }
        if b.starts_with(b"DKIF") && at(b, 4, &[0, 0]) {
            return Some(MediaFormat::Ivf);
        }
        if b.starts_with(b"nut/multimedia container\0") {
            return Some(MediaFormat::Nut);
        }
        if is_transport_stream(b, 188, 0) {
            return Some(MediaFormat::Ts);
        }
        if is_transport_stream(b, 192, 4) {
            return Some(MediaFormat::M2ts);
        }
        None
    }
}

/// HEIF/HEIC brands (AVIF files carry `mif1` too, so AVIF is checked first).
const HEIF_BRANDS: &[&[u8; 4]] = &[b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"mif1", b"msf1"];

const ASF_HEADER_GUID: [u8; 16] = [
    0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C,
];
const WTV_HEADER_GUID: [u8; 16] = [
    0xB7, 0xD8, 0x00, 0x20, 0x37, 0x49, 0xDA, 0x11, 0xA6, 0x4E, 0x00, 0x07, 0xE9, 0x5E, 0xAD, 0x8D,
];
/// ASF stream type "video media"; an ASF without one is a WMA audio file.
const ASF_VIDEO_STREAM_GUID: [u8; 16] = [
    0xC0, 0xEF, 0x19, 0xBC, 0x4D, 0x5B, 0xCF, 0x11, 0xA8, 0xFD, 0x00, 0x80, 0x5F, 0x5C, 0x44, 0x2B,
];

/// The brands of an `ftyp` box.
struct IsoBrands<'a> {
    major: &'a [u8],
    compatible: &'a [u8],
}

impl<'a> IsoBrands<'a> {
    fn parse(b: &'a [u8]) -> Option<Self> {
        if b.len() < 16 || &b[4..8] != b"ftyp" {
            return None;
        }
        let box_size = u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize;
        let end = if box_size >= 16 {
            box_size.min(b.len())
        } else {
            b.len().min(64)
        };
        Some(Self {
            major: &b[8..12],
            compatible: &b[16..end.max(16)],
        })
    }

    fn has(&self, brand: [u8; 4]) -> bool {
        self.major == brand || self.compatible.as_chunks::<4>().0.contains(&brand)
    }

    fn has_any(&self, brands: &[&[u8; 4]]) -> bool {
        brands.iter().any(|&b| self.has(*b))
    }
}

fn at(b: &[u8], offset: usize, pattern: &[u8]) -> bool {
    b.get(offset..offset + pattern.len()) == Some(pattern)
}

/// `BM` plus a plausible DIB header size: two letters alone would rename any text file that starts with "BM".
fn is_bmp(b: &[u8]) -> bool {
    if b.len() < 18 || &b[0..2] != b"BM" || b[6..10] != [0, 0, 0, 0] {
        return false;
    }
    matches!(
        u32::from_le_bytes([b[14], b[15], b[16], b[17]]),
        12 | 40 | 52 | 56 | 64 | 108 | 124
    )
}

fn is_ico(b: &[u8]) -> bool {
    if b.len() < 22 || b[0..4] != [0, 0, 1, 0] {
        return false;
    }
    let count = u16::from_le_bytes([b[4], b[5]]) as usize;
    let size = u32::from_le_bytes([b[14], b[15], b[16], b[17]]);
    let offset = u32::from_le_bytes([b[18], b[19], b[20], b[21]]) as usize;
    (1..=256).contains(&count) && b[9] == 0 && size > 0 && offset >= 6 + 16 * count
}

fn is_sgi(b: &[u8]) -> bool {
    b.len() >= 8
        && b[0] == 0x01
        && b[1] == 0xDA
        && b[2] <= 1
        && (b[3] == 1 || b[3] == 2)
        && (1..=3).contains(&u16::from_be_bytes([b[4], b[5]]))
}

fn is_pcx(b: &[u8]) -> bool {
    if b.len() < 12 || b[0] != 0x0A || !matches!(b[1], 0 | 2 | 3 | 4 | 5) || b[2] != 1 || !matches!(b[3], 1 | 2 | 4 | 8)
    {
        return false;
    }
    let (xmin, ymin) = (u16::from_le_bytes([b[4], b[5]]), u16::from_le_bytes([b[6], b[7]]));
    let (xmax, ymax) = (u16::from_le_bytes([b[8], b[9]]), u16::from_le_bytes([b[10], b[11]]));
    xmax >= xmin && ymax >= ymin
}

/// `P1`..`P7`, `Pf`, `PF` followed by white space and a number or a comment.
fn is_pnm(b: &[u8]) -> bool {
    if b.len() < 4
        || b[0] != b'P'
        || !matches!(b[1], b'1'..=b'7' | b'f' | b'F')
        || !matches!(b[2], b' ' | b'\t' | b'\n' | b'\r')
    {
        return false;
    }
    if b[1] == b'7' {
        return b[2] == b'\n' && (b[3..].starts_with(b"WIDTH") || b[3..].starts_with(b"#"));
    }
    b[3..]
        .iter()
        .find(|c| !c.is_ascii_whitespace())
        .is_some_and(|c| c.is_ascii_digit() || *c == b'#')
}

/// Text that is an `<svg` element or starts like an XML document (prolog, doctype or comment) and mentions `<svg` early.
/// An HTML page with an inline SVG starts differently and is not taken.
fn is_svg(b: &[u8]) -> bool {
    let text = b.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(b);
    let start = text.iter().position(|c| !c.is_ascii_whitespace()).unwrap_or(text.len());
    let text = &text[start..];
    text.starts_with(b"<svg")
        || ((text.starts_with(b"<?xml") || text.starts_with(b"<!--") || text.starts_with(b"<!DOCTYPE svg"))
            && memmem::find(text, b"<svg").is_some())
}

/// MPEG transport stream: the sync byte `0x47` repeats at the packet size four times in a row. `lead` is the number of
/// bytes in front of each sync byte (the 4-byte timestamp of M2TS). Text that happens to match is rejected.
fn is_transport_stream(header: &[u8], packet: usize, lead: usize) -> bool {
    const SYNCS: usize = 4;
    if header.len() < packet * (SYNCS - 1) + lead + 1 || looks_like_text(header) {
        return false;
    }
    (0..packet).any(|start| (0..SYNCS).all(|i| header.get(start + lead + i * packet) == Some(&0x47)))
}

fn looks_like_text(header: &[u8]) -> bool {
    let printable = header
        .iter()
        .filter(|c| matches!(c, b'\t' | b'\n' | b'\r' | 0x20..=0x7E))
        .count();
    printable * 100 >= header.len() * 95
}
