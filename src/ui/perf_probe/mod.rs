//! Developer performance probe for the Browse view (grid and table), enabled by the environment variable `RUSTVIEW_PERF`.
//!
//! Without the variable nothing is installed and the rendering hook costs one relaxed atomic load per frame.

mod driver;
mod frames;
pub mod plan;

pub use driver::start_if_requested;
pub use frames::note_rendering_state;
