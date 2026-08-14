use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub enum PlayerCommand {
    LoadFile { path: PathBuf, autoplay: bool },
    Play,
    Pause,
    TogglePause,
    Stop,
    SeekAbsolute(f64),
    SeekRelative(f64),
    SetVolume(f64),
    ToggleMute,
    SetMute(bool),
}
