//! SVG files are rasterized by resvg: FFmpeg's librsvg ignores the `viewBox` of a file without width and height (it shows
//! 100x100), and resvg also draws text with the installed fonts. Files the SVG references are never loaded.

use resvg::{tiny_skia, usvg};
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, OnceLock};
use thiserror::Error;

/// Larger files are not read (an SVG is text; this is generous).
const MAX_SVG_BYTES: u64 = 64 * 1024 * 1024;
/// The viewer renders at least this many pixels on the long edge so zooming into a small icon stays sharp.
pub const VIEWER_MIN_EDGE: u32 = 2048;
pub const VIEWER_MAX_EDGE: u32 = 4096;

#[derive(Debug, Error)]
pub enum SvgError {
    #[error("I/O error reading the SVG: {0}")]
    Io(#[from] std::io::Error),
    #[error("The SVG file is larger than {} MB", MAX_SVG_BYTES / (1024 * 1024))]
    TooLarge,
    #[error("The SVG could not be parsed: {0}")]
    Parse(String),
    #[error("The SVG has no drawable size")]
    EmptySize,
}

/// A rasterized SVG on a white background (RGB8) and the size the file declares.
pub struct SvgRaster {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    pub intrinsic_width: u32,
    pub intrinsic_height: u32,
}

/// Parse options shared by every SVG: system fonts are loaded once (on the first SVG), and nothing outside the file
/// is read (`href`s to other files resolve to nothing; embedded `data:` pictures work).
fn options() -> &'static usvg::Options<'static> {
    static OPTIONS: OnceLock<usvg::Options<'static>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let mut fonts = usvg::fontdb::Database::new();
        fonts.load_system_fonts();
        let mut options = usvg::Options {
            fontdb: Arc::new(fonts),
            ..usvg::Options::default()
        };
        options.image_href_resolver.resolve_string = Box::new(|_, _| None);
        options
    })
}

fn load_tree(path: &Path) -> Result<usvg::Tree, SvgError> {
    let mut data = Vec::new();
    File::open(path)?.take(MAX_SVG_BYTES + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_SVG_BYTES {
        return Err(SvgError::TooLarge);
    }
    usvg::Tree::from_data(&data, options()).map_err(|e| SvgError::Parse(e.to_string()))
}

fn intrinsic(tree: &usvg::Tree) -> Result<(u32, u32), SvgError> {
    let size = tree.size();
    let (w, h) = (size.width().ceil(), size.height().ceil());
    if !(w >= 1.0 && h >= 1.0 && w <= 1.0e6 && h <= 1.0e6) {
        return Err(SvgError::EmptySize);
    }
    Ok((w as u32, h as u32))
}

/// The size the file declares (its `width` and `height`, else its `viewBox`).
pub fn intrinsic_size(path: &Path) -> Result<(u32, u32), SvgError> {
    intrinsic(&load_tree(path)?)
}

/// Draws the SVG so its long edge is `longest_edge` pixels (scaled up or down, aspect ratio kept).
pub fn rasterize(path: &Path, longest_edge: u32) -> Result<SvgRaster, SvgError> {
    let tree = load_tree(path)?;
    let (iw, ih) = intrinsic(&tree)?;
    let scale = longest_edge.max(1) as f32 / iw.max(ih) as f32;
    let width = ((iw as f32 * scale).round() as u32).max(1);
    let height = ((ih as f32 * scale).round() as u32).max(1);

    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(SvgError::EmptySize)?;
    pixmap.fill(tiny_skia::Color::WHITE);
    let transform = tiny_skia::Transform::from_scale(width as f32 / tree.size().width(), height as f32 / tree.size().height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());

    // The background is opaque, so the premultiplied channels are the colors.
    let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
    for px in pixmap.data().as_chunks::<4>().0 {
        rgb.extend_from_slice(&px[..3]);
    }
    Ok(SvgRaster {
        width,
        height,
        rgb,
        intrinsic_width: iw,
        intrinsic_height: ih,
    })
}

/// The edge the viewer renders an SVG with: its own size, kept between `VIEWER_MIN_EDGE` and `VIEWER_MAX_EDGE`.
pub fn viewer_edge(intrinsic_width: u32, intrinsic_height: u32) -> u32 {
    intrinsic_width.max(intrinsic_height).clamp(VIEWER_MIN_EDGE, VIEWER_MAX_EDGE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_svg(body: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::Builder::new().suffix(".svg").tempfile().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f
    }

    #[test]
    fn a_file_without_width_and_height_uses_its_view_box() {
        let f = temp_svg(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 480 320"><rect width="480" height="320" fill="red"/></svg>"#);
        assert_eq!(intrinsic_size(f.path()).unwrap(), (480, 320));
        let raster = rasterize(f.path(), 240).unwrap();
        assert_eq!((raster.width, raster.height), (240, 160));
        assert_eq!(&raster.rgb[..3], &[255, 0, 0]);
    }

    #[test]
    fn transparent_areas_are_white_and_text_is_drawn() {
        let f = temp_svg(r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="32"><circle cx="8" cy="8" r="4" fill="blue"/></svg>"#);
        let raster = rasterize(f.path(), 64).unwrap();
        let last = raster.rgb.len() - 3;
        assert_eq!(&raster.rgb[last..], &[255, 255, 255]);
    }

    #[test]
    fn broken_and_empty_files_fail_cleanly() {
        assert!(matches!(intrinsic_size(temp_svg("not xml").path()), Err(SvgError::Parse(_))));
        assert!(intrinsic_size(temp_svg(r#"<svg xmlns="http://www.w3.org/2000/svg" width="0" height="0"/>"#).path()).is_err());
    }

    #[test]
    fn files_the_svg_points_to_are_not_loaded() {
        let f = temp_svg(r#"<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16"><image href="/etc/hostname" width="16" height="16"/></svg>"#);
        assert!(rasterize(f.path(), 16).is_ok());
    }

    #[test]
    fn the_viewer_edge_is_clamped() {
        assert_eq!(viewer_edge(24, 24), VIEWER_MIN_EDGE);
        assert_eq!(viewer_edge(3000, 100), 3000);
        assert_eq!(viewer_edge(20000, 10), VIEWER_MAX_EDGE);
    }
}
