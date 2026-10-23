//! What the editor needs to know about a picture file beyond its pixels: the format its content really has, whether it is
//! animated or has several pages, its transparency and bit depth, and the block layout of a JPEG. Read from the file's
//! headers on the worker that decodes the picture for a tile (the file is being read for it anyway), never on the UI thread.

use crate::media::MediaFormat;
use image::ImageFormat;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

/// Files larger than this are not read whole by the probe (a JPEG or AVIF this large is not edited).
const MAX_WHOLE_READ: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceProbe {
    /// The format the content has (`None`: not recognized by its signature; the extension decides then).
    pub signature: Option<MediaFormat>,
    pub animated: bool,
    pub multi_page: bool,
    /// `Some(true)` when the picture has an alpha channel or transparency, `None` when unknown.
    pub has_alpha: Option<bool>,
    /// Bits per channel when known.
    pub bit_depth: Option<u8>,
    /// The block layout of a JPEG.
    pub jpeg: Option<JpegLayout>,
    /// `Some(true)` for a lossless WebP, `Some(false)` for a lossy one.
    pub webp_lossless: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegLayout {
    /// Size of one MCU in pixels: a crop whose top left corner lies on this grid is lossless.
    pub mcu: (u32, u32),
    pub components: u8,
    pub progressive: bool,
}

impl JpegLayout {
    /// CMYK and YCCK files are only ever cropped without loss (re-encoding them would change their sampling).
    pub fn is_cmyk(self) -> bool {
        self.components == 4
    }
}

/// The format `image` recognizes from the first bytes of a file.
pub fn signature_format(head: &[u8]) -> Option<MediaFormat> {
    Some(match image::guess_format(head).ok()? {
        ImageFormat::Jpeg => MediaFormat::Jpeg,
        ImageFormat::Png => MediaFormat::Png,
        ImageFormat::Gif => MediaFormat::Gif,
        ImageFormat::WebP => MediaFormat::Webp,
        ImageFormat::Tiff => MediaFormat::Tiff,
        ImageFormat::Bmp => MediaFormat::Bmp,
        ImageFormat::Ico => MediaFormat::Ico,
        ImageFormat::Avif => MediaFormat::Avif,
        ImageFormat::Qoi => MediaFormat::Qoi,
        ImageFormat::Farbfeld => MediaFormat::Farbfeld,
        ImageFormat::Hdr => MediaFormat::Hdr,
        ImageFormat::OpenExr => MediaFormat::Exr,
        ImageFormat::Dds => MediaFormat::Dds,
        ImageFormat::Pnm => MediaFormat::Pnm,
        _ => return None,
    })
}

/// Examines the file at `path`; `format` is the format of its extension (the fallback when the content has no signature).
pub fn probe(path: &Path, format: MediaFormat) -> io::Result<SourceProbe> {
    let mut head = [0u8; 64];
    let read = read_head(path, &mut head)?;
    let signature = signature_format(&head[..read]);
    let mut probe = SourceProbe {
        signature,
        ..SourceProbe::default()
    };
    match signature.unwrap_or(format) {
        MediaFormat::Jpeg => probe_jpeg(path, &mut probe)?,
        MediaFormat::Png => probe_png(path, &mut probe)?,
        MediaFormat::Gif => probe_gif(path, &mut probe)?,
        MediaFormat::Webp => probe_webp(path, &mut probe)?,
        MediaFormat::Tiff => probe_tiff(path, &mut probe)?,
        MediaFormat::Avif => probe_avif(path, &mut probe)?,
        _ => {}
    }
    Ok(probe)
}

fn read_head(path: &Path, head: &mut [u8]) -> io::Result<usize> {
    let mut file = File::open(path)?;
    let mut filled = 0;
    while filled < head.len() {
        match file.read(&mut head[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

fn read_whole(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    if file.metadata()?.len() > MAX_WHOLE_READ {
        return Err(io::Error::other("the file is too large to be edited"));
    }
    let mut data = Vec::new();
    BufReader::new(file).read_to_end(&mut data)?;
    Ok(data)
}

fn invalid(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

fn probe_jpeg(path: &Path, probe: &mut SourceProbe) -> io::Result<()> {
    let data = read_whole(path)?;
    let decoder = libjpeg_turbo_rs::Decoder::new(&data).map_err(invalid)?;
    let header = decoder.header();
    let max_h = header
        .components
        .iter()
        .map(|c| c.horizontal_sampling)
        .max()
        .unwrap_or(1)
        .max(1);
    let max_v = header
        .components
        .iter()
        .map(|c| c.vertical_sampling)
        .max()
        .unwrap_or(1)
        .max(1);
    probe.jpeg = Some(JpegLayout {
        mcu: (8 * u32::from(max_h), 8 * u32::from(max_v)),
        components: u8::try_from(header.components.len()).unwrap_or(u8::MAX),
        progressive: header.is_progressive,
    });
    probe.has_alpha = Some(false);
    probe.bit_depth = Some(header.precision);
    Ok(())
}

fn probe_png(path: &Path, probe: &mut SourceProbe) -> io::Result<()> {
    let reader = png::Decoder::new(BufReader::new(File::open(path)?))
        .read_info()
        .map_err(invalid)?;
    let info = reader.info();
    probe.animated = info.animation_control.is_some_and(|actl| actl.num_frames > 1);
    probe.has_alpha =
        Some(matches!(info.color_type, png::ColorType::GrayscaleAlpha | png::ColorType::Rgba) || info.trns.is_some());
    probe.bit_depth = Some(info.bit_depth as u8);
    Ok(())
}

fn probe_gif(path: &Path, probe: &mut SourceProbe) -> io::Result<()> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    let mut decoder = options.read_info(BufReader::new(File::open(path)?)).map_err(invalid)?;
    let mut frames = 0;
    while frames < 2 && decoder.read_next_frame().map_err(invalid)?.is_some() {
        frames += 1;
    }
    probe.animated = frames > 1;
    probe.bit_depth = Some(8);
    Ok(())
}

fn probe_webp(path: &Path, probe: &mut SourceProbe) -> io::Result<()> {
    let mut decoder = image_webp::WebPDecoder::new(BufReader::new(File::open(path)?)).map_err(invalid)?;
    probe.animated = decoder.is_animated();
    probe.has_alpha = Some(decoder.has_alpha());
    probe.webp_lossless = Some(!decoder.is_lossy());
    probe.bit_depth = Some(8);
    Ok(())
}

fn probe_tiff(path: &Path, probe: &mut SourceProbe) -> io::Result<()> {
    let mut decoder = tiff::decoder::Decoder::new(BufReader::new(File::open(path)?)).map_err(invalid)?;
    probe.multi_page = decoder.more_images();
    if let Ok(color) = decoder.colortype() {
        probe.has_alpha = Some(matches!(
            color,
            tiff::ColorType::GrayA(_) | tiff::ColorType::RGBA(_) | tiff::ColorType::CMYKA(_)
        ));
        probe.bit_depth = Some(color.bit_depth());
    }
    Ok(())
}

fn probe_avif(path: &Path, probe: &mut SourceProbe) -> io::Result<()> {
    let data = read_whole(path)?;
    probe.animated = ftyp_brands(&data).any(|brand| brand == *b"avis" || brand == *b"msf1");
    if let Ok(avif) = avif_parse::read_avif(&mut data.as_slice()) {
        probe.has_alpha = Some(avif.alpha_item.is_some());
        probe.bit_depth = avif.primary_item_metadata().ok().map(|m| m.bit_depth);
    }
    Ok(())
}

/// The major and the compatible brands of an ISO-BMFF `ftyp` box at the start of `data`.
fn ftyp_brands(data: &[u8]) -> impl Iterator<Item = [u8; 4]> + '_ {
    let size = data
        .get(0..4)
        .and_then(|b| b.try_into().ok())
        .map_or(0, u32::from_be_bytes);
    let size = usize::try_from(size).unwrap_or(0).min(data.len());
    let body = if data.get(4..8) == Some(b"ftyp") && size >= 16 {
        &data[8..size]
    } else {
        &[][..]
    };
    // major brand (4), minor version (4), then the compatible brands
    let major = body.get(0..4).into_iter();
    let compatible = body
        .get(8..)
        .unwrap_or(&[])
        .as_chunks::<4>()
        .0
        .iter()
        .map(<[u8; 4]>::as_slice);
    major.chain(compatible).filter_map(|b| b.try_into().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_brands_of_an_ftyp_box_are_read() {
        let mut data = Vec::new();
        data.extend_from_slice(&24u32.to_be_bytes());
        data.extend_from_slice(b"ftypavis");
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(b"avifmif1");
        let brands: Vec<[u8; 4]> = ftyp_brands(&data).collect();
        assert_eq!(brands, [*b"avis", *b"avif", *b"mif1"]);
        assert_eq!(ftyp_brands(b"not a box").count(), 0);
        assert_eq!(ftyp_brands(&[]).count(), 0);
    }

    #[test]
    fn signatures_are_recognized_whatever_the_name() {
        assert_eq!(signature_format(b"\x89PNG\r\n\x1a\n...."), Some(MediaFormat::Png));
        assert_eq!(signature_format(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(MediaFormat::Jpeg));
        assert_eq!(signature_format(b"GIF89a"), Some(MediaFormat::Gif));
        assert_eq!(signature_format(b"nothing"), None);
    }
}
