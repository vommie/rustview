pub mod clipboard;
pub mod desktop;
pub mod display;
pub mod dnd;
pub mod error;
pub mod file_handler;
pub mod fs_access;
pub mod termination;
pub mod window_system;

pub use clipboard::SystemClipboard;
pub use desktop::{DesktopApp, DesktopIntegration};
pub use display::{MonitorInfo, SystemDisplayService};
pub use dnd::{DndExportMetrics, SystemDndHandler};
pub use error::SystemHandlerError;
pub use file_handler::SystemFileHandler;
