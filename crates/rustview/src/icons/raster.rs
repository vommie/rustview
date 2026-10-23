//! Rust-side icon rasterizer for places that cannot use Slint's `AppIcon` (e.g. native helper windows).
//!
//! The SVG source is recolored (`currentColor` -> theme color) and rendered with the same rasterizer Slint
//! uses internally, at the exact requested pixel size.

use super::Icon;
use resvg::{tiny_skia, usvg};
use rustview_core::util::cast::Az;
use std::fmt;
use std::path::Path;

/// Failure while rasterizing an icon.
#[derive(Debug)]
pub enum RasterError {
    InvalidSize(u32),
    Parse(String),
    Encode(String),
    Io(std::io::Error),
}

impl fmt::Display for RasterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize(size) => write!(f, "invalid icon size {size}px"),
            Self::Parse(e) => write!(f, "cannot parse icon SVG: {e}"),
            Self::Encode(e) => write!(f, "cannot encode icon PNG: {e}"),
            Self::Io(e) => write!(f, "cannot write icon: {e}"),
        }
    }
}

impl std::error::Error for RasterError {}

/// Returns the SVG source of an icon with `currentColor` replaced by `color` (`#rrggbb`).
/// Colored icons (ratings) are returned unchanged.
pub fn recolor_svg(icon: Icon, color: &str) -> String {
    if icon.is_monochrome() {
        icon.svg().replace("currentColor", color)
    } else {
        icon.svg().to_owned()
    }
}

/// Renders an icon into straight (non-premultiplied) RGBA pixels of `size_px` x `size_px`.
pub fn render_rgba(icon: Icon, size_px: u32, color: &str) -> Result<Vec<u8>, RasterError> {
    let mut pixmap = tiny_skia::Pixmap::new(size_px, size_px).ok_or(RasterError::InvalidSize(size_px))?;
    let svg = recolor_svg(icon, color);
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).map_err(|e| RasterError::Parse(e.to_string()))?;

    let scale = size_px.az::<f32>() / tree.size().width().max(1.0);
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    Ok(pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect())
}

/// Renders an icon and writes it as PNG to `dest`.
pub fn render_png(icon: Icon, size_px: u32, color: &str, dest: &Path) -> Result<(), RasterError> {
    let rgba = render_rgba(icon, size_px, color)?;
    let image = image::RgbaImage::from_raw(size_px, size_px, rgba)
        .ok_or_else(|| RasterError::Encode("pixel buffer size mismatch".to_owned()))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(RasterError::Io)?;
    }
    image
        .save_with_format(dest, image::ImageFormat::Png)
        .map_err(|e| RasterError::Encode(e.to_string()))?;
    tracing::debug!(icon = icon.id(), size_px, path = %dest.display(), "[Icons] Rendered icon PNG");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recolors_monochrome_icons_only() {
        assert!(recolor_svg(Icon::File, "#ff0000").contains("#ff0000"));
        assert!(!recolor_svg(Icon::File, "#ff0000").contains("currentColor"));
        assert_eq!(recolor_svg(Icon::Rating0, "#ff0000"), Icon::Rating0.svg());
    }

    #[test]
    fn renders_visible_pixels_in_the_requested_color() {
        let rgba = render_rgba(Icon::File, 24, "#336699").expect("render");
        assert_eq!(rgba.len(), 24 * 24 * 4);
        let visible: Vec<&[u8]> = rgba.chunks(4).filter(|p| p[3] == 255).collect();
        assert!(!visible.is_empty(), "icon must contain opaque pixels");
        assert!(visible.iter().all(|p| p[..3] == [0x33, 0x66, 0x99]));
    }

    #[test]
    fn rejects_zero_size() {
        assert!(matches!(
            render_rgba(Icon::File, 0, "#000000"),
            Err(RasterError::InvalidSize(0))
        ));
    }
}
