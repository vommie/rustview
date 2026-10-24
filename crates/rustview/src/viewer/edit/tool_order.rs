//! The order in which the crop tools of one Multiview were opened (pure). Esc cancels the newest open tool first (the one the
//! user just opened is the one they mean); once none is open, Esc does what it always did (close the fullscreen, end a selection, ...).

use crate::viewer::multiview::follow::TileKey;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolOrder(Vec<TileKey>);

impl ToolOrder {
    pub fn opened(&mut self, tile: TileKey) {
        if !self.0.contains(&tile) {
            self.0.push(tile);
        }
    }

    pub fn closed(&mut self, tile: TileKey) {
        self.0.retain(|t| *t != tile);
    }

    /// The tool Esc cancels next: the one opened last.
    pub fn newest(&self) -> Option<TileKey> {
        self.0.last().copied()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, tile: TileKey) -> bool {
        self.0.contains(&tile)
    }

    /// Drops tiles that are gone or have no tool any more.
    pub fn retain(&mut self, keep: impl Fn(TileKey) -> bool) {
        self.0.retain(|t| keep(*t));
    }

    pub fn keys(&self) -> &[TileKey] {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_closes_the_newest_tool_first() {
        let mut order = ToolOrder::default();
        order.opened(7);
        order.opened(3);
        order.opened(7);
        order.opened(5);
        let mut closed = Vec::new();
        while let Some(tile) = order.newest() {
            closed.push(tile);
            order.closed(tile);
        }
        assert_eq!(closed, [5, 3, 7]);
        assert!(order.is_empty());
    }

    #[test]
    fn tiles_that_left_are_forgotten() {
        let mut order = ToolOrder::default();
        order.opened(1);
        order.opened(2);
        order.opened(3);
        order.retain(|t| t != 3);
        assert_eq!(order.newest(), Some(2));
        assert!(!order.contains(3));
    }
}
