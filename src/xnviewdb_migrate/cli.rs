use super::error::MigrationError;
use super::logger::MigrationLogger;
use super::mapper::XnViewMapper;
use super::reader::XnViewDbReader;
use super::stats::MigrationStats;
use super::writer::XnViewDbWriter;
use crate::library::folders::normalize_path;
use crate::settings::{LogObscurer, SettingsManager};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Checks whether migration arguments or help options were supplied on the command line.
pub fn is_migration_requested(args: &[String]) -> bool {
    args.iter().any(|arg| {
        arg == "--migrate-xnview"
            || arg == "--migrate-xnview-db"
            || arg == "--migrate"
            || arg == "-m"
            || arg == "--help"
            || arg == "-h"
    })
}

/// Executes the CLI migration workflow if requested.
pub fn run_cli(
    args: &[String],
    data_dir: &Path,
    config_dir: &Path,
) -> Result<(), MigrationError> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    let mut xnview_path_opt: Option<PathBuf> = None;
    let mut auto_confirm = false;
    let mut skip_missing = true; // Default: true (skip non-existing folders and files)
    let mut root_filter_enabled = true; // Default: true (only import paths under configured base/root)
    let mut explicit_root_path: Option<PathBuf> = None;
    let mut enable_log = true; // Default: true (write audit log in home dir)
    let mut custom_log_path: Option<PathBuf> = None;
    let mut explicit_obfuscate: Option<bool> = None;

    let mut iter = args.iter().skip(1);
    while let Some(arg) = iter.next() {
        if arg == "--migrate-xnview"
            || arg == "--migrate-xnview-db"
            || arg == "--migrate"
            || arg == "-m"
        {
            if let Some(val) = iter.next() {
                xnview_path_opt = Some(PathBuf::from(val));
            }
        } else if let Some(path_str) = arg.strip_prefix("--migrate-xnview=") {
            xnview_path_opt = Some(PathBuf::from(path_str));
        } else if let Some(path_str) = arg.strip_prefix("--migrate=") {
            xnview_path_opt = Some(PathBuf::from(path_str));
        } else if arg == "--include-missing" || arg == "--no-skip-missing" {
            skip_missing = false;
        } else if arg == "--skip-missing" {
            skip_missing = true;
        } else if arg == "--no-root-filter" || arg == "--all-folders" {
            root_filter_enabled = false;
        } else if arg == "--root-filter" || arg == "--only-root" {
            root_filter_enabled = true;
        } else if arg == "--root" || arg == "--root-path" {
            if let Some(val) = iter.next() {
                explicit_root_path = Some(PathBuf::from(val));
                root_filter_enabled = true;
            }
        } else if let Some(path_str) = arg.strip_prefix("--root=") {
            explicit_root_path = Some(PathBuf::from(path_str));
            root_filter_enabled = true;
        } else if let Some(path_str) = arg.strip_prefix("--root-path=") {
            explicit_root_path = Some(PathBuf::from(path_str));
            root_filter_enabled = true;
        } else if arg == "--no-log" {
            enable_log = false;
        } else if arg == "--log" {
            enable_log = true;
        } else if arg == "--log-file" {
            if let Some(val) = iter.next() {
                custom_log_path = Some(PathBuf::from(val));
                enable_log = true;
            }
        } else if let Some(path_str) = arg.strip_prefix("--log-file=") {
            custom_log_path = Some(PathBuf::from(path_str));
            enable_log = true;
        } else if arg == "--obfuscate-logs" || arg == "--obfuscate" {
            explicit_obfuscate = Some(true);
        } else if arg == "--no-obfuscate-logs" || arg == "--plain-logs" || arg == "--clear-logs" {
            explicit_obfuscate = Some(false);
        } else if arg == "--yes" || arg == "-y" || arg == "--force" {
            auto_confirm = true;
        }
    }

    let Some(xnview_db_path) = xnview_path_opt else {
        eprintln!("Error: Missing required path to XnViewMP database file (XnView.db).");
        eprintln!("Usage: rustview --migrate-xnview /path/to/XnView.db");
        return Err(MigrationError::FileNotFound("Path not provided".into()));
    };

    if !xnview_db_path.exists() {
        eprintln!(
            "Error: Specified XnViewMP database file does not exist: {:?}",
            xnview_db_path
        );
        return Err(MigrationError::FileNotFound(
            xnview_db_path.to_string_lossy().to_string(),
        ));
    }

    // 1. Resolve & Configure Base/Root Folder Filtering
    let settings_mgr = SettingsManager::new(config_dir);
    let mut active_root_path: Option<PathBuf> = None;

    if let Some(explicit_root) = explicit_root_path {
        let norm_root = normalize_input_path(&explicit_root);
        if !norm_root.is_dir() {
            eprintln!(
                "Error: Specified base root directory does not exist: {:?}",
                norm_root
            );
            return Err(MigrationError::FileNotFound(
                norm_root.to_string_lossy().to_string(),
            ));
        }

        // Persist validated base path to settings
        let mut app_settings = settings_mgr.get();
        app_settings.general.root_folder = Some(norm_root.clone());
        settings_mgr.save_quiet(&app_settings);
        active_root_path = Some(norm_root);
    } else if root_filter_enabled {
        let current_setting_root = settings_mgr.get().general.root_folder;

        if let Some(existing_root) = current_setting_root {
            let norm_root = normalize_path(&existing_root);
            if norm_root.is_dir() {
                active_root_path = Some(norm_root);
            }
        }

        // If root filtering is enabled but no valid base path is configured in settings
        if active_root_path.is_none() && !auto_confirm {
            println!();
            println!("--------------------------------------------------------------------------------");
            println!("NOTICE: Base/Root Path Filtering is ENABLED (Default), but no root folder");
            println!("is currently configured in RustView settings.");
            println!();
            println!("  [1] Enter a Base Path now (will be validated and saved to settings)");
            println!("  [2] Continue WITHOUT filtering (import all folders from XnViewMP)");
            println!("  [C] Cancel migration");
            println!("--------------------------------------------------------------------------------");
            print!("Please select an option [1/2/C]: ");
            io::stdout().flush()?;

            let mut choice = String::new();
            io::stdin().read_line(&mut choice)?;
            let trimmed_choice = choice.trim().to_lowercase();

            if trimmed_choice == "1" {
                print!("Enter Base/Root Directory path: ");
                io::stdout().flush()?;
                let mut path_input = String::new();
                io::stdin().read_line(&mut path_input)?;
                let raw_input = path_input.trim();

                if raw_input.is_empty() {
                    println!("Error: Empty path provided. Aborting migration.");
                    return Err(MigrationError::UserCancelled);
                }

                let norm_root = normalize_input_path(Path::new(raw_input));
                if !norm_root.is_dir() {
                    eprintln!(
                        "Error: Entered directory does not exist on disk: {:?}",
                        norm_root
                    );
                    return Err(MigrationError::FileNotFound(
                        norm_root.to_string_lossy().to_string(),
                    ));
                }

                // Persist new base path to settings
                let mut app_settings = settings_mgr.get();
                app_settings.general.root_folder = Some(norm_root.clone());
                settings_mgr.save_quiet(&app_settings);
                println!("Saved Base Path to settings: {}", norm_root.display());
                active_root_path = Some(norm_root);
            } else if trimmed_choice == "2" {
                println!("Continuing migration without root folder filtering.");
                active_root_path = None;
            } else {
                println!("Migration cancelled by user. No data was modified.");
                return Err(MigrationError::UserCancelled);
            }
        }
    }

    // 2. Resolve Log Privacy / Obfuscation Mode
    let mut obfuscate_logs = explicit_obfuscate.unwrap_or_else(|| settings_mgr.get().debug.obfuscate_logs);

    // If obfuscation is active and user is interactive, prompt whether they want to keep it
    if explicit_obfuscate.is_none() && obfuscate_logs && !auto_confirm {
        println!();
        println!("--------------------------------------------------------------------------------");
        println!("PRIVACY NOTICE: Log Obfuscation / Data Masking is currently ENABLED.");
        println!("All file names, folder paths, and tag labels in the audit log will be masked with IDs.");
        println!();
        println!("If you plan to inspect the audit log yourself to verify skipped files or folders,");
        println!("you may prefer plain text logging for this migration run.");
        println!("--------------------------------------------------------------------------------");
        print!("Keep log obfuscation enabled? [Y/n] (Enter 'n' for plain text log): ");
        io::stdout().flush()?;

        let mut obsc_input = String::new();
        io::stdin().read_line(&mut obsc_input)?;
        let trimmed_obsc = obsc_input.trim().to_lowercase();
        if trimmed_obsc == "n" || trimmed_obsc == "no" {
            obfuscate_logs = false;
            println!("Log Obfuscation disabled for this migration run (plain text audit trail).");
        } else {
            println!("Log Obfuscation kept enabled (privacy-masked audit trail).");
        }
    }

    LogObscurer::set_obfuscate(obfuscate_logs);

    // 3. Initialize Migration Audit Logger
    let mut logger = MigrationLogger::new(enable_log, custom_log_path)?;

    println!();
    println!("================================================================================");
    println!("                  RUSTVIEW - XNVIEWMP DATABASE MIGRATION                        ");
    println!("================================================================================");
    println!("Source XnViewMP DB: {}", LogObscurer::path(&xnview_db_path));
    println!("Target RustView Dir: {}", LogObscurer::path(data_dir));
    println!(
        "Base/Root Filter   : {}",
        match active_root_path {
            Some(ref r) => format!("Active (only paths under '{}')", LogObscurer::path(r)),
            None => "Disabled (importing all folders)".to_string(),
        }
    );
    println!(
        "Log Privacy Mode   : {}",
        if LogObscurer::is_active() {
            "Enabled (Obfuscated IDs/Hashes)"
        } else {
            "Disabled (Plain text audit trail)"
        }
    );
    if let Some(log_p) = logger.path() {
        println!("Audit Log File    : {}", log_p.display());
    } else {
        println!("Audit Log File    : Disabled");
    }
    println!(
        "Skip Missing Items: {}",
        if skip_missing {
            "Enabled (Default: non-existent folders & files skipped)"
        } else {
            "Disabled (All records imported, including missing paths)"
        }
    );
    println!("ID Re-indexing    : Enabled (Clean, contiguous 1-based IDs generated)");
    println!();
    println!("WARNING: DESTRUCTIVE OPERATION!");
    println!("This will permanently ERASE all existing RustView databases, media records,");
    println!("tag hierarchies, and thumbnail cache files in:");
    println!("  • {}/library.lmdb", data_dir.display());
    println!("  • {}/thumbnails", data_dir.display());
    println!();
    println!("All data will be freshly imported from the XnViewMP database.");
    println!("No automatic backup of the existing RustView database will be created.");
    println!("--------------------------------------------------------------------------------");

    if !auto_confirm {
        print!("Are you sure you want to permanently wipe RustView and migrate now? [y/N]: ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let trimmed = input.trim().to_lowercase();

        if trimmed != "y" && trimmed != "yes" {
            println!("Migration cancelled by user. No data was modified.");
            return Err(MigrationError::UserCancelled);
        }
    }

    logger.log_header(
        &xnview_db_path,
        data_dir,
        skip_missing,
        active_root_path.as_deref(),
    );

    println!();
    println!("[1/4] Extracting tables from XnViewMP SQLite database...");
    let t_total_start = Instant::now();
    let mut stats = MigrationStats::new(
        &xnview_db_path.to_string_lossy(),
        skip_missing,
        active_root_path
            .as_ref()
            .map(|p| p.to_string_lossy())
            .as_deref(),
    );

    let t_read = Instant::now();
    let raw_data = XnViewDbReader::read_database(&xnview_db_path)?;
    stats.read_time_ms = t_read.elapsed().as_millis();
    if let Some(ref v) = raw_data.version {
        stats.source_version = v.clone();
    }

    println!("[2/4] Verifying filesystem, filtering entries, re-indexing IDs, and building relations...");
    let t_map = Instant::now();
    let mapped_data = XnViewMapper::map_data(
        raw_data,
        skip_missing,
        active_root_path.as_deref(),
        &mut stats,
        &mut logger,
    );
    stats.map_time_ms = t_map.elapsed().as_millis();

    println!("[3/4] Wiping existing RustView database and creating clean LMDB tables...");
    println!("[4/4] Writing and atomically committing re-indexed records to LMDB storage...");
    XnViewDbWriter::wipe_and_write(data_dir, mapped_data, &mut stats)?;

    stats.total_time_ms = t_total_start.elapsed().as_millis();

    logger.log_summary(&stats);

    // Print summary report
    stats.print_summary();

    if let Some(log_p) = logger.path() {
        println!("Detailed audit log successfully saved to: {}", log_p.display());
        println!();
    }

    Ok(())
}

/// Expands leading `~` to `$HOME` and normalizes path.
fn normalize_input_path(input: &Path) -> PathBuf {
    let s = input.to_string_lossy();
    let path = if let Some(stripped) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(stripped)
        } else {
            input.to_path_buf()
        }
    } else if s == "~" {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home)
        } else {
            input.to_path_buf()
        }
    } else {
        input.to_path_buf()
    };

    normalize_path(&path)
}

fn print_help() {
    println!();
    println!("RustView - High-Performance Media Manager & Viewer");
    println!();
    println!("Usage:");
    println!("  rustview [OPTIONS]");
    println!();
    println!("Options:");
    println!("  --migrate-xnview <PATH>    Migrate an existing XnViewMP SQLite database (XnView.db)");
    println!("                             into RustView. (Destructive: wipes current RustView database)");
    println!("  -m <PATH>                  Alias for --migrate-xnview");
    println!("  --root <PATH>              Set and enforce a base/root directory filter (only paths");
    println!("                             under this folder are migrated). Also saves path to settings.");
    println!("  --root-filter              Enforce base/root path filter from settings (Default: enabled)");
    println!("  --no-root-filter           Disable base/root path filtering (import all folders)");
    println!("  --all-folders              Alias for --no-root-filter");
    println!("  --skip-missing             Skip folders and files that do not physically exist on disk (Default: enabled)");
    println!("  --include-missing          Include non-existent folders and files (disables missing check)");
    println!("  --obfuscate-logs           Force log and audit trail obfuscation (mask names and paths)");
    println!("  --no-obfuscate-logs        Disable log obfuscation (output clear text paths and names)");
    println!("  --plain-logs               Alias for --no-obfuscate-logs");
    println!("  --no-log                   Disable writing audit log file in user home directory");
    println!("  --log                      Enable writing audit log file (Default: enabled in ~/rustview_migration_*.log)");
    println!("  --log-file <PATH>          Specify custom output path for the migration audit log file");
    println!("  -y, --yes, --force         Bypass interactive confirmation prompt during migration");
    println!("  -h, --help                 Print this help message and exit");
    println!();
    println!("Examples:");
    println!("  rustview --migrate-xnview ~/.config/xnviewmp/XnView.db");
    println!("  rustview -m /path/to/XnView.db --root ~/images --yes");
    println!("  rustview -m /path/to/XnView.db --no-root-filter --plain-logs --yes");
    println!();
}
