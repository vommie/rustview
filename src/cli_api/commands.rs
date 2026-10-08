use super::error::ApiError;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiCommand {
    TagTree {
        json: bool,
    },
    GetMedia {
        path: PathBuf,
        json: bool,
    },
    SetMedia {
        path: PathBuf,
        rating: Option<u8>,
        tags: Option<Vec<u32>>,
        hash: Option<String>,
        source: Option<String>,
        json: bool,
    },
    DeleteMedia {
        path: PathBuf,
        json: bool,
    },
    CheckHash {
        hash: String,
        json: bool,
    },
    CheckHashes {
        hashes: Vec<String>,
        json: bool,
    },
    InsertHash {
        hash: String,
        json: bool,
    },
    Help,
}

/// Detects if an API invocation was requested on CLI.
pub fn is_api_requested(args: &[String]) -> bool {
    if args.len() < 2 {
        return false;
    }
    let first = &args[1];
    first == "api"
        || first == "--api"
        || first.starts_with("--api-")
        || first == "tag-tree"
        || first == "get-media"
        || first == "set-media"
        || first == "check-hash"
        || first == "check-hashes"
        || first == "batch-check-hashes"
        || first == "batch-check-hash"
        || first == "check-hash-batch"
        || first == "hashes-check"
        || first == "insert-hash"
}

/// Parses command line arguments into structured ApiCommand.
pub fn parse_api_args(args: &[String]) -> Result<ApiCommand, ApiError> {
    let mut json = false;
    let mut action: Option<String> = None;
    let mut path: Option<PathBuf> = None;
    let mut rating: Option<u8> = None;
    let mut tags: Option<Vec<u32>> = None;
    let mut hash: Option<String> = None;
    let mut source: Option<String> = None;
    let mut hashes_list: Vec<String> = Vec::new();
    let mut hashes_file: Option<PathBuf> = None;
    let mut from_stdin = false;

    let mut skip_next = false;
    let mut relevant_args: Vec<&str> = Vec::new();

    for (i, arg) in args.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }

        if arg == "api" || arg == "--api" {
            continue;
        }

        if arg == "--json" {
            json = true;
            continue;
        }

        if arg == "--help" || arg == "-h" {
            return Ok(ApiCommand::Help);
        }

        if action.is_none() && !arg.starts_with('-') {
            action = Some(arg.to_lowercase());
            continue;
        }

        if let Some(cmd) = arg.strip_prefix("--api-") {
            action = Some(cmd.to_lowercase());
            continue;
        }

        if arg == "--path" || arg == "-p" {
            if let Some(val) = args.get(i + 1) {
                path = Some(PathBuf::from(val));
                skip_next = true;
                continue;
            }
        } else if let Some(val) = arg.strip_prefix("--path=") {
            path = Some(PathBuf::from(val));
            continue;
        }

        if arg == "--rating" || arg == "-r" {
            if let Some(val) = args.get(i + 1) {
                let r: u8 = val.parse().map_err(|_| {
                    ApiError::InvalidArgument("Rating must be an integer between 0 and 5".into())
                })?;
                rating = Some(r.min(5));
                skip_next = true;
                continue;
            }
        } else if let Some(val) = arg.strip_prefix("--rating=") {
            let r: u8 = val.parse().map_err(|_| {
                ApiError::InvalidArgument("Rating must be an integer between 0 and 5".into())
            })?;
            rating = Some(r.min(5));
            continue;
        }

        if arg == "--tags" || arg == "-t" {
            if let Some(val) = args.get(i + 1) {
                let parsed = parse_tag_list(val)?;
                tags = Some(parsed);
                skip_next = true;
                continue;
            }
        } else if let Some(val) = arg.strip_prefix("--tags=") {
            let parsed = parse_tag_list(val)?;
            tags = Some(parsed);
            continue;
        }

        if arg == "--hash" {
            if let Some(val) = args.get(i + 1) {
                hash = Some(val.trim().to_lowercase());
                skip_next = true;
                continue;
            }
        } else if let Some(val) = arg.strip_prefix("--hash=") {
            hash = Some(val.trim().to_lowercase());
            continue;
        }

        if arg == "--source" || arg == "--source-app" || arg == "-s" {
            if let Some(val) = args.get(i + 1) {
                source = Some(val.trim().to_string());
                skip_next = true;
                continue;
            }
        } else if let Some(val) = arg.strip_prefix("--source=") {
            source = Some(val.trim().to_string());
            continue;
        } else if let Some(val) = arg.strip_prefix("--source-app=") {
            source = Some(val.trim().to_string());
            continue;
        }

        if arg == "--hashes" || arg == "-H" {
            if let Some(val) = args.get(i + 1) {
                if val == "-" {
                    from_stdin = true;
                } else {
                    for part in val.split([',', ';', ' ', '\t', '\n']) {
                        let trimmed = part.trim().to_lowercase();
                        if !trimmed.is_empty() {
                            hashes_list.push(trimmed);
                        }
                    }
                }
                skip_next = true;
                continue;
            }
        } else if let Some(val) = arg.strip_prefix("--hashes=") {
            if val == "-" {
                from_stdin = true;
            } else {
                for part in val.split([',', ';', ' ', '\t', '\n']) {
                    let trimmed = part.trim().to_lowercase();
                    if !trimmed.is_empty() {
                        hashes_list.push(trimmed);
                    }
                }
            }
            continue;
        }

        if arg == "--hashes-file" || arg == "--file" || arg == "-f" {
            if let Some(val) = args.get(i + 1) {
                hashes_file = Some(PathBuf::from(val));
                skip_next = true;
                continue;
            }
        } else if let Some(val) = arg.strip_prefix("--hashes-file=") {
            hashes_file = Some(PathBuf::from(val));
            continue;
        } else if let Some(val) = arg.strip_prefix("--file=") {
            hashes_file = Some(PathBuf::from(val));
            continue;
        }

        if arg == "--stdin" || arg == "-" {
            from_stdin = true;
            continue;
        }

        relevant_args.push(arg);
    }

    // Read hashes from file if requested
    if let Some(ref file_p) = hashes_file {
        match std::fs::read_to_string(file_p) {
            Ok(content) => {
                for line in content.lines() {
                    let trimmed_line = line.trim();
                    if !trimmed_line.is_empty() && !trimmed_line.starts_with('#') {
                        for part in trimmed_line.split([',', ';', ' ', '\t']) {
                            let clean = part.trim().to_lowercase();
                            if !clean.is_empty() {
                                hashes_list.push(clean);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                return Err(ApiError::Io(e));
            }
        }
    }

    // Read hashes from STDIN if requested
    if from_stdin {
        let mut buffer = String::new();
        if std::io::stdin().read_to_string(&mut buffer).is_ok() {
            for line in buffer.lines() {
                let trimmed_line = line.trim();
                if !trimmed_line.is_empty() && !trimmed_line.starts_with('#') {
                    for part in trimmed_line.split([',', ';', ' ', '\t']) {
                        let clean = part.trim().to_lowercase();
                        if !clean.is_empty() {
                            hashes_list.push(clean);
                        }
                    }
                }
            }
        }
    }

    let act = action.unwrap_or_else(|| "help".to_string());

    match act.as_str() {
        "tag-tree" | "tags-tree" | "tags" => Ok(ApiCommand::TagTree { json }),
        "get-media" | "get" | "query" | "info" => {
            let target_path = path
                .or_else(|| relevant_args.first().map(PathBuf::from))
                .ok_or_else(|| {
                    ApiError::MissingArgument("Target file path is required (--path <PATH>)".into())
                })?;
            Ok(ApiCommand::GetMedia {
                path: target_path,
                json,
            })
        }
        "set-media" | "set" | "upsert" | "tag" | "rate" => {
            let target_path = path
                .or_else(|| relevant_args.first().map(PathBuf::from))
                .ok_or_else(|| {
                    ApiError::MissingArgument("Target file path is required (--path <PATH>)".into())
                })?;
            Ok(ApiCommand::SetMedia {
                path: target_path,
                rating,
                tags,
                hash,
                source,
                json,
            })
        }
        "delete-media" | "delete" | "remove" => {
            let target_path = path
                .or_else(|| relevant_args.first().map(PathBuf::from))
                .ok_or_else(|| {
                    ApiError::MissingArgument("Target file path is required (--path <PATH>)".into())
                })?;
            Ok(ApiCommand::DeleteMedia {
                path: target_path,
                json,
            })
        }
        "check-hash" | "hash-check" => {
            let target_hash = hash
                .or_else(|| relevant_args.first().map(|s| s.to_string()))
                .ok_or_else(|| {
                    ApiError::MissingArgument("MD5 hash string is required (--hash <MD5>)".into())
                })?;
            Ok(ApiCommand::CheckHash {
                hash: target_hash,
                json,
            })
        }
        "check-hashes"
        | "batch-check-hashes"
        | "batch-check-hash"
        | "check-hash-batch"
        | "hashes-check"
        | "hashes" => {
            for arg in &relevant_args {
                for part in arg.split([',', ';', ' ', '\t']) {
                    let clean = part.trim().to_lowercase();
                    if !clean.is_empty() {
                        hashes_list.push(clean);
                    }
                }
            }
            if hashes_list.is_empty() {
                return Err(ApiError::MissingArgument(
                    "At least one MD5 hash string is required (--hashes <MD5,MD5...>, --hashes-file <FILE>, or positional arguments)"
                        .into(),
                ));
            }
            Ok(ApiCommand::CheckHashes {
                hashes: hashes_list,
                json,
            })
        }
        "insert-hash" | "hash-insert" => {
            let target_hash = hash
                .or_else(|| relevant_args.first().map(|s| s.to_string()))
                .ok_or_else(|| {
                    ApiError::MissingArgument("MD5 hash string is required (--hash <MD5>)".into())
                })?;
            Ok(ApiCommand::InsertHash {
                hash: target_hash,
                json,
            })
        }
        "help" => Ok(ApiCommand::Help),
        other => Err(ApiError::UnknownCommand(other.to_string())),
    }
}

fn parse_tag_list(input: &str) -> Result<Vec<u32>, ApiError> {
    let mut list = Vec::new();
    for part in input.split([',', ';', ' ']) {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            let id: u32 = trimmed.parse().map_err(|_| {
                ApiError::InvalidArgument(format!(
                    "Invalid tag ID '{}'. Tag IDs must be numbers.",
                    trimmed
                ))
            })?;
            list.push(id);
        }
    }
    Ok(list)
}
