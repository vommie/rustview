//! The edits of one picture that are applied but not saved yet (pure): a list of operations with undo and redo, and the one
//! plan that saving carries out on the original. A new kind of edit (rotating, flipping) is a new `EditOp` variant plus its
//! step in `EditStack::plan`; the stack, the dirty state, saving and discarding stay the same.

use super::geometry::PixelRect;

/// One edit, relative to the picture as the edits before it left it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditOp {
    /// Keep only this rectangle.
    Crop(PixelRect),
}

/// What saving does to the original: everything the edits did, composed into the fewest steps (several crops are one crop).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditPlan {
    /// The rectangle of the original that is kept (`None` = all of it).
    pub crop: Option<PixelRect>,
    /// The size of the picture that is written.
    pub output_size: (u32, u32),
}

impl EditPlan {
    /// The plan changes nothing (the original would be written as it is).
    pub fn is_identity(&self) -> bool {
        self.crop.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditStack {
    /// Size of the original picture.
    original: (u32, u32),
    done: Vec<EditOp>,
    undone: Vec<EditOp>,
}

impl EditStack {
    pub fn new(original: (u32, u32)) -> Self {
        Self {
            original,
            done: Vec::new(),
            undone: Vec::new(),
        }
    }

    pub fn original_size(&self) -> (u32, u32) {
        self.original
    }

    /// There are edits that are not saved.
    pub fn is_dirty(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_undo(&self) -> bool {
        !self.done.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.undone.is_empty()
    }

    pub fn ops(&self) -> &[EditOp] {
        &self.done
    }

    /// Size of the picture after the edits.
    pub fn current_size(&self) -> (u32, u32) {
        self.plan().output_size
    }

    /// Applies an edit. An edit that changes nothing (a crop of the whole picture) or does not fit the picture is refused.
    /// A new edit ends what could be redone.
    pub fn push(&mut self, op: EditOp) -> bool {
        let size = self.current_size();
        let valid = match op {
            EditOp::Crop(rect) => rect.fits_in(size) && !rect.is_full(size),
        };
        if valid {
            self.done.push(op);
            self.undone.clear();
        }
        valid
    }

    pub fn undo(&mut self) -> Option<EditOp> {
        let op = self.done.pop()?;
        self.undone.push(op);
        Some(op)
    }

    pub fn redo(&mut self) -> Option<EditOp> {
        let op = self.undone.pop()?;
        self.done.push(op);
        Some(op)
    }

    /// Forgets every edit (they were saved or discarded).
    pub fn clear(&mut self) {
        self.done.clear();
        self.undone.clear();
    }

    /// The composed plan of every applied edit, in the coordinates of the original.
    pub fn plan(&self) -> EditPlan {
        let mut kept = PixelRect::full(self.original);
        for op in &self.done {
            match *op {
                EditOp::Crop(rect) => kept = kept.compose(rect),
            }
        }
        EditPlan {
            crop: (!kept.is_full(self.original)).then_some(kept),
            output_size: kept.size(),
        }
    }

    /// A short description for dialogs ("Crop to 893 x 1116 px").
    pub fn describe(&self) -> String {
        let plan = self.plan();
        match plan.crop {
            Some(rect) => format!("Crop to {} x {} px", rect.width, rect.height),
            None => "No changes".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_stack_is_clean_and_plans_nothing() {
        let stack = EditStack::new((800, 600));
        assert!(!stack.is_dirty());
        assert!(stack.plan().is_identity());
        assert_eq!(stack.current_size(), (800, 600));
        assert_eq!(stack.describe(), "No changes");
    }

    #[test]
    fn crops_compose_into_one_crop_of_the_original() {
        let mut stack = EditStack::new((800, 600));
        assert!(stack.push(EditOp::Crop(PixelRect::new(100, 50, 400, 300))));
        assert!(stack.push(EditOp::Crop(PixelRect::new(10, 20, 200, 100))));
        let plan = stack.plan();
        assert_eq!(plan.crop, Some(PixelRect::new(110, 70, 200, 100)));
        assert_eq!(plan.output_size, (200, 100));
        assert_eq!(stack.describe(), "Crop to 200 x 100 px");
        assert!(stack.is_dirty());
    }

    #[test]
    fn edits_that_change_nothing_or_do_not_fit_are_refused() {
        let mut stack = EditStack::new((800, 600));
        assert!(!stack.push(EditOp::Crop(PixelRect::full((800, 600)))));
        assert!(!stack.push(EditOp::Crop(PixelRect::new(700, 0, 200, 100))));
        assert!(!stack.push(EditOp::Crop(PixelRect::new(0, 0, 0, 100))));
        assert!(!stack.is_dirty());
        assert!(stack.push(EditOp::Crop(PixelRect::new(0, 0, 400, 300))));
        // The second crop is relative to the first one's result.
        assert!(!stack.push(EditOp::Crop(PixelRect::new(0, 0, 500, 300))));
    }

    #[test]
    fn undo_and_redo_walk_the_list_and_a_new_edit_ends_redo() {
        let mut stack = EditStack::new((800, 600));
        let first = EditOp::Crop(PixelRect::new(0, 0, 400, 300));
        let second = EditOp::Crop(PixelRect::new(10, 10, 100, 100));
        stack.push(first);
        stack.push(second);
        assert_eq!(stack.undo(), Some(second));
        assert_eq!(stack.current_size(), (400, 300));
        assert!(stack.can_redo());
        assert_eq!(stack.undo(), Some(first));
        assert!(!stack.is_dirty());
        assert_eq!(stack.undo(), None);
        assert_eq!(stack.redo(), Some(first));
        assert!(stack.push(EditOp::Crop(PixelRect::new(5, 5, 50, 50))));
        assert!(!stack.can_redo());
        assert_eq!(stack.plan().crop, Some(PixelRect::new(5, 5, 50, 50)));
        stack.clear();
        assert!(!stack.is_dirty() && !stack.can_redo());
    }
}
