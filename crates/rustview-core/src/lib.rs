//! RustView core: everything that works without a user interface (library database, media decoding, thumbnails, queries, settings,
//! the command line API and the XnView MP migration). The application crate `rustview` builds the Slint user interface on top.

pub mod cli_api;
pub mod library;
pub mod media;
pub mod query;
pub mod settings;
pub mod system;
pub mod telemetry;
pub mod thumbnails;
pub mod util;
pub mod xnviewdb_migrate;
