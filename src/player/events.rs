use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    FileLoaded { path: PathBuf, duration: f64 },
    PlaybackStateChanged { is_paused: bool },
    PositionChanged { current_time: f64, duration: f64 },
    VolumeChanged { volume: f64, is_muted: bool },
    PlaybackEnded,
    Error(String),
}

pub fn format_playback_time(seconds: f64) -> String {
    if seconds.is_nan() || seconds < 0.0 {
        return "00:00".to_string();
    }
    let total_secs = seconds.round() as u64;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let hours = mins / 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins % 60, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}
