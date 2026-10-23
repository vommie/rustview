//! Back and forward through the folders the user visited (Alt+Left, Alt+Right).

use rustview_core::library::models::FolderId;

/// Longest back stack; the oldest entry falls off first.
const LIMIT: usize = 100;

#[derive(Debug, Clone, Default)]
pub struct NavigationHistory {
    back: Vec<FolderId>,
    forward: Vec<FolderId>,
    /// The next `record` belongs to a jump through the history itself and leaves both stacks alone.
    replaying: bool,
}

impl NavigationHistory {
    /// The user opened `to` while `from` was open (a click, "...", a double click, a favorite): `from` becomes the next "back".
    pub fn record(&mut self, from: Option<FolderId>, to: FolderId) {
        if std::mem::take(&mut self.replaying) {
            return;
        }
        let Some(from) = from.filter(|&from| from != to) else {
            return;
        };
        if self.back.last() != Some(&from) {
            self.back.push(from);
            if self.back.len() > LIMIT {
                self.back.remove(0);
            }
        }
        self.forward.clear();
    }

    /// The folder Alt+Left goes to: the newest one that `is_valid` still; the open folder becomes the next "forward".
    pub fn step_back(&mut self, current: FolderId, is_valid: impl Fn(FolderId) -> bool) -> Option<FolderId> {
        Self::step(&mut self.back, &mut self.forward, current, is_valid).inspect(|_| self.replaying = true)
    }

    /// The folder Alt+Right goes to; the open folder becomes the next "back".
    pub fn step_forward(&mut self, current: FolderId, is_valid: impl Fn(FolderId) -> bool) -> Option<FolderId> {
        Self::step(&mut self.forward, &mut self.back, current, is_valid).inspect(|_| self.replaying = true)
    }

    fn step(
        from: &mut Vec<FolderId>,
        to: &mut Vec<FolderId>,
        current: FolderId,
        is_valid: impl Fn(FolderId) -> bool,
    ) -> Option<FolderId> {
        while let Some(candidate) = from.pop() {
            if candidate != current && is_valid(candidate) {
                if to.last() != Some(&current) {
                    to.push(current);
                }
                return Some(candidate);
            }
        }
        None
    }

    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}
