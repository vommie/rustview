//! How a picture has to be turned for display. Phones store a rotation next to the pixels: videos in the display matrix of
//! the stream (older files in a `rotate` tag), HEIF and AVIF images in the `irot` property, which FFmpeg also exposes as a
//! display matrix. Mirroring is not handled (it is rare and never produced by a camera that was not mirrored on purpose).

use ffmpeg_next::codec::packet::side_data::Type as SideDataType;
use ffmpeg_next::format::stream::Stream;
use ffmpeg_next::DictionaryRef;

/// Clockwise degrees (0, 90, 180 or 270) of a display matrix, as it is stored in the side data (nine native-endian 32-bit
/// integers). `None` for matrices that do not describe a rotation by a multiple of 90 degrees.
pub fn clockwise_degrees_of_matrix(data: &[u8]) -> Option<i32> {
    if data.len() < 36 {
        return None;
    }
    let mut matrix = [0i32; 9];
    for (slot, bytes) in matrix.iter_mut().zip(data.as_chunks::<4>().0) {
        *slot = i32::from_ne_bytes(*bytes);
    }
    // SAFETY: `matrix` holds the nine integers the function reads.
    let counter_clockwise = unsafe { ffmpeg_next::ffi::av_display_rotation_get(matrix.as_ptr()) };
    clockwise_quarter_turn(counter_clockwise)
}

/// FFmpeg reports the angle counter-clockwise; the display needs it clockwise.
fn clockwise_quarter_turn(counter_clockwise_degrees: f64) -> Option<i32> {
    if !counter_clockwise_degrees.is_finite() {
        return None;
    }
    let clockwise = (-counter_clockwise_degrees).rem_euclid(360.0);
    let nearest = (clockwise / 90.0).round() * 90.0;
    ((clockwise - nearest).abs() < 1.0).then(|| (nearest as i32).rem_euclid(360))
}

/// The rotation a stream asks for: its display matrix, else the `rotate` tag of the stream or the container.
pub fn stream_rotation(stream: &Stream<'_>, container: &DictionaryRef<'_>) -> i32 {
    for side_data in stream.side_data() {
        if side_data.kind() == SideDataType::DisplayMatrix
            && let Some(degrees) = clockwise_degrees_of_matrix(side_data.data())
        {
            return degrees;
        }
    }
    let tag = |value: Option<&str>| value.and_then(|v| v.parse::<i32>().ok());
    tag(stream.metadata().get("rotate"))
        .or_else(|| tag(container.get("rotate")))
        .unwrap_or(0)
        .rem_euclid(360)
}

/// Whether a rotation swaps width and height.
#[inline]
pub fn swaps_axes(clockwise_degrees: i32) -> bool {
    matches!(clockwise_degrees.rem_euclid(360), 90 | 270)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_clockwise_angles_become_clockwise_quarter_turns() {
        assert_eq!(clockwise_quarter_turn(0.0), Some(0));
        assert_eq!(clockwise_quarter_turn(90.0), Some(270));
        assert_eq!(clockwise_quarter_turn(-90.0), Some(90));
        assert_eq!(clockwise_quarter_turn(180.0), Some(180));
        assert_eq!(clockwise_quarter_turn(-180.0), Some(180));
        assert_eq!(clockwise_quarter_turn(270.0), Some(90));
        assert_eq!(clockwise_quarter_turn(89.6), Some(270));
    }

    #[test]
    fn angles_that_are_no_quarter_turn_and_broken_matrices_mean_no_rotation() {
        assert_eq!(clockwise_quarter_turn(45.0), None);
        assert_eq!(clockwise_quarter_turn(f64::NAN), None);
        assert_eq!(clockwise_degrees_of_matrix(&[0u8; 12]), None);
    }

    #[test]
    fn a_real_display_matrix_is_read() {
        // rotation=90 as `avifenc --irot 1` writes it (16.16 fixed point, row-major)
        let matrix: [i32; 9] = [0, -65536, 0, 65536, 0, 0, 0, 0, 1 << 30];
        let bytes: Vec<u8> = matrix.iter().flat_map(|v| v.to_ne_bytes()).collect();
        assert_eq!(clockwise_degrees_of_matrix(&bytes), Some(270));
    }

    #[test]
    fn only_quarter_turns_swap_the_axes() {
        assert!(swaps_axes(90) && swaps_axes(270) && swaps_axes(-90));
        assert!(!swaps_axes(0) && !swaps_axes(180));
    }
}
