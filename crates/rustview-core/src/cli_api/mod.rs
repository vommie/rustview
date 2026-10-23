pub mod commands;
pub mod error;
pub mod handlers;
pub mod models;

pub use commands::{ApiCommand, ApiSubcommand};
pub use error::ApiError;
pub use handlers::ApiHandler;
pub use models::*;

use crate::library::Library;
use std::path::Path;
use std::sync::Arc;

/// Standalone CLI API runner executing commands against `RustView`'s LMDB storage without initializing any GUI.
pub fn run_cli_api(command: ApiSubcommand, data_dir: &Path) -> Result<i32, ApiError> {
    let cmd = command.into_command()?;

    let db_path = data_dir.join("library.lmdb");
    if !db_path.exists() {
        let _ = std::fs::create_dir_all(&db_path);
    }

    let library = Arc::new(Library::open(&db_path)?);
    run_api_with_library(cmd, &library)
}

/// Executes a parsed API command directly on an open Library instance (ideal for integration testing and embedded use).
pub fn run_api_with_library(cmd: ApiCommand, library: &Library) -> Result<i32, ApiError> {
    match cmd {
        ApiCommand::TagTree { json } => ApiHandler::get_tag_tree(library, json),
        ApiCommand::GetMedia { path, json } => ApiHandler::get_media(library, &path, json),
        ApiCommand::SetMedia {
            path,
            rating,
            tags,
            hash,
            source,
            json,
        } => ApiHandler::set_media(library, &path, rating, tags, hash, source, json),
        ApiCommand::DeleteMedia { path, json } => ApiHandler::delete_media(library, &path, json),
        ApiCommand::CheckHash { hash, json } => ApiHandler::check_hash(library, &hash, json),
        ApiCommand::CheckHashes { hashes, json } => ApiHandler::check_hashes(library, &hashes, json),
        ApiCommand::InsertHash { hash, json } => ApiHandler::insert_hash(library, &hash, json),
    }
}
