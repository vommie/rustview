//! Ordering of the badges inside one badge area (e.g. all `bottom-left` badges).
//!
//! Badges arrive in registration order, which is the automatic order. A badge may request an
//! explicit slot (0 = first). Explicit badges are placed first; the remaining badges fill the free
//! slots in registration order. The result is deterministic for any input.

use super::badge::ThumbnailBadge;

/// Orders badges of a single area. Slots count only *present* badges and are clamped to the
/// available range; a slot that is already taken moves the badge to the next free slot.
pub fn order_badges(badges: Vec<ThumbnailBadge>) -> Vec<ThumbnailBadge> {
    let count = badges.len();
    if count < 2 || badges.iter().all(|b| b.slot.is_none()) {
        return badges;
    }

    let mut slots: Vec<Option<ThumbnailBadge>> = (0..count).map(|_| None).collect();
    let mut automatic = Vec::new();

    for badge in badges {
        match badge.slot {
            Some(requested) => {
                let target = requested.min(count - 1);
                match nearest_free_slot(&slots, target) {
                    Some(free) => {
                        if free != requested {
                            tracing::debug!(
                                badge = badge.id,
                                requested,
                                assigned = free,
                                "[ThumbnailBadges] Requested badge slot unavailable, using nearest free slot"
                            );
                        }
                        slots[free] = Some(badge);
                    }
                    None => automatic.push(badge),
                }
            }
            None => automatic.push(badge),
        }
    }

    let mut automatic = automatic.into_iter();
    for slot in slots.iter_mut().filter(|s| s.is_none()) {
        *slot = automatic.next();
    }
    slots.into_iter().flatten().collect()
}

/// Finds the free slot closest to `target`, preferring later slots on a tie.
fn nearest_free_slot(slots: &[Option<ThumbnailBadge>], target: usize) -> Option<usize> {
    (target..slots.len()).chain((0..target).rev()).find(|&i| slots[i].is_none())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(id: &'static str) -> ThumbnailBadge {
        ThumbnailBadge::text(id, id)
    }

    fn ids(badges: Vec<ThumbnailBadge>) -> Vec<&'static str> {
        badges.into_iter().map(|b| b.id).collect()
    }

    #[test]
    fn keeps_registration_order_without_explicit_slots() {
        assert_eq!(ids(order_badges(vec![b("a"), b("b"), b("c")])), ["a", "b", "c"]);
    }

    #[test]
    fn explicit_slot_moves_badge_and_others_fill_around_it() {
        assert_eq!(ids(order_badges(vec![b("a"), b("b").slot(0), b("c")])), ["b", "a", "c"]);
        assert_eq!(ids(order_badges(vec![b("a").slot(2), b("b"), b("c")])), ["b", "c", "a"]);
    }

    #[test]
    fn slot_beyond_range_is_clamped_to_last_position() {
        assert_eq!(ids(order_badges(vec![b("a").slot(9), b("b"), b("c")])), ["b", "c", "a"]);
    }

    #[test]
    fn colliding_slots_use_next_free_slot_in_registration_order() {
        assert_eq!(ids(order_badges(vec![b("a").slot(0), b("b").slot(0), b("c")])), ["a", "b", "c"]);
    }

    #[test]
    fn all_slots_explicit_and_colliding_at_the_end_still_returns_every_badge() {
        let out = order_badges(vec![b("a").slot(5), b("b").slot(5), b("c").slot(5)]);
        assert_eq!(ids(out), ["c", "b", "a"]);
    }

    #[test]
    fn handles_empty_and_single_badge_input() {
        assert!(order_badges(vec![]).is_empty());
        assert_eq!(ids(order_badges(vec![b("a").slot(3)])), ["a"]);
    }
}
