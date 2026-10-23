//! Tag actions on the selected media: assigning, removing, rating and the tag queries.

use super::TagSelectionStats;
use super::TagTreeSync;
use super::{BridgeTags, TagActionContext};
use std::time::Instant;

mod query;
mod rating;
mod toggle;

pub struct TagActionHandler;

/// Timings of a tag or rating query for the benchmark log.
pub(super) struct QueryTimings {
    pub(super) t_flow_start: Instant,
    pub(super) db_search_us: u128,
    pub(super) preindex_us: u128,
    pub(super) t_reload: Instant,
}

impl TagActionHandler {}
