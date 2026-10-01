pub mod error;
pub mod loader;
pub mod manager;
pub mod mpv_engine;
pub mod tab_item;
pub mod types;
pub mod x11_embed;

pub use error::ViewerError;
pub use loader::HighResImageLoader;
pub use manager::{ViewerTabManager, MAX_CACHED_HIGH_RES_BUFFERS};
pub use mpv_engine::{MpvEventCallback, MpvFullscreenCallback, MpvPlayer};
pub use tab_item::{ImageBufferEvictor, TabCollection, TabSessionFactory, TabTitleFormatter};
pub use types::{
    ImageTabPayload, MediaTabSession, MpvPlaybackState, TabContentType, TabId, TabPayload,
    VideoTabPayload, ViewerAction,
};
pub use x11_embed::{
    find_window_by_pid_and_title, find_window_by_title, force_x11_fullscreen,
    force_x11_maximized, get_slint_window_xid, get_slint_window_xid_with_title_fallback,
    is_x11_window_maximized, SubwindowGeometry, ViewportBounds, X11VideoSubwindow, XRectangle,
};
