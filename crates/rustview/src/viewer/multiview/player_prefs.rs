//! The settings the video player keys read (Settings > Multiview > Jump Keys), kept as a global like the zoom preferences.

use parking_lot::RwLock;
use rustview_core::settings::schema::{
    DEFAULT_VIEWER_VIDEO_JUMP_LONG_SECS, DEFAULT_VIEWER_VIDEO_JUMP_SECS, ViewerVideoSettings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerPrefs {
    pub jump_secs: u32,
    pub jump_long_secs: u32,
}

static PREFS: RwLock<PlayerPrefs> = RwLock::new(PlayerPrefs {
    jump_secs: DEFAULT_VIEWER_VIDEO_JUMP_SECS,
    jump_long_secs: DEFAULT_VIEWER_VIDEO_JUMP_LONG_SECS,
});

pub fn current() -> PlayerPrefs {
    *PREFS.read()
}

/// Makes the video settings the current preferences (at startup and when they change).
pub fn apply(videos: &ViewerVideoSettings) {
    *PREFS.write() = PlayerPrefs {
        jump_secs: videos.jump_secs,
        jump_long_secs: videos.jump_long_secs,
    };
}
