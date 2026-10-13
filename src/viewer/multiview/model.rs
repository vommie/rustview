//! Small shared types of the Multiview (pure).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileKind {
    Image,
    Video,
}

/// Where the aspect ratio of a tile comes from: the library record (can be wrong for anamorphic or display-matrix
/// rotated videos) or what mpv reports after the file is loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectSource {
    Library,
    Probed,
}

/// Aspect ratio (width / height) from pixel dimensions, falling back when a dimension is unknown (zero).
pub fn aspect_from_size(width: u32, height: u32, fallback: f32) -> f32 {
    if width > 0 && height > 0 {
        width as f32 / height as f32
    } else {
        fallback
    }
}

/// Display aspect ratio from mpv's `video-params`: `dw`/`dh` are not rotated, `rotate` (0/90/180/270) is reported separately.
pub fn aspect_from_video_params(display_w: i64, display_h: i64, rotate: i64) -> Option<f32> {
    if display_w <= 0 || display_h <= 0 {
        return None;
    }
    let (w, h) = if rotate.rem_euclid(180) == 90 {
        (display_h, display_w)
    } else {
        (display_w, display_h)
    };
    Some(w as f32 / h as f32)
}

/// True if two aspect ratios differ by more than one percent (the threshold for re-laying out after probing).
pub fn aspect_differs(a: f32, b: f32) -> bool {
    (a - b).abs() / b.max(0.0001) > 0.01
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_dimensions_use_the_fallback() {
        assert_eq!(aspect_from_size(0, 100, 1.5), 1.5);
        assert_eq!(aspect_from_size(100, 0, 1.5), 1.5);
        assert_eq!(aspect_from_size(200, 100, 1.5), 2.0);
    }

    #[test]
    fn rotation_swaps_the_display_size() {
        assert_eq!(aspect_from_video_params(640, 360, 0), Some(640.0 / 360.0));
        assert_eq!(aspect_from_video_params(640, 360, 90), Some(360.0 / 640.0));
        assert_eq!(aspect_from_video_params(640, 360, 270), Some(360.0 / 640.0));
        assert_eq!(aspect_from_video_params(640, 360, 180), Some(640.0 / 360.0));
        assert_eq!(aspect_from_video_params(640, 360, -90), Some(360.0 / 640.0));
        assert_eq!(aspect_from_video_params(0, 360, 0), None);
    }

    #[test]
    fn small_aspect_differences_are_ignored() {
        assert!(!aspect_differs(1.7778, 1.7777));
        assert!(aspect_differs(0.8889, 1.7778));
    }
}
