//! Keeping the metadata of a picture when it is written again: EXIF, the ICC profile and XMP, and for a save in the same
//! format every other metadata part of the original container (JPEG `APPn` and `COM` segments, PNG ancillary chunks, WebP
//! chunks). EXIF is patched to the new size (`exif_patch`). Parts that would describe the old picture and cannot be updated
//! are dropped: the Multi-Picture (MPF) index of a JPEG (its secondary pictures follow the old image data) and the
//! animation chunks of a PNG.

use super::exif_patch::{EXIF_HEADER, patch_exif};
use super::formats::WriteFormat;
use crate::media::MediaFormat;
use bytes::Bytes;
use img_parts::jpeg::{Jpeg, JpegSegment};
use img_parts::png::{Png, PngChunk};
use img_parts::riff::{RiffChunk, RiffContent};
use img_parts::webp::WebP;
use img_parts::{ImageEXIF, ImageICC};

const XMP_JPEG_PREFIX: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const XMP_PNG_KEYWORD: &[u8] = b"XML:com.adobe.xmp";
const MPF_PREFIX: &[u8] = b"MPF\0";
const APP0: u8 = 0xE0;
const APP15: u8 = 0xEF;
const COM: u8 = 0xFE;

/// PNG chunks that must come before PLTE (and therefore before the image data).
const PNG_BEFORE_PLTE: [[u8; 4]; 8] = [
    *b"iCCP", *b"sRGB", *b"gAMA", *b"cHRM", *b"sBIT", *b"cICP", *b"mDCV", *b"cLLI",
];
/// PNG chunks the encoder writes itself or that belong to an animation.
const PNG_OWN: [[u8; 4]; 8] = [
    *b"IHDR", *b"PLTE", *b"tRNS", *b"IDAT", *b"IEND", *b"acTL", *b"fcTL", *b"fdAT",
];

/// The metadata of the original picture.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMetadata {
    /// EXIF as TIFF data (without the "Exif\0\0" prefix).
    pub exif: Option<Vec<u8>>,
    pub icc: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
    /// The original's own metadata parts, for a save in the same format.
    pub parts: ContainerParts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ContainerParts {
    #[default]
    None,
    /// APP1..APP15 and COM segments in their order (APP0/JFIF is written by the encoder).
    Jpeg(Vec<(u8, Vec<u8>)>),
    /// Ancillary chunks in their order.
    Png(Vec<([u8; 4], Vec<u8>)>),
    /// Metadata chunks (ICCP, EXIF, XMP and unknown ones).
    Webp(Vec<([u8; 4], Vec<u8>)>),
}

/// Reads the metadata of `data`, a file whose content is `format`.
pub fn read_metadata(data: &[u8], format: MediaFormat) -> SourceMetadata {
    let bytes = Bytes::copy_from_slice(data);
    match format {
        MediaFormat::Jpeg => Jpeg::from_bytes(bytes).map(|jpeg| read_jpeg(&jpeg)).unwrap_or_default(),
        MediaFormat::Png => Png::from_bytes(bytes).map(|png| read_png(&png)).unwrap_or_default(),
        MediaFormat::Webp => WebP::from_bytes(bytes).map(|webp| read_webp(&webp)).unwrap_or_default(),
        MediaFormat::Tiff => read_tiff(data),
        _ => SourceMetadata::default(),
    }
}

fn read_jpeg(jpeg: &Jpeg) -> SourceMetadata {
    let parts = jpeg
        .segments()
        .iter()
        .filter(|s| (s.marker() > APP0 && s.marker() <= APP15) || s.marker() == COM)
        .filter(|s| !(s.marker() == 0xE2 && s.contents().starts_with(MPF_PREFIX)))
        .map(|s| (s.marker(), s.contents().to_vec()))
        .collect();
    let xmp = jpeg
        .segments()
        .iter()
        .find(|s| s.marker() == 0xE1 && s.contents().starts_with(XMP_JPEG_PREFIX))
        .map(|s| s.contents()[XMP_JPEG_PREFIX.len()..].to_vec());
    SourceMetadata {
        exif: jpeg.exif().map(|b| b.to_vec()),
        icc: jpeg.icc_profile().map(|b| b.to_vec()),
        xmp,
        parts: ContainerParts::Jpeg(parts),
    }
}

fn read_png(png: &Png) -> SourceMetadata {
    let parts = png
        .chunks()
        .iter()
        .filter(|c| !PNG_OWN.contains(&c.kind()))
        .map(|c| (c.kind(), c.contents().to_vec()))
        .collect();
    let xmp = png
        .chunks_by_type(*b"iTXt")
        .find_map(|chunk| xmp_from_itxt(chunk.contents()));
    SourceMetadata {
        exif: png.exif().map(|b| b.to_vec()),
        icc: png.icc_profile().map(|b| b.to_vec()),
        xmp,
        parts: ContainerParts::Png(parts),
    }
}

/// The text of an uncompressed iTXt chunk with the XMP keyword.
fn xmp_from_itxt(contents: &[u8]) -> Option<Vec<u8>> {
    let rest = contents.strip_prefix(XMP_PNG_KEYWORD)?.strip_prefix(b"\0")?;
    // compression flag, compression method, language tag\0, translated keyword\0, text
    let (&flag, rest) = rest.split_first()?;
    if flag != 0 {
        return None;
    }
    let rest = rest.get(1..)?;
    let lang_end = rest.iter().position(|b| *b == 0)?;
    let rest = rest.get(lang_end + 1..)?;
    let translated_end = rest.iter().position(|b| *b == 0)?;
    Some(rest.get(translated_end + 1..)?.to_vec())
}

fn read_webp(webp: &WebP) -> SourceMetadata {
    let parts = webp
        .chunks()
        .iter()
        .filter(|c| !matches!(&c.id(), b"VP8 " | b"VP8L" | b"VP8X" | b"ALPH" | b"ANIM" | b"ANMF"))
        .filter_map(|c| c.content().data().map(|d| (c.id(), d.to_vec())))
        .collect();
    let xmp = webp
        .chunk_by_id(*b"XMP ")
        .and_then(|c| c.content().data())
        .map(|d| d.to_vec());
    SourceMetadata {
        exif: webp.exif().map(|b| b.to_vec()),
        icc: webp.icc_profile().map(|b| b.to_vec()),
        xmp,
        parts: ContainerParts::Webp(parts),
    }
}

fn read_tiff(data: &[u8]) -> SourceMetadata {
    let Ok(mut decoder) = tiff::decoder::Decoder::new(std::io::Cursor::new(data)) else {
        return SourceMetadata::default();
    };
    let bytes_of = |decoder: &mut tiff::decoder::Decoder<std::io::Cursor<&[u8]>>, tag: u16| {
        decoder
            .get_tag_u8_vec(tiff::tags::Tag::Unknown(tag))
            .ok()
            .filter(|v| !v.is_empty())
    };
    SourceMetadata {
        exif: None,
        icc: bytes_of(&mut decoder, 34675),
        xmp: bytes_of(&mut decoder, 700),
        parts: ContainerParts::None,
    }
}

/// The EXIF block for a picture of `size` (patched), if there is one.
pub fn patched_exif(meta: &SourceMetadata, size: (u32, u32)) -> Option<Vec<u8>> {
    let mut exif = meta.exif.clone()?;
    let report = patch_exif(&mut exif, size);
    tracing::debug!(
        size_fields = report.size_fields,
        thumbnail_unlinked = report.thumbnail_unlinked,
        "[ImageEdit: Metadata] EXIF brought up to date"
    );
    Some(exif)
}

/// Puts the metadata of the original into `encoded`, a file of `target` with a picture of `size`. Formats whose encoder
/// takes the metadata itself (TIFF) and formats without metadata are returned as they are.
pub fn embed_metadata(
    encoded: Vec<u8>,
    target: WriteFormat,
    meta: &SourceMetadata,
    size: (u32, u32),
) -> Result<Vec<u8>, String> {
    if meta == &SourceMetadata::default() {
        return Ok(encoded);
    }
    let exif = patched_exif(meta, size);
    match target {
        WriteFormat::Jpeg => embed_jpeg(encoded, meta, exif),
        WriteFormat::Png => embed_png(encoded, meta, exif),
        WriteFormat::Webp => embed_webp(encoded, meta, exif),
        _ => Ok(encoded),
    }
}

fn embed_jpeg(encoded: Vec<u8>, meta: &SourceMetadata, exif: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut jpeg = Jpeg::from_bytes(Bytes::from(encoded)).map_err(|e| e.to_string())?;
    // After SOI and the encoder's own APP0, before the tables and the frame.
    let at = jpeg.segments().iter().take_while(|s| s.marker() == APP0).count();
    let segments: Vec<JpegSegment> = if let ContainerParts::Jpeg(parts) = &meta.parts {
        parts
            .iter()
            .map(|(marker, contents)| {
                let is_exif = *marker == 0xE1 && contents.starts_with(EXIF_HEADER);
                let contents = match (&exif, is_exif) {
                    (Some(exif), true) => [EXIF_HEADER, exif.as_slice()].concat(),
                    _ => contents.clone(),
                };
                JpegSegment::new_with_contents(*marker, Bytes::from(contents))
            })
            .collect()
    } else {
        meta.xmp
            .iter()
            .map(|xmp| JpegSegment::new_with_contents(0xE1, Bytes::from([XMP_JPEG_PREFIX, xmp.as_slice()].concat())))
            .collect()
    };
    for (offset, segment) in segments.into_iter().enumerate() {
        jpeg.segments_mut().insert(at + offset, segment);
    }
    if !matches!(meta.parts, ContainerParts::Jpeg(_)) {
        jpeg.set_icc_profile(meta.icc.clone().map(Bytes::from));
        jpeg.set_exif(exif.map(Bytes::from));
    }
    Ok(jpeg.encoder().bytes().to_vec())
}

fn embed_png(encoded: Vec<u8>, meta: &SourceMetadata, exif: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut png = Png::from_bytes(Bytes::from(encoded)).map_err(|e| e.to_string())?;
    if let ContainerParts::Png(parts) = &meta.parts {
        {
            for (kind, contents) in parts {
                let contents = if *kind == *b"eXIf" {
                    exif.clone().unwrap_or_else(|| contents.clone())
                } else {
                    contents.clone()
                };
                let chunk = PngChunk::new(*kind, Bytes::from(contents));
                let chunks = png.chunks_mut();
                // Chunks that must precede PLTE go right after IHDR, the others right before the image data.
                let at = if PNG_BEFORE_PLTE.contains(kind) {
                    chunks.iter().position(|c| c.kind() == *b"PLTE" || c.kind() == *b"IDAT")
                } else {
                    chunks.iter().position(|c| c.kind() == *b"IDAT")
                }
                .unwrap_or(chunks.len().min(1));
                chunks.insert(at, chunk);
            }
        }
    } else {
        {
            png.set_icc_profile(meta.icc.clone().map(Bytes::from));
            png.set_exif(exif.map(Bytes::from));
            if let Some(xmp) = &meta.xmp {
                let mut contents = XMP_PNG_KEYWORD.to_vec();
                contents.extend_from_slice(&[0, 0, 0, 0, 0]);
                contents.extend_from_slice(xmp);
                let chunks = png.chunks_mut();
                let at = chunks.iter().position(|c| c.kind() == *b"IDAT").unwrap_or(chunks.len());
                chunks.insert(at, PngChunk::new(*b"iTXt", Bytes::from(contents)));
            }
        }
    }
    Ok(png.encoder().bytes().to_vec())
}

fn embed_webp(encoded: Vec<u8>, meta: &SourceMetadata, exif: Option<Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut webp = WebP::from_bytes(Bytes::from(encoded)).map_err(|e| e.to_string())?;
    let extra: Vec<([u8; 4], Vec<u8>)> = match &meta.parts {
        ContainerParts::Webp(parts) => parts
            .iter()
            .filter(|(id, _)| !matches!(id, b"ICCP" | b"EXIF"))
            .cloned()
            .collect(),
        _ => meta.xmp.iter().map(|xmp| (*b"XMP ", xmp.clone())).collect(),
    };
    webp.set_icc_profile(meta.icc.clone().map(Bytes::from));
    for (id, data) in extra {
        webp.chunks_mut()
            .push(RiffChunk::new(id, RiffContent::Data(Bytes::from(data))));
    }
    // set_exif recomputes the extended header (VP8X) and its flags for every chunk that is there now.
    webp.set_exif(exif.map(Bytes::from));
    Ok(webp.encoder().bytes().to_vec())
}
