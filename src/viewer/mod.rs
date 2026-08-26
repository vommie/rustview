pub mod error;
pub mod loader;
pub mod manager;
pub mod types;

pub use error::ViewerError;
pub use loader::HighResImageLoader;
pub use manager::{ViewerTabManager, MAX_CACHED_HIGH_RES_BUFFERS};
pub use types::{
    ImageTabPayload, MediaTabSession, TabContentType, TabId, TabPayload, VideoTabPayload,
    ViewerAction,
};
