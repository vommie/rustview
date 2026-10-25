pub mod ab_repeat;
pub mod edit;
pub mod error;
pub mod loader;
pub mod manager;
pub mod mpv_engine;
pub mod multiview;
pub mod tab_item;
pub mod tab_merge;
mod tab_native;
pub mod tab_ui_gate;
pub mod types;
pub mod x11;
pub mod zoom;

pub use error::ViewerError;
pub use loader::HighResImageLoader;
pub use manager::ViewerTabManager;
pub use mpv_engine::{MpvEventCallback, MpvFullscreenCallback, MpvPlayer};
pub use tab_item::{TabCollection, TabTitleFormatter};
pub use tab_merge::{MergeError, MergeMenu, MergeRefusal, MergeSide, MergeSource, MergedTabs};
pub use types::{MULTIVIEW_TAB_MEDIA_ID, MediaTabSession, MpvPlaybackState, MultiviewTabPayload, TabId, ViewerAction};
pub use x11::{
    OverlayServer, PointerProbe, SubwindowGeometry, ViewportBounds, X11VideoSubwindow, XRectangle,
    find_window_by_pid_and_title, find_window_by_title, force_x11_fullscreen, force_x11_fullscreen_on_monitor,
    force_x11_maximized, get_raw_window_xid, get_slint_window_xid, get_slint_window_xid_with_title_fallback,
    install_nonfatal_error_handler, is_x11_window_maximized, request_x11_modal_state, set_x11_dialog_parent,
    set_x11_initial_maximized, x11_display_answers, x11_root_size, x11_server_facts,
};
