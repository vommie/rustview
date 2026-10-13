//! Slot-stable reconcile of the shown tiles with a changed selection (pure).
//!
//! Tiles that are still selected keep their slot (stable positions, running playback, dragged gutters); newcomers fill
//! the freed slots from the left and are then appended; everything beyond the tile cap is ignored. An empty wish never
//! changes anything (the Multiview stays as it is).

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReconcilePlan {
    pub remove: Vec<u64>,
    pub add: Vec<u64>,
    /// The resulting tiles in slot order.
    pub slots: Vec<u64>,
}

impl ReconcilePlan {
    pub fn is_noop(&self) -> bool {
        self.remove.is_empty() && self.add.is_empty()
    }
}

pub fn reconcile(current: &[u64], wanted: &[u64], cap: usize) -> ReconcilePlan {
    if wanted.is_empty() {
        return ReconcilePlan {
            remove: Vec::new(),
            add: Vec::new(),
            slots: current.to_vec(),
        };
    }

    let remove: Vec<u64> = current
        .iter()
        .copied()
        .filter(|id| !wanted.contains(id))
        .collect();
    let kept = current.len() - remove.len();
    let room = cap.saturating_sub(kept);
    let add: Vec<u64> = wanted
        .iter()
        .copied()
        .filter(|id| !current.contains(id))
        .take(room)
        .collect();

    let mut newcomers = add.iter().copied();
    let mut slots: Vec<Option<u64>> = current
        .iter()
        .map(|id| {
            if remove.contains(id) {
                newcomers.next()
            } else {
                Some(*id)
            }
        })
        .collect();
    slots.extend(newcomers.map(Some));
    ReconcilePlan {
        remove,
        add,
        slots: slots.into_iter().flatten().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_selection_is_a_noop() {
        let p = reconcile(&[1, 2, 3], &[1, 2, 3], 4);
        assert!(p.is_noop());
        assert_eq!(p.slots, vec![1, 2, 3]);
    }

    #[test]
    fn empty_selection_never_closes_or_changes_anything() {
        let p = reconcile(&[1, 2], &[], 4);
        assert!(p.is_noop());
        assert_eq!(p.slots, vec![1, 2]);
    }

    #[test]
    fn newcomers_take_the_freed_slots_from_the_left() {
        let p = reconcile(&[1, 2, 3], &[2, 3, 9], 4);
        assert_eq!(
            (p.remove, p.add, p.slots),
            (vec![1], vec![9], vec![9, 2, 3])
        );
    }

    #[test]
    fn more_removals_than_newcomers_compacts_and_keeps_the_order() {
        let p = reconcile(&[1, 2, 3, 4], &[3, 8], 4);
        assert_eq!(
            (p.remove, p.add, p.slots),
            (vec![1, 2, 4], vec![8], vec![8, 3])
        );
    }

    #[test]
    fn the_cap_is_respected_and_existing_tiles_win_over_newcomers() {
        let p = reconcile(&[1, 2, 3], &[1, 2, 3, 4, 5, 6], 4);
        assert_eq!(p.add, vec![4]);
        assert_eq!(p.slots, vec![1, 2, 3, 4]);
        let p = reconcile(&[1, 2, 3, 4], &[5, 6, 1], 4);
        assert_eq!(
            (p.remove.len(), p.add, p.slots),
            (3, vec![5, 6], vec![1, 5, 6]),
            "the kept tile keeps its slot"
        );
    }

    #[test]
    fn shrinking_to_one_keeps_a_single_tile() {
        let p = reconcile(&[1, 2, 3], &[2], 4);
        assert_eq!(p.slots, vec![2]);
        assert_eq!(p.remove, vec![1, 3]);
    }
}
