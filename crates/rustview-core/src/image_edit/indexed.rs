//! Cropping pictures with a palette without converting them (pure): a PNG keeps its palette, transparency table and bit depth
//! (1, 2, 4 or 8 bits per pixel), a still GIF keeps its colors. Decoding them to RGB and encoding them again would make a
//! palette PNG a truecolor one and would make a GIF quantize its colors anew.

use super::geometry::PixelRect;
use super::options::PngCompression;
use std::io::Cursor;

/// The file is a PNG with a palette.
pub fn is_indexed_png(data: &[u8]) -> bool {
    // IHDR is the first chunk: signature (8) + length (4) + type (4) + width (4) + height (4) + bit depth (1) + color type (1).
    data.len() > 25 && data.starts_with(b"\x89PNG\r\n\x1a\n") && &data[12..16] == b"IHDR" && data[25] == 3
}

/// The samples of one row of `bits` bits each (most significant bits first, as PNG packs them).
fn unpack_row(row: &[u8], bits: u8, count: usize) -> Vec<u8> {
    if bits == 8 {
        return row[..count.min(row.len())].to_vec();
    }
    let per_byte = usize::from(8 / bits);
    let mask = (1u16 << bits) - 1;
    (0..count)
        .map(|i| {
            let byte = row.get(i / per_byte).copied().unwrap_or(0);
            let shift = 8 - usize::from(bits) * (i % per_byte + 1);
            u8::try_from((u16::from(byte) >> shift) & mask).unwrap_or(0)
        })
        .collect()
}

fn pack_row(samples: &[u8], bits: u8) -> Vec<u8> {
    if bits == 8 {
        return samples.to_vec();
    }
    let per_byte = usize::from(8 / bits);
    let mut row = vec![0u8; samples.len().div_ceil(per_byte)];
    for (i, sample) in samples.iter().enumerate() {
        let shift = 8 - usize::from(bits) * (i % per_byte + 1);
        row[i / per_byte] |= sample << shift;
    }
    row
}

pub fn crop_indexed_png(data: &[u8], crop: PixelRect, compression: PngCompression) -> Result<Vec<u8>, String> {
    let mut decoder = png::Decoder::new(Cursor::new(data));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let info = reader.info().clone();
    if info.color_type != png::ColorType::Indexed {
        return Err("the PNG has no palette".to_string());
    }
    let palette = info.palette.clone().ok_or("the PNG has no palette")?;
    let bits = info.bit_depth as u8;
    let mut buffer = vec![0; reader.output_buffer_size().ok_or("the PNG is too large")?];
    let frame = reader.next_frame(&mut buffer).map_err(|e| e.to_string())?;
    let to_usize = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
    if crop.right() > info.width || crop.bottom() > info.height {
        return Err("the crop lies outside the picture".to_string());
    }
    let mut packed = Vec::new();
    for row in to_usize(crop.y)..to_usize(crop.bottom()) {
        let line = &buffer[row * frame.line_size..(row + 1) * frame.line_size];
        let samples = unpack_row(line, bits, to_usize(info.width));
        packed.extend(pack_row(&samples[to_usize(crop.x)..to_usize(crop.right())], bits));
    }
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, crop.width, crop.height);
        encoder.set_color(png::ColorType::Indexed);
        encoder.set_depth(info.bit_depth);
        encoder.set_palette(palette.to_vec());
        if let Some(trns) = &info.trns {
            encoder.set_trns(trns.to_vec());
        }
        encoder.set_compression(match compression {
            PngCompression::Fast => png::Compression::Fast,
            PngCompression::Balanced => png::Compression::Balanced,
            PngCompression::Best => png::Compression::High,
        });
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&packed).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
    }
    Ok(out)
}

/// Crops the (only) frame of a still GIF on its logical screen and writes it with the same colors.
pub fn crop_gif(data: &[u8], crop: PixelRect) -> Result<Vec<u8>, String> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::Indexed);
    let mut decoder = options.read_info(Cursor::new(data)).map_err(|e| e.to_string())?;
    let (screen_w, screen_h) = (usize::from(decoder.width()), usize::from(decoder.height()));
    let global = decoder.global_palette().map(<[u8]>::to_vec);
    let background = decoder.bg_color();
    let frame = decoder
        .read_next_frame()
        .map_err(|e| e.to_string())?
        .ok_or("the GIF has no picture")?
        .clone();
    let palette = frame.palette.clone().or(global).ok_or("the GIF has no colors")?;
    let fill = frame
        .transparent
        .or_else(|| background.and_then(|b| u8::try_from(b).ok()))
        .unwrap_or(0);
    let mut canvas = vec![fill; screen_w * screen_h];
    let (left, top) = (usize::from(frame.left), usize::from(frame.top));
    let frame_w = usize::from(frame.width);
    for (row, line) in frame.buffer.chunks(frame_w.max(1)).enumerate() {
        let y = top + row;
        if y >= screen_h {
            break;
        }
        for (column, index) in line.iter().enumerate() {
            let x = left + column;
            if x < screen_w {
                canvas[y * screen_w + x] = *index;
            }
        }
    }
    let to_usize = |v: u32| usize::try_from(v).unwrap_or(usize::MAX);
    if to_usize(crop.right()) > screen_w || to_usize(crop.bottom()) > screen_h {
        return Err("the crop lies outside the picture".to_string());
    }
    let mut cropped = Vec::with_capacity(to_usize(crop.width) * to_usize(crop.height));
    for y in to_usize(crop.y)..to_usize(crop.bottom()) {
        cropped.extend_from_slice(&canvas[y * screen_w + to_usize(crop.x)..y * screen_w + to_usize(crop.right())]);
    }
    let (w, h) = (
        u16::try_from(crop.width).map_err(|e| e.to_string())?,
        u16::try_from(crop.height).map_err(|e| e.to_string())?,
    );
    let mut out = Vec::new();
    {
        let mut encoder = gif::Encoder::new(&mut out, w, h, &palette).map_err(|e| e.to_string())?;
        let new_frame = gif::Frame {
            width: w,
            height: h,
            buffer: std::borrow::Cow::Owned(cropped),
            transparent: frame.transparent,
            ..gif::Frame::default()
        };
        encoder.write_frame(&new_frame).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_rows_survive_unpacking_and_packing_at_every_bit_depth() {
        for bits in [1u8, 2, 4, 8] {
            let max = (1u16 << bits) - 1;
            let samples: Vec<u8> = (0..13u16).map(|i| u8::try_from(i % (max + 1)).unwrap_or(0)).collect();
            let packed = pack_row(&samples, bits);
            assert_eq!(unpack_row(&packed, bits, samples.len()), samples, "{bits} bits");
        }
        // 1 bit, MSB first: 1,0,1,1,0,0,0,1 -> 0b1011_0001
        assert_eq!(pack_row(&[1, 0, 1, 1, 0, 0, 0, 1], 1), vec![0b1011_0001]);
    }
}
