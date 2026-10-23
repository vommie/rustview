//! The edit state of one image tile: what is known about its file (for saving), the applied edits, the crop tool while it is
//! open, and the picture the tile showed before the edits (to draw the edited picture at once, before a sharper one arrives).

use super::claims::EditClaim;
use rustview_core::image_edit::conflict::FileIdentity;
use rustview_core::image_edit::crop_tool::CropTool;
use rustview_core::image_edit::formats::{EditRefusal, SaveSupport, save_support};
use rustview_core::image_edit::geometry::PixelRect;
use rustview_core::image_edit::probe::SourceProbe;
use rustview_core::image_edit::stack::EditStack;
use rustview_core::media::MediaFormat;
use rustview_core::util::cast::{Az, FloatToInt};
use slint::{Rgb8Pixel, SharedPixelBuffer};

/// What the decoder learned about the file of an image tile (read while it decoded the picture).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditSource {
    pub identity: Option<FileIdentity>,
    pub probe: Result<rustview_core::image_edit::probe::SourceProbe, String>,
    /// The format of the extension (what the library knows).
    pub format: MediaFormat,
    /// Size of the decoded original: the edits are made on it.
    pub size: (u32, u32),
}

impl EditSource {
    /// Whether the picture can be edited, and how it is saved.
    pub fn support(&self) -> Result<SaveSupport, EditRefusal> {
        match &self.probe {
            Ok(probe) => save_support(self.format, probe),
            Err(_) => Err(EditRefusal::Unreadable),
        }
    }

    pub fn probe(&self) -> Option<&SourceProbe> {
        self.probe.as_ref().ok()
    }

    /// The block grid a crop keeps to: CMYK JPEGs always (they are only cropped without loss), other JPEGs when the user
    /// wants every JPEG crop to be lossless.
    pub fn crop_grid(&self, align_jpeg: bool) -> Option<(u32, u32)> {
        let layout = self.probe()?.jpeg?;
        let in_place = matches!(self.support(), Ok(SaveSupport::InPlace(_)));
        (in_place && (layout.is_cmyk() || align_jpeg)).then_some(layout.mcu)
    }
}

pub struct TileEdit {
    pub stack: EditStack,
    pub tool: Option<CropTool>,
    /// The right to edit this medium (no other tile edits it meanwhile).
    pub claim: EditClaim,
    /// The tile's picture before the edits (display size).
    pub base_pixels: SharedPixelBuffer<Rgb8Pixel>,
    /// Changes with every change of the edits; a sharp picture decoded for an older state is dropped.
    pub revision: u64,
    /// The file is being written right now (no edits meanwhile).
    pub saving: bool,
}

impl TileEdit {
    pub fn new(claim: EditClaim, original: (u32, u32), base_pixels: SharedPixelBuffer<Rgb8Pixel>) -> Self {
        Self {
            stack: EditStack::new(original),
            tool: None,
            claim,
            base_pixels,
            revision: 0,
            saving: false,
        }
    }

    /// Still needed: a tool is open, edits are unsaved or the file is being written.
    pub fn is_live(&self) -> bool {
        self.tool.is_some() || self.stack.is_dirty() || self.saving
    }
}

/// The part `crop` of the original, cut out of `base` (the display-size picture of the original of size `original`): what the
/// tile shows right after an edit, until the sharp picture of the edited state is decoded.
pub fn crop_display(
    base: &SharedPixelBuffer<Rgb8Pixel>,
    original: (u32, u32),
    crop: PixelRect,
) -> SharedPixelBuffer<Rgb8Pixel> {
    let (bw, bh) = (base.width(), base.height());
    let scale_x = f64::from(bw) / f64::from(original.0.max(1));
    let scale_y = f64::from(bh) / f64::from(original.1.max(1));
    let edge = |v: u32, scale: f64, limit: u32| (f64::from(v) * scale).round().to_int::<u32>().min(limit);
    let x0 = edge(crop.x, scale_x, bw.saturating_sub(1));
    let y0 = edge(crop.y, scale_y, bh.saturating_sub(1));
    let x1 = edge(crop.right(), scale_x, bw).max(x0 + 1);
    let y1 = edge(crop.bottom(), scale_y, bh).max(y0 + 1);
    let (w, h) = (x1 - x0, y1 - y0);
    let mut out = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
    let stride = usize::try_from(bw).unwrap_or(0) * 3;
    let row = usize::try_from(w).unwrap_or(0) * 3;
    let source = base.as_bytes();
    let target = out.make_mut_bytes();
    for y in 0..usize::try_from(h).unwrap_or(0) {
        let from = (usize::try_from(y0).unwrap_or(0) + y) * stride + usize::try_from(x0).unwrap_or(0) * 3;
        if let (Some(src), Some(dst)) = (source.get(from..from + row), target.get_mut(y * row..(y + 1) * row)) {
            dst.copy_from_slice(src);
        }
    }
    out
}

/// The aspect ratio of a size (width / height).
pub fn aspect_of((w, h): (u32, u32)) -> f32 {
    w.az::<f32>() / h.max(1).az::<f32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_display_crop_scales_the_rectangle_to_the_display_picture() {
        // A 4x2 display picture of an 8x4 original; each pixel has its x as red.
        let mut base = SharedPixelBuffer::<Rgb8Pixel>::new(4, 2);
        for (i, px) in base.make_mut_slice().iter_mut().enumerate() {
            *px = Rgb8Pixel::new(u8::try_from(i % 4).unwrap_or(0), u8::try_from(i / 4).unwrap_or(0), 0);
        }
        let out = crop_display(&base, (8, 4), PixelRect::new(2, 2, 4, 2));
        assert_eq!((out.width(), out.height()), (2, 1));
        let reds: Vec<u8> = out.as_slice().iter().map(|p| p.r).collect();
        assert_eq!(reds, [1, 2]);
        assert!(out.as_slice().iter().all(|p| p.g == 1));
        // A crop smaller than one display pixel still gives one pixel.
        let tiny = crop_display(&base, (8, 4), PixelRect::new(7, 3, 1, 1));
        assert_eq!((tiny.width(), tiny.height()), (1, 1));
    }

    #[test]
    fn the_grid_applies_to_jpegs_that_are_overwritten() {
        use rustview_core::image_edit::probe::JpegLayout;
        let jpeg = |components: u8| EditSource {
            identity: None,
            probe: Ok(SourceProbe {
                signature: Some(MediaFormat::Jpeg),
                jpeg: Some(JpegLayout {
                    mcu: (16, 16),
                    components,
                    progressive: false,
                }),
                ..SourceProbe::default()
            }),
            format: MediaFormat::Jpeg,
            size: (100, 100),
        };
        assert_eq!(jpeg(3).crop_grid(false), None);
        assert_eq!(jpeg(3).crop_grid(true), Some((16, 16)));
        assert_eq!(jpeg(4).crop_grid(false), Some((16, 16)));
        let unreadable = EditSource {
            probe: Err("gone".to_string()),
            ..jpeg(3)
        };
        assert_eq!(unreadable.support(), Err(EditRefusal::Unreadable));
        assert_eq!(unreadable.crop_grid(true), None);
    }
}
