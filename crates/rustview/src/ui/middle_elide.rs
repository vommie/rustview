//! Rust side of `ui/middle_elide_text.slint`: names that are too wide are shortened in the middle. Slint measures the text and
//! decides how many characters fit; this only cuts the string, which Slint cannot do.

use crate::ui::TextCut;
use rustview_core::util::cast::SaturatingAs;
use slint::{ComponentHandle, Global};
use unicode_segmentation::UnicodeSegmentation;

/// Connects `TextCut` of `component` (once for every window that shows a `MiddleElideText`).
pub fn install<C: ComponentHandle>(component: &C)
where
    for<'a> TextCut<'a>: Global<'a, C>,
{
    component
        .global::<TextCut>()
        .on_middle(|text, keep| middle(&text, keep.saturating_as::<usize>()).into());
}

/// `text` shortened to `keep` grapheme clusters, the larger half from its start, joined by an ellipsis (`text` itself when it
/// is not longer).
pub fn middle(text: &str, keep: usize) -> String {
    let graphemes: Vec<&str> = text.graphemes(true).collect();
    if graphemes.len() <= keep {
        return text.to_owned();
    }
    let tail = keep / 2;
    let head = keep - tail;
    let mut cut = graphemes[..head].concat().trim_end().to_owned();
    cut.push('\u{2026}');
    cut.push_str(graphemes[graphemes.len() - tail..].concat().trim_start());
    cut
}

#[cfg(test)]
mod tests {
    use super::middle;

    #[test]
    fn a_name_is_cut_in_the_middle() {
        assert_eq!(middle("Range number one", 20), "Range number one");
        assert_eq!(middle("Range number one", 16), "Range number one");
        assert_eq!(
            middle("Range number one", 8),
            "Rang\u{2026}one",
            "no space next to the ellipsis"
        );
        assert_eq!(middle("abcdefg", 3), "ab\u{2026}g");
        assert_eq!(middle("abcdefg", 0), "\u{2026}");
    }

    #[test]
    fn characters_are_never_split() {
        assert_eq!(middle("Z\u{fc}rich S\u{e9}e Ufer", 6), "Z\u{fc}r\u{2026}fer");
        let flags = "\u{1f1e9}\u{1f1ea}\u{1f1eb}\u{1f1f7}\u{1f1ee}\u{1f1f9}";
        assert_eq!(middle(flags, 2), "\u{1f1e9}\u{1f1ea}\u{2026}\u{1f1ee}\u{1f1f9}");
    }
}
