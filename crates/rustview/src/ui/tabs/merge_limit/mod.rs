//! The warning that the selected tabs cannot be merged because they hold more media than one Multiview shows.
//!
//! `texts` words it (pure, unit tested), `dialog` shows it. It only informs: the user removes the media from the tabs and merges
//! again; nothing is merged partly and no tab is changed.

mod dialog;
pub mod texts;

pub use dialog::MergeLimitDialog;
