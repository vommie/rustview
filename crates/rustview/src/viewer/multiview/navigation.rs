//! Previous/next medium for a Multiview that shows a single image (pure).
//!
//! The order is the current Browse view; the walk is cyclic and skips media that cannot be shown (deleted, pending,
//! corrupted - decided from in-memory flags by the caller). A current medium that is no longer part of the view
//! (filter, folder or delete) anchors to the first (next) or last (previous) showable medium.

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
