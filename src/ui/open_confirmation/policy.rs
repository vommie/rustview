//! When an "Open" action asks first and what it says. Pure: no Slint, no settings access.

use crate::settings::schema::MIN_CONFIRM_OPEN_FILE_COUNT;

/// What the user is about to open; decides the wording of the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenKind {
    Default,
    OpenWith,
    FileManager,
    Tabs,
    BackgroundTabs,
}

impl OpenKind {
    fn action(self, count: usize) -> String {
        match self {
            Self::Default => format!("open {count} files with their default applications"),
            Self::OpenWith => format!("open {count} files with an application"),
            Self::FileManager => format!("show {count} items in the File Manager"),
            Self::Tabs => format!("open {count} files in tabs"),
            Self::BackgroundTabs => format!("open {count} files in background tabs"),
        }
    }

    pub fn log_name(self) -> &'static str {
        match self {
            Self::Default => "open",
            Self::OpenWith => "open-with",
            Self::FileManager => "file-manager",
            Self::Tabs => "tabs",
            Self::BackgroundTabs => "background-tabs",
        }
    }
}

/// True when `count` items reach the configured limit. A single item never asks, whatever the limit.
pub fn needs_confirmation(count: usize, limit: u32) -> bool {
    let limit = limit.max(MIN_CONFIRM_OPEN_FILE_COUNT) as usize;
    count >= limit
}

pub fn message(kind: OpenKind, count: usize) -> String {
    format!("You are about to {} at once. Do you want to continue?", kind.action(count))
}

pub const HINT: &str = "You can change this limit in Settings > General > Confirmations.";
