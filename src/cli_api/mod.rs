pub mod commands;
pub mod error;
pub mod handlers;
pub mod models;

pub use commands::{is_api_requested, parse_api_args, ApiCommand};
pub use error::ApiError;
pub use handlers::ApiHandler;
pub use models::*;

use crate::library::Library;
use std::path::Path;
use std::sync::Arc;

/// Standalone CLI API runner executing commands against RustView's LMDB storage without initializing any GUI.
pub fn run_cli_api(args: &[String], data_dir: &Path) -> Result<i32, ApiError> {
    let cmd = parse_api_args(args)?;

    if cmd == ApiCommand::Help {
        print_api_help();
        return Ok(0);
    }

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
            json,
        } => ApiHandler::set_media(library, &path, rating, tags, hash, json),
        ApiCommand::DeleteMedia { path, json } => {
            ApiHandler::delete_media(library, &path, json)
        }
        ApiCommand::CheckHash { hash, json } => ApiHandler::check_hash(library, &hash, json),
        ApiCommand::InsertHash { hash, json } => ApiHandler::insert_hash(library, &hash, json),
        ApiCommand::Help => {
            print_api_help();
            Ok(0)
        }
    }
}

pub fn print_api_help() {
    println!();
    println!("RustView Headless CLI-API Suite");
    println!("High-performance headless command-line interface for external tool integration (e.g. PyCut)");
    println!();
    println!("Usage:");
    println!("  rustview api <COMMAND> [OPTIONS]");
    println!();
    println!("Commands:");
    println!("  tag-tree                   Export hierarchical tag tree (IDs, parents, labels)");
    println!("  get-media                  Inspect and get database details for a media file");
    println!("  set-media                  Upsert media record, assign tags, ratings & ensure folder");
    println!("  delete-media               Remove media record and unassign associated tags");
    println!("  check-hash                 Query duplicate tracking database by MD5 checksum");
    println!("  insert-hash                Register a new checksum in the external hashes table");
    println!();
    println!("Options:");
    println!("  -p, --path <PATH>          Absolute path to the target media file");
    println!("  -r, --rating <0-5>         Star rating (0 = unrated, 1..=5 stars)");
    println!("  -t, --tags <ID,ID,...>     Comma-separated list of Tag IDs");
    println!("      --hash <MD5_HEX>       32-character hexadecimal MD5 hash string");
    println!("      --json                 Output machine-readable JSON to stdout");
    println!("  -h, --help                 Print this help message");
    println!();
    println!("Examples:");
    println!("  rustview api tag-tree --json");
    println!("  rustview api get-media --path /media/clip.mp4 --json");
    println!("  rustview api set-media --path /media/clip.mp4 --rating 5 --tags 1,4,7 --json");
    println!("  rustview api check-hash --hash d5dd11d265f453dee9d51a40ba717efb --json");
    println!();
}
