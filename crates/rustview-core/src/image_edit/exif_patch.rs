//! Brings the EXIF block of an edited picture up to date without rewriting it (pure).
//!
//! EXIF keeps the pixel size of the picture and usually a small preview of it (the thumbnail in IFD1). After a crop both would
//! describe the old picture, and programs that show the embedded preview would show the uncropped one. Rewriting the whole
//! block with an EXIF library would move its parts around and break the maker notes of many cameras (they hold offsets into
//! the block), so the block is patched in place instead: the size fields get the new values (a SHORT field that cannot hold
//! the new value becomes a LONG, which fits in the same entry) and the link to IFD1 is cut, so the stale preview is no longer
//! part of the picture. Everything else - the orientation, the camera data, GPS - stays byte for byte.

const TAG_IMAGE_WIDTH: u16 = 0x0100;
const TAG_IMAGE_LENGTH: u16 = 0x0101;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_PIXEL_X: u16 = 0xA002;
const TAG_PIXEL_Y: u16 = 0xA003;
const TYPE_SHORT: u16 = 3;
const TYPE_LONG: u16 = 4;
/// An IFD with more entries than this is not a real one (a damaged block).
const MAX_ENTRIES: u16 = 1024;

/// The prefix of the EXIF block in a JPEG APP1 segment and in some other containers.
pub const EXIF_HEADER: &[u8] = b"Exif\0\0";

#[derive(Debug, Clone, Copy)]
enum Order {
    Little,
    Big,
}

struct Tiff<'a> {
    data: &'a mut [u8],
    order: Order,
}

impl Tiff<'_> {
    fn u16_at(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(at..at + 2)?.try_into().ok()?;
        Some(match self.order {
            Order::Little => u16::from_le_bytes(b),
            Order::Big => u16::from_be_bytes(b),
        })
    }

    fn u32_at(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(at..at + 4)?.try_into().ok()?;
        Some(match self.order {
            Order::Little => u32::from_le_bytes(b),
            Order::Big => u32::from_be_bytes(b),
        })
    }

    fn put_u16(&mut self, at: usize, value: u16) -> Option<()> {
        let bytes = match self.order {
            Order::Little => value.to_le_bytes(),
            Order::Big => value.to_be_bytes(),
        };
        self.data.get_mut(at..at + 2)?.copy_from_slice(&bytes);
        Some(())
    }

    fn put_u32(&mut self, at: usize, value: u32) -> Option<()> {
        let bytes = match self.order {
            Order::Little => value.to_le_bytes(),
            Order::Big => value.to_be_bytes(),
        };
        self.data.get_mut(at..at + 4)?.copy_from_slice(&bytes);
        Some(())
    }

    /// The offsets of the 12-byte entries of the IFD at `offset` and the offset of its link to the next IFD.
    fn entries(&self, offset: usize) -> Option<(Vec<usize>, usize)> {
        let count = self.u16_at(offset)?;
        if count > MAX_ENTRIES {
            return None;
        }
        let first = offset + 2;
        let entries: Vec<usize> = (0..usize::from(count)).map(|i| first + i * 12).collect();
        let next = first + usize::from(count) * 12;
        self.data.get(next..next + 4)?;
        Some((entries, next))
    }

    /// Writes `value` into the entry at `entry` (a SHORT or LONG field with one value).
    fn set_size_field(&mut self, entry: usize, value: u32) -> Option<()> {
        let kind = self.u16_at(entry + 2)?;
        let count = self.u32_at(entry + 4)?;
        if count != 1 || !(kind == TYPE_SHORT || kind == TYPE_LONG) {
            return None;
        }
        // The value field holds four bytes: a SHORT sits in the first two. Clear it and write the value as it fits.
        self.put_u32(entry + 8, 0)?;
        match u16::try_from(value) {
            Ok(short) if kind == TYPE_SHORT => self.put_u16(entry + 8, short),
            _ => {
                self.put_u16(entry + 2, TYPE_LONG)?;
                self.put_u32(entry + 8, value)
            }
        }
    }
}

/// What was changed in the block.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PatchReport {
    pub size_fields: u32,
    pub thumbnail_unlinked: bool,
}

/// Patches the EXIF block `exif` (with or without the "Exif\0\0" prefix) for a picture of `width` x `height`. A block that is
/// not valid TIFF is left as it is (the report says nothing was changed); it never panics.
pub fn patch_exif(exif: &mut [u8], (width, height): (u32, u32)) -> PatchReport {
    let start = if exif.starts_with(EXIF_HEADER) {
        EXIF_HEADER.len()
    } else {
        0
    };
    let Some(data) = exif.get_mut(start..) else {
        return PatchReport::default();
    };
    patch_tiff(data, width, height).unwrap_or_default()
}

fn patch_tiff(data: &mut [u8], width: u32, height: u32) -> Option<PatchReport> {
    let order = match data.get(0..4)? {
        [b'I', b'I', 42, 0] => Order::Little,
        [b'M', b'M', 0, 42] => Order::Big,
        _ => return None,
    };
    let mut tiff = Tiff { data, order };
    let ifd0 = usize::try_from(tiff.u32_at(4)?).ok()?;
    let (entries, next) = tiff.entries(ifd0)?;
    let mut report = PatchReport::default();
    let mut exif_ifd = None;
    for entry in entries {
        match tiff.u16_at(entry)? {
            TAG_IMAGE_WIDTH => report.size_fields += u32::from(tiff.set_size_field(entry, width).is_some()),
            TAG_IMAGE_LENGTH => report.size_fields += u32::from(tiff.set_size_field(entry, height).is_some()),
            TAG_EXIF_IFD => exif_ifd = tiff.u32_at(entry + 8).and_then(|o| usize::try_from(o).ok()),
            _ => {}
        }
    }
    if tiff.u32_at(next)? != 0 {
        tiff.put_u32(next, 0)?;
        report.thumbnail_unlinked = true;
    }
    if let Some(offset) = exif_ifd.filter(|o| *o != ifd0)
        && let Some((entries, _)) = tiff.entries(offset)
    {
        for entry in entries {
            match tiff.u16_at(entry) {
                Some(TAG_PIXEL_X) => report.size_fields += u32::from(tiff.set_size_field(entry, width).is_some()),
                Some(TAG_PIXEL_Y) => report.size_fields += u32::from(tiff.set_size_field(entry, height).is_some()),
                _ => {}
            }
        }
    }
    Some(report)
}

/// Reads the size fields back (for tests and the log): IFD0 width/length and EXIF pixel x/y, as far as present.
pub fn read_sizes(exif: &[u8]) -> Option<[Option<u32>; 4]> {
    let start = if exif.starts_with(EXIF_HEADER) {
        EXIF_HEADER.len()
    } else {
        0
    };
    let mut copy = exif.get(start..)?.to_vec();
    let order = match copy.get(0..4)? {
        [b'I', b'I', 42, 0] => Order::Little,
        [b'M', b'M', 0, 42] => Order::Big,
        _ => return None,
    };
    let tiff = Tiff { data: &mut copy, order };
    let value = |entry: usize| -> Option<u32> {
        match tiff.u16_at(entry + 2)? {
            TYPE_SHORT => tiff.u16_at(entry + 8).map(u32::from),
            TYPE_LONG => tiff.u32_at(entry + 8),
            _ => None,
        }
    };
    let ifd0 = usize::try_from(tiff.u32_at(4)?).ok()?;
    let mut sizes = [None; 4];
    let mut exif_ifd = None;
    for entry in tiff.entries(ifd0)?.0 {
        match tiff.u16_at(entry)? {
            TAG_IMAGE_WIDTH => sizes[0] = value(entry),
            TAG_IMAGE_LENGTH => sizes[1] = value(entry),
            TAG_EXIF_IFD => exif_ifd = tiff.u32_at(entry + 8).and_then(|o| usize::try_from(o).ok()),
            _ => {}
        }
    }
    if let Some((entries, _)) = exif_ifd.and_then(|o| tiff.entries(o)) {
        for entry in entries {
            match tiff.u16_at(entry) {
                Some(TAG_PIXEL_X) => sizes[2] = value(entry),
                Some(TAG_PIXEL_Y) => sizes[3] = value(entry),
                _ => {}
            }
        }
    }
    Some(sizes)
}

/// The offset of the IFD1 link of IFD0 (0 = no thumbnail linked).
pub fn thumbnail_link(exif: &[u8]) -> Option<u32> {
    let start = if exif.starts_with(EXIF_HEADER) {
        EXIF_HEADER.len()
    } else {
        0
    };
    let mut copy = exif.get(start..)?.to_vec();
    let order = match copy.get(0..4)? {
        [b'I', b'I', 42, 0] => Order::Little,
        [b'M', b'M', 0, 42] => Order::Big,
        _ => return None,
    };
    let tiff = Tiff { data: &mut copy, order };
    let ifd0 = usize::try_from(tiff.u32_at(4)?).ok()?;
    let (_, next) = tiff.entries(ifd0)?;
    tiff.u32_at(next)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A small EXIF block: IFD0 (width SHORT, length LONG, orientation, EXIF pointer, link to IFD1), the EXIF IFD (pixel x
    /// SHORT, pixel y LONG, a maker note placeholder) and IFD1 (a thumbnail tag).
    pub(crate) fn sample_exif(big_endian: bool) -> Vec<u8> {
        let mut out = Vec::new();
        let w16 = |out: &mut Vec<u8>, v: u16| {
            out.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
        };
        let w32 = |out: &mut Vec<u8>, v: u32| {
            out.extend_from_slice(&if big_endian { v.to_be_bytes() } else { v.to_le_bytes() });
        };
        out.extend_from_slice(EXIF_HEADER);
        out.extend_from_slice(if big_endian { b"MM\0*" } else { b"II*\0" });
        w32(&mut out, 8);
        let entry = |out: &mut Vec<u8>, tag: u16, kind: u16, value: u32| {
            w16(out, tag);
            w16(out, kind);
            w32(out, 1);
            if kind == TYPE_SHORT {
                w16(out, u16::try_from(value).unwrap_or(0));
                w16(out, 0);
            } else {
                w32(out, value);
            }
        };
        // IFD0 at 8: 4 entries -> 2 + 48 + 4 = 54 bytes, ends at 62
        w16(&mut out, 4);
        entry(&mut out, TAG_IMAGE_WIDTH, TYPE_SHORT, 4000);
        entry(&mut out, TAG_IMAGE_LENGTH, TYPE_LONG, 3000);
        entry(&mut out, 0x0112, TYPE_SHORT, 6);
        entry(&mut out, TAG_EXIF_IFD, TYPE_LONG, 62);
        w32(&mut out, 92);
        // EXIF IFD at 62: 2 entries -> 2 + 24 + 4 = 30 bytes, ends at 92
        w16(&mut out, 2);
        entry(&mut out, TAG_PIXEL_X, TYPE_SHORT, 4000);
        entry(&mut out, TAG_PIXEL_Y, TYPE_LONG, 3000);
        w32(&mut out, 0);
        // IFD1 at 92: one entry
        w16(&mut out, 1);
        entry(&mut out, 0x0201, TYPE_LONG, 0);
        w32(&mut out, 0);
        out
    }

    #[test]
    fn the_size_fields_get_the_new_size_and_the_thumbnail_is_unlinked() {
        for big_endian in [false, true] {
            let mut exif = sample_exif(big_endian);
            assert_eq!(
                read_sizes(&exif),
                Some([Some(4000), Some(3000), Some(4000), Some(3000)])
            );
            assert_eq!(thumbnail_link(&exif), Some(92));
            let report = patch_exif(&mut exif, (893, 70_000));
            assert_eq!(
                report,
                PatchReport {
                    size_fields: 4,
                    thumbnail_unlinked: true
                }
            );
            // 70 000 does not fit a SHORT: those fields became LONGs.
            assert_eq!(
                read_sizes(&exif),
                Some([Some(893), Some(70_000), Some(893), Some(70_000)])
            );
            assert_eq!(thumbnail_link(&exif), Some(0));
            // Everything else stays: the orientation entry is unchanged and the block keeps its length.
            assert_eq!(exif.len(), sample_exif(big_endian).len());
        }
    }

    #[test]
    fn only_the_size_fields_and_the_link_change() {
        let original = sample_exif(false);
        let mut exif = original.clone();
        patch_exif(&mut exif, (4000, 3000));
        let changed: Vec<usize> = original
            .iter()
            .zip(&exif)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        // Same size: only bytes of the IFD1 link (after the header, IFD0 at 8 with its 4 entries) differ.
        let link = EXIF_HEADER.len() + 8 + 2 + 4 * 12;
        assert_ne!(changed.len(), 0);
        assert!(changed.iter().all(|i| (link..link + 4).contains(i)), "{changed:?}");
    }

    #[test]
    fn damaged_blocks_are_left_alone() {
        for mut junk in [
            Vec::new(),
            b"Exif\0\0".to_vec(),
            b"Exif\0\0XX*\0\x08\0\0\0".to_vec(),
            b"II*\0\xff\xff\xff\xff".to_vec(),
            b"II*\0\x08\0\0\0\xff\xff".to_vec(),
        ] {
            let before = junk.clone();
            assert_eq!(patch_exif(&mut junk, (1, 1)), PatchReport::default());
            assert_eq!(junk, before);
        }
        // A block cut off in the middle of IFD0 is not touched past its end.
        let mut cut = sample_exif(false);
        cut.truncate(40);
        let _ = patch_exif(&mut cut, (1, 1));
    }
}
