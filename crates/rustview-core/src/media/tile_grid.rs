//! Large HEIC and some AVIF pictures are stored as a grid of tiles (iPhone photos: 512x512 tiles). FFmpeg's demuxer hands
//! the tiles over as separate video streams plus a "tile grid" stream group; only the `ffmpeg` command puts them together.
//! This module does the same: it decodes every tile, places it, crops to the picture and applies the stored rotation.

use super::orientation;
use ffmpeg_next::format::Pixel;
use ffmpeg_next::software::scaling::context::Context as SwsContext;
use ffmpeg_next::software::scaling::flag::Flags as SwsFlags;
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// A grid larger than this (in canvas bytes) is refused instead of allocated.
const MAX_CANVAS_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum GridError {
    #[error("FFmpeg error: {0}")]
    Ffmpeg(String),
    #[error("The tile grid is invalid or too large: {0}")]
    Invalid(String),
}

impl From<ffmpeg_next::Error> for GridError {
    fn from(e: ffmpeg_next::Error) -> Self {
        Self::Ffmpeg(e.to_string())
    }
}

/// The composed picture (RGB8, already rotated).
pub struct GridImage {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Placement {
    stream_index: usize,
    x: i32,
    y: i32,
}

/// What the stream group says about the picture.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GridLayout {
    coded_width: u32,
    coded_height: u32,
    crop_x: u32,
    crop_y: u32,
    width: u32,
    height: u32,
    background: [u8; 3],
    rotation: i32,
    tiles: Vec<Placement>,
}

impl GridLayout {
    /// The size the picture has on screen (after the rotation).
    fn display_size(&self) -> (u32, u32) {
        if orientation::swaps_axes(self.rotation) {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }

    fn validate(&self) -> Result<(), GridError> {
        let canvas = u64::from(self.coded_width) * u64::from(self.coded_height) * 3;
        if self.tiles.is_empty() || self.width == 0 || self.height == 0 || canvas == 0 || canvas > MAX_CANVAS_BYTES {
            return Err(GridError::Invalid(format!(
                "{} tiles, canvas {}x{}",
                self.tiles.len(),
                self.coded_width,
                self.coded_height
            )));
        }
        if self.crop_x + self.width > self.coded_width || self.crop_y + self.height > self.coded_height {
            return Err(GridError::Invalid("the picture reaches beyond the canvas".into()));
        }
        Ok(())
    }
}

/// Reads the layout of the largest tile grid of the file, `None` when the file has none.
fn find_layout(ictx: &ffmpeg_next::format::context::Input) -> Option<GridLayout> {
    use ffmpeg_next::ffi::{AVPacketSideDataType, AVStreamGroupParamsType};

    let mut best: Option<GridLayout> = None;
    // SAFETY: the pointers belong to the open input context, which outlives this function; every value is copied out
    // before it returns, and the indexes are checked against the counts FFmpeg reports.
    unsafe {
        let context = ictx.as_ptr();
        for i in 0..(*context).nb_stream_groups as usize {
            let group = *(*context).stream_groups.add(i);
            if group.is_null() || (*group).type_ != AVStreamGroupParamsType::AV_STREAM_GROUP_PARAMS_TILE_GRID {
                continue;
            }
            let grid = (*group).params.tile_grid;
            if grid.is_null() || (*grid).offsets.is_null() {
                continue;
            }
            let grid = &*grid;

            let mut tiles = Vec::with_capacity(grid.nb_tiles as usize);
            for t in 0..grid.nb_tiles as usize {
                let offset = *grid.offsets.add(t);
                if offset.idx >= (*group).nb_streams {
                    continue;
                }
                let stream = *(*group).streams.add(offset.idx as usize);
                if stream.is_null() {
                    continue;
                }
                tiles.push(Placement {
                    stream_index: (*stream).index as usize,
                    x: offset.horizontal,
                    y: offset.vertical,
                });
            }

            let mut rotation = 0;
            for d in 0..grid.nb_coded_side_data.max(0) as usize {
                let side_data = &*grid.coded_side_data.add(d);
                if side_data.type_ == AVPacketSideDataType::AV_PKT_DATA_DISPLAYMATRIX && !side_data.data.is_null() {
                    let bytes = std::slice::from_raw_parts(side_data.data, side_data.size);
                    rotation = orientation::clockwise_degrees_of_matrix(bytes).unwrap_or(0);
                }
            }

            let layout = GridLayout {
                coded_width: grid.coded_width.max(0) as u32,
                coded_height: grid.coded_height.max(0) as u32,
                crop_x: grid.horizontal_offset.max(0) as u32,
                crop_y: grid.vertical_offset.max(0) as u32,
                width: grid.width.max(0) as u32,
                height: grid.height.max(0) as u32,
                background: [grid.background[0], grid.background[1], grid.background[2]],
                rotation,
                tiles,
            };
            let area = u64::from(layout.width) * u64::from(layout.height);
            if best
                .as_ref()
                .is_none_or(|b| area > u64::from(b.width) * u64::from(b.height))
            {
                best = Some(layout);
            }
        }
    }
    best
}

/// The size of the tile-grid picture of a file as it is shown (rotation applied), `None` for files without a grid.
pub fn display_size(path: &Path) -> Result<Option<(u32, u32)>, GridError> {
    super::metadata::ensure_ffmpeg_initialized();
    let ictx = ffmpeg_next::format::input(path)?;
    Ok(find_layout(&ictx).map(|l| l.display_size()))
}

/// Decodes and composes the tile-grid picture of a file, `None` for files without a grid.
pub fn decode(path: &Path) -> Result<Option<GridImage>, GridError> {
    super::metadata::ensure_ffmpeg_initialized();
    let mut ictx = ffmpeg_next::format::input(path)?;
    let Some(layout) = find_layout(&ictx) else {
        return Ok(None);
    };
    layout.validate()?;

    let mut canvas = filled_canvas(&layout);
    let mut placements: HashMap<usize, Vec<(i32, i32)>> = HashMap::new();
    for tile in &layout.tiles {
        placements.entry(tile.stream_index).or_default().push((tile.x, tile.y));
    }

    let mut decoders = HashMap::new();
    for &index in placements.keys() {
        let stream = ictx
            .stream(index)
            .ok_or_else(|| GridError::Invalid(format!("tile stream {index} is missing")))?;
        let context = ffmpeg_next::codec::context::Context::from_parameters(stream.parameters())?;
        decoders.insert(index, context.decoder().video()?);
    }

    let mut placed = 0usize;
    for (stream, packet) in ictx.packets() {
        let index = stream.index();
        let Some(decoder) = decoders.get_mut(&index) else {
            continue;
        };
        if decoder.send_packet(&packet).is_err() {
            continue;
        }
        let _ = decoder.send_eof();
        let mut frame = ffmpeg_next::frame::Video::empty();
        if decoder.receive_frame(&mut frame).is_ok()
            && let Some(rgb) = frame_to_rgb(&frame)
        {
            let tile = RgbTile {
                data: &rgb,
                width: frame.width(),
                height: frame.height(),
            };
            for &(x, y) in &placements[&index] {
                blit(&mut canvas, layout.coded_width, layout.coded_height, &tile, x, y);
            }
            placed += placements[&index].len();
            decoders.remove(&index);
        }
        if decoders.is_empty() {
            break;
        }
    }
    if placed == 0 {
        return Err(GridError::Invalid("no tile could be decoded".into()));
    }

    let cropped = crop(
        &canvas,
        layout.coded_width,
        layout.crop_x,
        layout.crop_y,
        layout.width,
        layout.height,
    );
    let (width, height, rgb) = rotate(cropped, layout.width, layout.height, layout.rotation);
    Ok(Some(GridImage { width, height, rgb }))
}

fn filled_canvas(layout: &GridLayout) -> Vec<u8> {
    let pixels = layout.coded_width as usize * layout.coded_height as usize;
    let mut canvas = Vec::with_capacity(pixels * 3);
    for _ in 0..pixels {
        canvas.extend_from_slice(&layout.background);
    }
    canvas
}

/// One decoded tile as tightly packed RGB24.
fn frame_to_rgb(frame: &ffmpeg_next::frame::Video) -> Option<Vec<u8>> {
    let (w, h) = (frame.width(), frame.height());
    if w == 0 || h == 0 {
        return None;
    }
    let mut scaler = SwsContext::get(frame.format(), w, h, Pixel::RGB24, w, h, SwsFlags::BILINEAR).ok()?;
    let mut rgb_frame = ffmpeg_next::frame::Video::empty();
    scaler.run(frame, &mut rgb_frame).ok()?;
    let stride = rgb_frame.stride(0);
    let row = w as usize * 3;
    let data = rgb_frame.data(0);
    let mut out = Vec::with_capacity(row * h as usize);
    for y in 0..h as usize {
        out.extend_from_slice(data.get(y * stride..y * stride + row)?);
    }
    Some(out)
}

/// A decoded tile: tightly packed RGB24.
struct RgbTile<'a> {
    data: &'a [u8],
    width: u32,
    height: u32,
}

/// Copies a tile into the canvas at (x, y); whatever lies outside the canvas is cut off.
fn blit(canvas: &mut [u8], canvas_w: u32, canvas_h: u32, tile: &RgbTile<'_>, x: i32, y: i32) {
    let (tile_w, tile_h) = (tile.width, tile.height);
    for ty in 0..tile_h as i64 {
        let cy = i64::from(y) + ty;
        if cy < 0 || cy >= i64::from(canvas_h) {
            continue;
        }
        let (start_x, skip) = if x < 0 { (0, i64::from(-x)) } else { (i64::from(x), 0) };
        let copy = (i64::from(tile_w) - skip).min(i64::from(canvas_w) - start_x);
        if copy <= 0 {
            continue;
        }
        let src = (ty as usize * tile_w as usize + skip as usize) * 3;
        let dst = (cy as usize * canvas_w as usize + start_x as usize) * 3;
        canvas[dst..dst + copy as usize * 3].copy_from_slice(&tile.data[src..src + copy as usize * 3]);
    }
}

fn crop(canvas: &[u8], canvas_w: u32, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(w as usize * h as usize * 3);
    for row in y..y + h {
        let start = (row as usize * canvas_w as usize + x as usize) * 3;
        out.extend_from_slice(&canvas[start..start + w as usize * 3]);
    }
    out
}

/// Turns the picture clockwise by 0, 90, 180 or 270 degrees.
fn rotate(rgb: Vec<u8>, width: u32, height: u32, clockwise_degrees: i32) -> (u32, u32, Vec<u8>) {
    let Some(img) = image::RgbImage::from_raw(width, height, rgb) else {
        return (width, height, Vec::new());
    };
    let turned = match clockwise_degrees.rem_euclid(360) {
        90 => image::imageops::rotate90(&img),
        180 => image::imageops::rotate180(&img),
        270 => image::imageops::rotate270(&img),
        _ => img,
    };
    (turned.width(), turned.height(), turned.into_raw())
}
