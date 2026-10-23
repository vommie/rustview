//! The picture of the drag ghost: a rounded card in the theme colors with the thumbnail (or the generic file icon), the file name and,
//! for several files, a count badge. Drawn as one SVG by resvg, so text and rounded corners come out anti-aliased.

use crate::viewer::x11::GhostPixels;
use base64::Engine as _;
use resvg::{tiny_skia, usvg};
use rustview_core::util::cast::{Az, FloatToInt};
use std::path::Path;
use std::sync::LazyLock;

const MARGIN_START: f32 = 10.0;
const MARGIN_END: f32 = 12.0;
const MARGIN_VERTICAL: f32 = 6.0;
const SPACING: f32 = 8.0;
const THUMB_PX: f32 = 24.0;
const TITLE_SIZE: f32 = 11.0;
const BADGE_SIZE: f32 = 10.0;
const BADGE_PADDING_H: f32 = 6.0;
const BADGE_PADDING_V: f32 = 2.0;
const CARD_RADIUS: f32 = 8.0;
const BORDER_WIDTH: f32 = 1.5;
/// The file name is cut off after this many characters.
const MAX_TITLE_CHARS: usize = 28;

const SURFACE: &str = "#1e1e2e";
const SURFACE_OPACITY: &str = "0.96";
const PRIMARY: &str = "#89b4fa";
const TEXT: &str = "#cdd6f4";
const ON_PRIMARY: &str = "#11111b";

/// What the ghost shows.
pub struct GhostSpec<'a> {
    pub title: &'a str,
    pub count: usize,
    pub thumbnail: Option<&'a Path>,
}

/// The font of the ghost: the first of the usual desktop fonts that is installed, otherwise any font that is.
fn font_family() -> &'static str {
    static FAMILY: LazyLock<String> = LazyLock::new(|| {
        let database = &rustview_core::media::svg::options().fontdb;
        let installed = |wanted: &str| {
            database
                .faces()
                .any(|face| face.families.iter().any(|(family, _)| family == wanted))
        };
        [
            "Noto Sans",
            "DejaVu Sans",
            "Liberation Sans",
            "Open Sans",
            "Ubuntu",
            "Cantarell",
        ]
        .into_iter()
        .find(|wanted| installed(wanted))
        .map(str::to_owned)
        .or_else(|| {
            database
                .faces()
                .next()
                .and_then(|face| face.families.first().map(|(family, _)| family.clone()))
        })
        .unwrap_or_else(|| "sans-serif".to_owned())
    });
    &FAMILY
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `text` cut to `MAX_TITLE_CHARS` characters, with an ellipsis when something was cut.
fn ellipsize(text: &str) -> String {
    if text.chars().count() <= MAX_TITLE_CHARS {
        return text.to_owned();
    }
    let kept: String = text.chars().take(MAX_TITLE_CHARS - 1).collect();
    format!("{kept}\u{2026}")
}

/// The width of a bold text line, measured by laying it out like the picture does (a rough estimate when no font can be loaded).
fn text_width(text: &str, size: f32) -> f32 {
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="2000" height="60"><text x="0" y="40" font-family="{}" font-size="{size}" font-weight="bold">{}</text></svg>"#,
        escape_xml(font_family()),
        escape_xml(text)
    );
    usvg::Tree::from_str(&svg, rustview_core::media::svg::options())
        .ok()
        .map(|tree| tree.root().abs_bounding_box().width())
        .filter(|width| *width > 0.0)
        .unwrap_or_else(|| text.chars().count().az::<f32>() * size * 0.6)
}

/// The thumbnail scaled into the 24 px box (aspect ratio kept) as an `<image>` element at `x`, `y`; `None` when it cannot be read.
fn thumbnail_element(path: &Path, x: f32, y: f32) -> Option<String> {
    let thumbnail = image::open(path)
        .ok()?
        .thumbnail(THUMB_PX.to_int::<u32>(), THUMB_PX.to_int::<u32>())
        .to_rgba8();
    let (width, height) = (thumbnail.width().az::<f32>(), thumbnail.height().az::<f32>());
    let mut png = std::io::Cursor::new(Vec::new());
    thumbnail.write_to(&mut png, image::ImageFormat::Png).ok()?;
    let data = base64::engine::general_purpose::STANDARD.encode(png.into_inner());
    let (left, top) = (x + (THUMB_PX - width) / 2.0, y + (THUMB_PX - height) / 2.0);
    Some(format!(
        r#"<clipPath id="thumb"><rect x="{left}" y="{top}" width="{width}" height="{height}" rx="3"/></clipPath>
<image x="{left}" y="{top}" width="{width}" height="{height}" clip-path="url(#thumb)" href="data:image/png;base64,{data}"/>"#
    ))
}

/// Draws the ghost. With `opaque` (no compositing manager: the alpha channel would not be blended) the card has square corners
/// and a solid background.
pub fn render(spec: &GhostSpec<'_>, opaque: bool) -> Option<GhostPixels> {
    let title = ellipsize(spec.title);
    let title_width = text_width(&title, TITLE_SIZE);
    let badge = (spec.count > 1).then(|| format!("{} Files", spec.count));
    let badge_width = badge
        .as_ref()
        .map(|text| text_width(text, BADGE_SIZE) + 2.0 * BADGE_PADDING_H);

    let content_height = THUMB_PX;
    let height = content_height + 2.0 * MARGIN_VERTICAL;
    let thumbnail = spec
        .thumbnail
        .and_then(|path| thumbnail_element(path, MARGIN_START, MARGIN_VERTICAL));
    let title_x = if thumbnail.is_some() {
        MARGIN_START + THUMB_PX + SPACING
    } else {
        MARGIN_START
    };
    let title_baseline = MARGIN_VERTICAL + content_height / 2.0 + TITLE_SIZE * 0.35;
    let mut right = title_x + title_width;

    let mut badge_elements = String::new();
    if let (Some(text), Some(width)) = (&badge, badge_width) {
        let (x, badge_height) = (right + SPACING, BADGE_SIZE * 1.2 + 2.0 * BADGE_PADDING_V);
        let y = MARGIN_VERTICAL + (content_height - badge_height) / 2.0;
        let baseline = MARGIN_VERTICAL + content_height / 2.0 + BADGE_SIZE * 0.35;
        badge_elements = format!(
            r#"<rect x="{x}" y="{y}" width="{width}" height="{badge_height}" rx="4" fill="{PRIMARY}"/>
<text x="{}" y="{baseline}" font-family="{}" font-size="{BADGE_SIZE}" font-weight="bold" fill="{ON_PRIMARY}">{}</text>"#,
            x + BADGE_PADDING_H,
            escape_xml(font_family()),
            escape_xml(text)
        );
        right = x + width;
    }
    let width = (right + MARGIN_END).ceil();

    let radius = if opaque { 0.0 } else { CARD_RADIUS };
    let background = if opaque {
        format!(r#"<rect width="{width}" height="{height}" fill="{SURFACE}"/>"#)
    } else {
        String::new()
    };
    let half = BORDER_WIDTH / 2.0;
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
{background}
<rect x="{half}" y="{half}" width="{}" height="{}" rx="{radius}" fill="{SURFACE}" fill-opacity="{SURFACE_OPACITY}" stroke="{PRIMARY}" stroke-width="{BORDER_WIDTH}"/>
{}
<text x="{title_x}" y="{title_baseline}" font-family="{}" font-size="{TITLE_SIZE}" font-weight="bold" fill="{TEXT}">{}</text>
{badge_elements}
</svg>"#,
        width - BORDER_WIDTH,
        height - BORDER_WIDTH,
        thumbnail.unwrap_or_default(),
        escape_xml(font_family()),
        escape_xml(&title)
    );

    let tree = usvg::Tree::from_str(&svg, rustview_core::media::svg::options()).ok()?;
    let (pixel_width, pixel_height) = (width.to_int::<u32>(), height.to_int::<u32>());
    let mut pixmap = tiny_skia::Pixmap::new(pixel_width, pixel_height)?;
    resvg::render(&tree, tiny_skia::Transform::identity(), &mut pixmap.as_mut());

    // tiny-skia stores pre-multiplied RGBA; the X server wants pre-multiplied BGRA (a pixel is the little-endian word ARGB).
    let bgra = pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            [
                pixel.blue(),
                pixel.green(),
                pixel.red(),
                if opaque { 255 } else { pixel.alpha() },
            ]
        })
        .collect();
    Some(GhostPixels {
        width: u16::try_from(pixel_width).ok()?,
        height: u16::try_from(pixel_height).ok()?,
        bgra,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(title: &str, count: usize) -> GhostSpec<'_> {
        GhostSpec {
            title,
            count,
            thumbnail: None,
        }
    }

    #[test]
    fn long_names_are_cut_with_an_ellipsis() {
        assert_eq!(ellipsize("short.jpg"), "short.jpg");
        let cut = ellipsize(&"x".repeat(60));
        assert_eq!(cut.chars().count(), MAX_TITLE_CHARS);
        assert!(cut.ends_with('\u{2026}'));
    }

    #[test]
    fn the_card_has_the_height_of_a_thumbnail_plus_margins() {
        let pixels = render(&spec("photo.jpg", 1), false).expect("a picture");
        assert_eq!(f32::from(pixels.height), THUMB_PX + 2.0 * MARGIN_VERTICAL);
        assert_eq!(
            pixels.bgra.len(),
            usize::from(pixels.width) * usize::from(pixels.height) * 4
        );
        assert!(pixels.width > 40);
    }

    #[test]
    fn several_files_get_a_wider_card_with_a_badge() {
        let one = render(&spec("photo.jpg", 1), false).expect("a picture");
        let many = render(&spec("photo.jpg", 12), false).expect("a picture");
        assert!(many.width > one.width);
    }

    #[test]
    fn the_rounded_card_has_transparent_corners_and_the_opaque_one_has_none() {
        let rounded = render(&spec("photo.jpg", 1), false).expect("a picture");
        assert_eq!(
            rounded.bgra[3], 0,
            "the top left corner pixel of a rounded card is transparent"
        );
        let square = render(&spec("photo.jpg", 1), true).expect("a picture");
        assert!(square.bgra.as_chunks::<4>().0.iter().all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn a_missing_thumbnail_still_draws_the_card() {
        let spec = GhostSpec {
            title: "x.jpg",
            count: 1,
            thumbnail: Some(Path::new("/does/not/exist.png")),
        };
        assert!(render(&spec, false).is_some());
    }

    #[test]
    fn xml_special_characters_do_not_break_the_picture() {
        assert!(render(&spec("a & b <c> \"d\".jpg", 2), false).is_some());
    }
}
