//! Previous/next medium for a Multiview that shows a single image (pure).
//!
//! The order is the current Browse view; the walk is cyclic and skips media that cannot be shown (deleted, pending,
//! corrupted - decided from in-memory flags by the caller). A current medium that is no longer part of the view
//! (filter, folder or delete) anchors to the first (next) or last (previous) showable medium, as the former Quick-Fullscreen did.

/// Index of the neighbor of `current` among `len` entries, or `None` if there is nothing else to show.
pub fn cyclic_neighbor(
    len: usize,
    current: Option<usize>,
    forward: bool,
    is_showable: impl Fn(usize) -> bool,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let Some(pos) = current.filter(|&p| p < len) else {
        return if forward {
            (0..len).find(|&i| is_showable(i))
        } else {
            (0..len).rev().find(|&i| is_showable(i))
        };
    };
    (1..len)
        .map(|step| {
            if forward {
                (pos + step) % len
            } else {
                (pos + len - step) % len
            }
        })
        .find(|&i| is_showable(i))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: usize) -> bool {
        true
    }

    #[test]
    fn walks_forward_and_backward_and_wraps_around() {
        assert_eq!(cyclic_neighbor(5, Some(1), true, all), Some(2));
        assert_eq!(cyclic_neighbor(5, Some(1), false, all), Some(0));
        assert_eq!(cyclic_neighbor(5, Some(4), true, all), Some(0));
        assert_eq!(cyclic_neighbor(5, Some(0), false, all), Some(4));
    }

    #[test]
    fn skips_media_that_cannot_be_shown() {
        let showable = |i: usize| i != 2 && i != 3;
        assert_eq!(cyclic_neighbor(6, Some(1), true, showable), Some(4));
        assert_eq!(cyclic_neighbor(6, Some(4), false, showable), Some(1));
        assert_eq!(cyclic_neighbor(6, Some(5), true, showable), Some(0));
    }

    #[test]
    fn a_lone_showable_medium_has_no_neighbor() {
        assert_eq!(cyclic_neighbor(1, Some(0), true, all), None);
        assert_eq!(cyclic_neighbor(4, Some(2), true, |i| i == 2), None);
        assert_eq!(cyclic_neighbor(4, Some(2), false, |_| false), None);
    }

    #[test]
    fn an_orphaned_medium_anchors_to_the_boundaries() {
        assert_eq!(cyclic_neighbor(5, None, true, all), Some(0));
        assert_eq!(cyclic_neighbor(5, None, false, all), Some(4));
        assert_eq!(cyclic_neighbor(5, Some(99), true, all), Some(0));
        assert_eq!(cyclic_neighbor(5, None, true, |i| i >= 2), Some(2));
        assert_eq!(cyclic_neighbor(5, None, false, |i| i <= 2), Some(2));
    }

    #[test]
    fn an_empty_view_never_panics() {
        assert_eq!(cyclic_neighbor(0, None, true, all), None);
        assert_eq!(cyclic_neighbor(0, Some(0), false, all), None);
    }
}
