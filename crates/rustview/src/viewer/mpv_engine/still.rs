//! A still of a video for the minimap of a zoomed video: mpv's `screenshot-raw` command, which hands the current frame over in
//! memory (no file). libmpv2 has no command with a result, so this one call goes through libmpv's raw client API.

use super::MpvPlayer;
use libmpv2_sys::{
    mpv_format_MPV_FORMAT_BYTE_ARRAY, mpv_format_MPV_FORMAT_INT64, mpv_format_MPV_FORMAT_NODE_MAP, mpv_node,
};
use slint::{Rgb8Pixel, SharedPixelBuffer};
use std::ffi::{CStr, CString, c_char};

/// What `screenshot-raw` returned: `w` x `h` pixels of `B8G8R8X8`, `stride` bytes per row.
struct RawFrame<'a> {
    w: i64,
    h: i64,
    stride: i64,
    data: &'a [u8],
}

impl MpvPlayer {
    /// The current frame of the video without subtitles and OSD, as RGB pixels. It blocks until mpv delivered it, so it belongs
    /// on a worker thread.
    pub fn still(&self) -> Result<SharedPixelBuffer<Rgb8Pixel>, String> {
        let args = ["screenshot-raw", "video", "bgr0"]
            .iter()
            .map(|arg| CString::new(*arg))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let mut pointers: Vec<*const c_char> = args.iter().map(|arg| arg.as_ptr()).collect();
        pointers.push(std::ptr::null());
        let mut node = std::mem::MaybeUninit::<mpv_node>::zeroed();
        // SAFETY: `ctx` is the live handle of this player (it lives as long as `self`); `pointers` is a NULL-terminated array of
        // C strings that outlive the call; mpv writes `node` only.
        let code =
            unsafe { libmpv2_sys::mpv_command_ret(self.mpv.ctx.as_ptr(), pointers.as_mut_ptr(), node.as_mut_ptr()) };
        if code < 0 {
            return Err(format!("screenshot-raw failed ({code})"));
        }
        // SAFETY: the call succeeded, so mpv filled the node.
        let mut node = unsafe { node.assume_init() };
        let pixels = raw_frame(&node).and_then(|frame| to_rgb(&frame));
        // SAFETY: the node was filled by mpv_command_ret; it is freed once, after the pixels were copied out of it.
        unsafe { libmpv2_sys::mpv_free_node_contents(&raw mut node) };
        pixels
    }
}

/// The fields of the node map `screenshot-raw` returns (borrowed from the node).
fn raw_frame(node: &mpv_node) -> Result<RawFrame<'_>, String> {
    if node.format != mpv_format_MPV_FORMAT_NODE_MAP {
        return Err("screenshot-raw returned no map".to_string());
    }
    // SAFETY: the node is a NODE_MAP, so `u.list` points to mpv's node list, which lives as long as the node.
    let list = unsafe { &*node.u.list };
    let mut frame = RawFrame {
        w: 0,
        h: 0,
        stride: 0,
        data: &[],
    };
    for i in 0..usize::try_from(list.num).unwrap_or(0) {
        // SAFETY: i < num, so keys[i] (a C string) and values[i] are valid entries of the list (mpv's node list contract).
        let key = unsafe { CStr::from_ptr(*list.keys.add(i)) };
        // SAFETY: as above.
        let value = unsafe { &*list.values.add(i) };
        match (key.to_bytes(), value.format) {
            // SAFETY: the format says the union holds an int64.
            (b"w", f) if f == mpv_format_MPV_FORMAT_INT64 => frame.w = unsafe { value.u.int64 },
            // SAFETY: as above.
            (b"h", f) if f == mpv_format_MPV_FORMAT_INT64 => frame.h = unsafe { value.u.int64 },
            // SAFETY: as above.
            (b"stride", f) if f == mpv_format_MPV_FORMAT_INT64 => frame.stride = unsafe { value.u.int64 },
            (b"data", f) if f == mpv_format_MPV_FORMAT_BYTE_ARRAY => {
                // SAFETY: the format says the union holds a byte array; its `size` bytes at `data` live as long as the node.
                frame.data = unsafe {
                    let array = &*value.u.ba;
                    std::slice::from_raw_parts(array.data.cast::<u8>(), array.size)
                };
            }
            _ => {}
        }
    }
    Ok(frame)
}

/// `B8G8R8X8` rows to RGB pixels.
fn to_rgb(frame: &RawFrame<'_>) -> Result<SharedPixelBuffer<Rgb8Pixel>, String> {
    let (w, h, stride) = (
        usize::try_from(frame.w).map_err(|e| e.to_string())?,
        usize::try_from(frame.h).map_err(|e| e.to_string())?,
        usize::try_from(frame.stride).map_err(|e| e.to_string())?,
    );
    if w == 0 || h == 0 || stride < w * 4 || frame.data.len() < stride * (h - 1) + w * 4 {
        return Err(format!(
            "screenshot-raw returned an unexpected frame ({w} x {h}, stride {stride})"
        ));
    }
    let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(
        u32::try_from(w).map_err(|e| e.to_string())?,
        u32::try_from(h).map_err(|e| e.to_string())?,
    );
    for (row, out) in pixels.make_mut_slice().chunks_exact_mut(w).enumerate() {
        let line = &frame.data[row * stride..row * stride + w * 4];
        for (pixel, bgrx) in out.iter_mut().zip(line.as_chunks::<4>().0) {
            *pixel = Rgb8Pixel {
                r: bgrx[2],
                g: bgrx[1],
                b: bgrx[0],
            };
        }
    }
    Ok(pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgrx_rows_become_rgb_pixels_and_the_stride_padding_is_skipped() {
        // 2 x 2 pixels, rows of 12 bytes (4 bytes of padding).
        let data = [
            1, 2, 3, 0, 4, 5, 6, 0, 9, 9, 9, 9, 7, 8, 9, 0, 10, 11, 12, 0, 9, 9, 9, 9,
        ];
        let pixels = to_rgb(&RawFrame {
            w: 2,
            h: 2,
            stride: 12,
            data: &data,
        })
        .unwrap();
        let rgb: Vec<(u8, u8, u8)> = pixels.as_slice().iter().map(|p| (p.r, p.g, p.b)).collect();
        assert_eq!(rgb, [(3, 2, 1), (6, 5, 4), (9, 8, 7), (12, 11, 10)]);
        assert!(
            to_rgb(&RawFrame {
                w: 2,
                h: 2,
                stride: 4,
                data: &data
            })
            .is_err(),
            "a stride below the width"
        );
    }
}
