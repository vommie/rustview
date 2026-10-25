//! What the A-B repeat group of one video's control bar remembers (pure): the open bubble or menu, the pointer over the strip
//! of saved repeats and a dragged A or B flag. The repeat itself lives in the mpv script (`viewer::ab_repeat`).

/// The bubble above Save or the load menu.
#[derive(Debug, Clone, PartialEq)]
pub enum AbPopup {
    /// The name the user types for the repeat to save.
    Save { text: String },
    Load {
        query: String,
        /// 0-based.
        page: usize,
        renaming: Option<Renaming>,
    },
}

/// A saved repeat whose name is being edited in the menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Renaming {
    /// Its position in the saved list.
    pub index: usize,
    pub text: String,
}

/// The two points of a repeat (the flags of the playbar, the buttons, the keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbPoint {
    A,
    B,
}

#[derive(Debug, Default)]
pub struct AbBarState {
    pub popup: Option<AbPopup>,
    /// The pointer over the strip of saved repeats, as a share of the progress bar.
    strip_hover: Option<f32>,
    /// A flag is dragged to this share of the progress bar.
    drag: Option<(AbPoint, f32)>,
}

impl AbBarState {
    pub fn set_strip_hover(&mut self, share: Option<f32>) {
        self.strip_hover = share.map(|s| s.clamp(0.0, 1.0));
    }

    pub fn strip_hover(&self) -> Option<f32> {
        self.strip_hover
    }

    pub fn drag(&self) -> Option<(AbPoint, f32)> {
        self.drag
    }

    /// A flag is pressed or dragged to `share`.
    pub fn drag_to(&mut self, point: AbPoint, share: f32) {
        self.drag = Some((point, share.clamp(0.0, 1.0)));
    }

    /// The flag is let go: where it ended (`None`: no drag ran).
    pub fn drop_flag(&mut self) -> Option<(AbPoint, f32)> {
        self.drag.take()
    }

    pub fn popup_open(&self) -> bool {
        self.popup.is_some()
    }

    /// The group keeps its bar on screen (a popup is open or a flag is dragged).
    pub fn holds_bar(&self) -> bool {
        self.popup.is_some() || self.drag.is_some()
    }

    /// The bar went away: hovers, drags and popups are over.
    pub fn release(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_flag_holds_the_bar_until_it_is_dropped() {
        let mut state = AbBarState::default();
        assert!(!state.holds_bar());
        state.drag_to(AbPoint::A, 1.4);
        assert_eq!(state.drag(), Some((AbPoint::A, 1.0)));
        assert!(state.holds_bar());
        assert_eq!(state.drop_flag(), Some((AbPoint::A, 1.0)));
        assert_eq!(state.drop_flag(), None);
        state.popup = Some(AbPopup::Save { text: String::new() });
        assert!(state.holds_bar() && state.popup_open());
        state.release();
        assert!(!state.holds_bar());
    }
}
