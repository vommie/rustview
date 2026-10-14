//! Which tile an action such as F2 (rename) acts on (pure).

/// The tile under the pointer, else the last hovered tile, else the first tile. `ids` are the media ids in slot order.
pub fn pick_target(
    under_pointer: Option<usize>,
    hovered: Option<usize>,
    last_hovered: Option<u64>,
    ids: &[u64],
) -> Option<u64> {
    under_pointer
        .or(hovered)
        .and_then(|index| ids.get(index).copied())
        .or(last_hovered.filter(|id| ids.contains(id)))
        .or_else(|| ids.first().copied())
}
