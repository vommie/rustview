use super::formats::MediaFormat;
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// High-performance magic byte sniffer inspecting file headers to identify supported media formats.
pub struct MediaDetector;

impl MediaDetector {
    /// Inspects the header of a physical file to detect its media format and recommended extension.
    pub fn detect_format(path: &Path) -> Option<(MediaFormat, &'static str)> {
        let mut file = File::open(path).ok()?;
        let mut buffer = [0u8; 512];
        let bytes_read = file.read(&mut buffer).ok()?;
        if bytes_read < 3 {
            return None;
        }

        Self::detect_from_bytes(&buffer[..bytes_read])
    }

    /// Detects format and standard extension directly from a byte slice header.
    pub fn detect_from_bytes(bytes: &[u8]) -> Option<(MediaFormat, &'static str)> {
        if bytes.len() < 3 {
            return None;
        }

        // 1. JPEG / JFIF / JPE (FF D8 FF)
        if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
            return Some((MediaFormat::Jpeg, "jpg"));
        }

        // 2. PNG (89 50 4E 47 0D 0A 1A 0A)
        if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
            return Some((MediaFormat::Png, "png"));
        }

        // 3. GIF (GIF87a or GIF89a)
        if bytes.len() >= 6 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
            return Some((MediaFormat::Gif, "gif"));
        }

        // 4. BMP (42 4D / 'BM')
        if bytes.len() >= 2 && bytes[0] == 0x42 && bytes[1] == 0x4D {
            return Some((MediaFormat::Bmp, "bmp"));
        }

        // 5. PSD / Photoshop (38 42 50 53 / '8BPS')
        if bytes.len() >= 4 && &bytes[0..4] == b"8BPS" {
            return Some((MediaFormat::Psd, "psd"));
        }

        // 6. TIFF (49 49 2A 00 / 'II*\0' or 4D 4D 00 2A / 'MM\0*')
        if bytes.len() >= 4
            && ((bytes[0] == 0x49 && bytes[1] == 0x49 && bytes[2] == 0x2A && bytes[3] == 0x00)
                || (bytes[0] == 0x4D && bytes[1] == 0x4D && bytes[2] == 0x00 && bytes[3] == 0x2A))
        {
            return Some((MediaFormat::Tiff, "tiff"));
        }

        // 7. RIFF-based formats (WEBP, AVI)
        if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" {
            if &bytes[8..12] == b"WEBP" {
                return Some((MediaFormat::Webp, "webp"));
            }
            if &bytes[8..12] == b"AVI " || &bytes[8..12] == b"AVIX" {
                return Some((MediaFormat::Avi, "avi"));
            }
        }

        // 8. FLV (46 4C 56 01 / 'FLV\x01')
        if bytes.len() >= 4 && &bytes[0..3] == b"FLV" {
            return Some((MediaFormat::Flv, "flv"));
        }

        // 9. Matroska / WebM EBML (1A 45 DF A3)
        if bytes.len() >= 4
            && bytes[0] == 0x1A
            && bytes[1] == 0x45
            && bytes[2] == 0xDF
            && bytes[3] == 0xA3
        {
            let limit = bytes.len().min(64);
            if memchr::memmem::find(&bytes[..limit], b"webm").is_some() {
                return Some((MediaFormat::Webm, "webm"));
            }
            return Some((MediaFormat::Mkv, "mkv"));
        }

        // 10. MPEG / MPG (00 00 01 BA = MPEG-PS, 00 00 01 B3 = MPEG Sequence)
        if bytes.len() >= 4
            && bytes[0] == 0x00
            && bytes[1] == 0x00
            && bytes[2] == 0x01
            && (bytes[3] == 0xBA || bytes[3] == 0xB3)
        {
            return Some((MediaFormat::Mpeg, "mpg"));
        }

        // 11. ISO Base Media File Format (MP4, QuickTime MOV, AVIF)
        if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
            let major_brand = &bytes[8..12];
            match major_brand {
                b"avif" | b"avis" => return Some((MediaFormat::Avif, "avif")),
                b"qt  " => return Some((MediaFormat::Mov, "mov")),
                _ => {
                    let limit = bytes.len().min(64);
                    if memchr::memmem::find(&bytes[..limit], b"avif").is_some() {
                        return Some((MediaFormat::Avif, "avif"));
                    }
                    return Some((MediaFormat::Mp4, "mp4"));
                }
            }
        }

        // QuickTime alternate header boxes
        if bytes.len() >= 8
            && (&bytes[4..8] == b"moov"
                || &bytes[4..8] == b"mdat"
                || &bytes[4..8] == b"free"
                || &bytes[4..8] == b"wide")
        {
            return Some((MediaFormat::Mov, "mov"));
        }

        None
    }
}
