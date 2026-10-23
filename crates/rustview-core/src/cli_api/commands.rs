//! Command line of the headless API, parsed with `clap`.
//!
//! External tools (PyCutRv, remove_dups) call `rustview api <command> [options]`; the same commands also work without the leading `api`.
//! The long list of aliases is the compatibility surface those tools were written against.

use super::error::ApiError;
use clap::{Args, Subcommand};
use std::io::Read;
use std::path::{Path, PathBuf};

/// A validated API command, ready to run against the library.
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
}

/// The commands of the API as they are typed on the command line.
#[derive(Debug, Clone, Subcommand)]
pub enum ApiSubcommand {
    /// Export the hierarchical tag tree (IDs, parents, labels)
    #[command(aliases = ["tags-tree", "tags"])]
    TagTree(OutputArgs),
    /// Inspect and get the database details of a media file
    #[command(aliases = ["get", "query", "info"])]
    GetMedia(MediaPathArgs),
    /// Upsert a media record: assign tags and a rating, ensure its folder
    #[command(aliases = ["set", "upsert", "tag", "rate"])]
    SetMedia(SetMediaArgs),
    /// Remove a media record and unassign its tags
    #[command(aliases = ["delete", "remove"])]
    DeleteMedia(MediaPathArgs),
    /// Query the duplicate tracking database by a single MD5 checksum
    #[command(alias = "hash-check")]
    CheckHash(HashArgs),
    /// Query the duplicate tracking database for several MD5 checksums
    #[command(aliases = ["batch-check-hashes", "batch-check-hash", "check-hash-batch", "hashes-check", "hashes"])]
    CheckHashes(HashListArgs),
    /// Register a new checksum in the external hashes table
    #[command(alias = "hash-insert")]
    InsertHash(HashArgs),
}

#[derive(Debug, Clone, Args)]
pub struct OutputArgs {
    /// Output machine-readable JSON to stdout
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Args)]
pub struct MediaPathArgs {
    /// Absolute path to the target media file
    #[arg(short, long, value_name = "PATH")]
    pub path: Option<PathBuf>,
    /// The path as a positional argument (same as --path)
    #[arg(value_name = "PATH")]
    pub positional_path: Option<PathBuf>,
    #[command(flatten)]
    pub output: OutputArgs,
}

#[derive(Debug, Clone, Args)]
pub struct SetMediaArgs {
    #[command(flatten)]
    pub media: MediaPathArgs,
    /// Star rating (0 = unrated, 1..=5 stars)
    #[arg(short, long, value_name = "0-5")]
    pub rating: Option<String>,
    /// Comma-separated list of tag IDs
    #[arg(short, long, value_name = "ID,ID,...")]
    pub tags: Option<String>,
    /// 32-character hexadecimal MD5 hash string
    #[arg(long, value_name = "MD5_HEX")]
    pub hash: Option<String>,
    /// Application identifier creating this entry (default: 'CLI-API')
    #[arg(short, long, alias = "source-app", value_name = "APP_NAME")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct HashArgs {
    /// 32-character hexadecimal MD5 hash string
    #[arg(long, value_name = "MD5_HEX")]
    pub hash: Option<String>,
    /// The hash as a positional argument (same as --hash)
    #[arg(value_name = "MD5_HEX")]
    pub positional_hash: Option<String>,
    #[command(flatten)]
    pub output: OutputArgs,
}

#[derive(Debug, Clone, Args)]
pub struct HashListArgs {
    /// Comma- or space-separated list of MD5 hash strings ('-' reads them from standard input)
    #[arg(short = 'H', long, value_name = "MD5,MD5...")]
    pub hashes: Vec<String>,
    /// File with MD5 hashes (one or more per line, '#' starts a comment line)
    #[arg(short = 'f', long, alias = "file", value_name = "FILE")]
    pub hashes_file: Option<PathBuf>,
    /// Read the hashes from standard input
    #[arg(long)]
    pub stdin: bool,
    /// MD5 hash strings as positional arguments ('-' reads them from standard input)
    #[arg(value_name = "MD5")]
    pub positional_hashes: Vec<String>,
    #[command(flatten)]
    pub output: OutputArgs,
}

fn parse_rating(text: &str) -> Result<u8, ApiError> {
    text.parse::<u8>()
        .map(|rating| rating.min(5))
        .map_err(|_| ApiError::InvalidArgument("Rating must be an integer between 0 and 5".into()))
}

fn parse_tag_list(text: &str) -> Result<Vec<u32>, ApiError> {
    text.split([',', ';', ' '])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.parse::<u32>()
                .map_err(|_| ApiError::InvalidArgument(format!("Invalid tag ID '{part}'. Tag IDs must be numbers.")))
        })
        .collect()
}

/// Splits `text` at commas, semicolons and white space into lower-case, non-empty hash strings.
fn split_hashes(text: &str, separators: &[char]) -> Vec<String> {
    text.split(separators)
        .map(|part| part.trim().to_lowercase())
        .filter(|part| !part.is_empty())
        .collect()
}

/// Hash strings from the lines of a file or of standard input: blank lines and `#` comment lines are skipped.
fn hashes_from_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .flat_map(|line| split_hashes(line, &[',', ';', ' ', '\t']))
        .collect()
}

fn required_path(path: Option<PathBuf>, positional: Option<PathBuf>) -> Result<PathBuf, ApiError> {
    path.or(positional)
        .ok_or_else(|| ApiError::MissingArgument("Target file path is required (--path <PATH>)".into()))
}

fn required_hash(hash: Option<String>, positional: Option<String>) -> Result<String, ApiError> {
    hash.or(positional)
        .map(|hash| hash.trim().to_lowercase())
        .ok_or_else(|| ApiError::MissingArgument("MD5 hash string is required (--hash <MD5>)".into()))
}

impl ApiSubcommand {
    /// Validates the arguments (reading hash lists from files or standard input where asked) and returns the command to run.
    pub fn into_command(self) -> Result<ApiCommand, ApiError> {
        match self {
            Self::TagTree(output) => Ok(ApiCommand::TagTree { json: output.json }),
            Self::GetMedia(args) => Ok(ApiCommand::GetMedia {
                path: required_path(args.path, args.positional_path)?,
                json: args.output.json,
            }),
            Self::SetMedia(args) => Ok(ApiCommand::SetMedia {
                path: required_path(args.media.path, args.media.positional_path)?,
                rating: args.rating.as_deref().map(parse_rating).transpose()?,
                tags: args.tags.as_deref().map(parse_tag_list).transpose()?,
                hash: args.hash.map(|hash| hash.trim().to_lowercase()),
                source: args.source.map(|source| source.trim().to_owned()),
                json: args.media.output.json,
            }),
            Self::DeleteMedia(args) => Ok(ApiCommand::DeleteMedia {
                path: required_path(args.path, args.positional_path)?,
                json: args.output.json,
            }),
            Self::CheckHash(args) => Ok(ApiCommand::CheckHash {
                hash: required_hash(args.hash, args.positional_hash)?,
                json: args.output.json,
            }),
            Self::InsertHash(args) => Ok(ApiCommand::InsertHash {
                hash: required_hash(args.hash, args.positional_hash)?,
                json: args.output.json,
            }),
            Self::CheckHashes(args) => {
                let hashes = collect_hashes(&args)?;
                if hashes.is_empty() {
                    return Err(ApiError::MissingArgument(
                        "At least one MD5 hash string is required (--hashes <MD5,MD5...>, --hashes-file <FILE>, or positional arguments)".into(),
                    ));
                }
                Ok(ApiCommand::CheckHashes { hashes, json: args.output.json })
            }
        }
    }
}

/// All hashes of a `check-hashes` call: the option values, the file, standard input and the positional arguments, in this order.
fn collect_hashes(args: &HashListArgs) -> Result<Vec<String>, ApiError> {
    let mut from_stdin = args.stdin;
    let mut hashes = Vec::new();

    for value in &args.hashes {
        if value == "-" {
            from_stdin = true;
        } else {
            hashes.extend(split_hashes(value, &[',', ';', ' ', '\t', '\n']));
        }
    }
    if let Some(file) = args.hashes_file.as_deref() {
        hashes.extend(hashes_from_lines(&read_file(file)?));
    }
    let mut positional = Vec::new();
    for value in &args.positional_hashes {
        if value == "-" {
            from_stdin = true;
        } else {
            positional.extend(split_hashes(value, &[',', ';', ' ', '\t']));
        }
    }
    if from_stdin {
        let mut buffer = String::new();
        if std::io::stdin().read_to_string(&mut buffer).is_ok() {
            hashes.extend(hashes_from_lines(&buffer));
        }
    }
    hashes.extend(positional);
    Ok(hashes)
}

fn read_file(path: &Path) -> Result<String, ApiError> {
    std::fs::read_to_string(path).map_err(ApiError::Io)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    /// The command line of a test: the API commands directly (the application crate adds the optional `api` word).
    #[derive(Debug, Parser)]
    struct TestCli {
        #[command(subcommand)]
        command: ApiSubcommand,
    }

    /// Parses a command line the way the application does and returns the API command.
    fn parse(args: &[&str]) -> Result<ApiCommand, String> {
        let cli = TestCli::try_parse_from(std::iter::once("rustview").chain(args.iter().copied())).map_err(|e| e.to_string())?;
        cli.command.into_command().map_err(|e| e.to_string())
    }

    #[test]
    fn the_documented_examples_parse() {
        assert_eq!(parse(&["tag-tree", "--json"]), Ok(ApiCommand::TagTree { json: true }));
        assert_eq!(
            parse(&["get-media", "--path", "/media/clip.mp4", "--json"]),
            Ok(ApiCommand::GetMedia { path: "/media/clip.mp4".into(), json: true })
        );
        assert_eq!(
            parse(&["set-media", "--path", "/media/clip.mp4", "--rating", "5", "--tags", "1,4,7", "--source", "PyCutRv", "--json"]),
            Ok(ApiCommand::SetMedia {
                path: "/media/clip.mp4".into(),
                rating: Some(5),
                tags: Some(vec![1, 4, 7]),
                hash: None,
                source: Some("PyCutRv".into()),
                json: true,
            })
        );
        assert_eq!(
            parse(&["check-hash", "--hash", "D5DD11D265F453DEE9D51A40BA717EFB", "--json"]),
            Ok(ApiCommand::CheckHash { hash: "d5dd11d265f453dee9d51a40ba717efb".into(), json: true })
        );
    }

    #[test]
    fn the_command_works_without_the_api_word_and_under_its_aliases() {
        let expected = Ok(ApiCommand::GetMedia { path: "/a.jpg".into(), json: false });
        assert_eq!(parse(&["get-media", "--path", "/a.jpg"]), expected);
        assert_eq!(parse(&["get", "/a.jpg"]), expected);
        assert_eq!(parse(&["info", "-p", "/a.jpg"]), expected);
        assert_eq!(parse(&["tags"]), Ok(ApiCommand::TagTree { json: false }));
        assert_eq!(parse(&["remove", "/a.jpg", "--json"]), Ok(ApiCommand::DeleteMedia { path: "/a.jpg".into(), json: true }));
    }

    #[test]
    fn the_equals_form_and_the_old_option_spellings_work() {
        assert_eq!(
            parse(&["set-media", "--path=/a.jpg", "--rating=3", "--tags=1;2 3", "--source-app=Tool"]),
            Ok(ApiCommand::SetMedia {
                path: "/a.jpg".into(),
                rating: Some(3),
                tags: Some(vec![1, 2, 3]),
                hash: None,
                source: Some("Tool".into()),
                json: false,
            })
        );
    }

    #[test]
    fn a_rating_above_five_is_capped_and_garbage_is_refused() {
        let command = parse(&["set-media", "/a.jpg", "--rating", "9"]);
        assert!(matches!(command, Ok(ApiCommand::SetMedia { rating: Some(5), .. })));
        assert!(parse(&["set-media", "/a.jpg", "--rating", "x"]).is_err());
        assert!(parse(&["set-media", "/a.jpg", "--tags", "1,b"]).is_err());
    }

    #[test]
    fn a_missing_path_or_hash_is_reported() {
        assert!(parse(&["get-media"]).unwrap_err().contains("Target file path is required"));
        assert!(parse(&["check-hash"]).unwrap_err().contains("MD5 hash string is required"));
    }

    #[test]
    fn hash_lists_are_split_and_lower_cased() {
        assert_eq!(
            parse(&["check-hashes", "--hashes", "AA,bb;cc dd", "ee"]),
            Ok(ApiCommand::CheckHashes { hashes: ["aa", "bb", "cc", "dd", "ee"].map(String::from).to_vec(), json: false })
        );
        assert!(parse(&["check-hashes"]).unwrap_err().contains("At least one MD5 hash string"));
    }

    #[test]
    fn hash_files_skip_comments_and_blank_lines() {
        assert_eq!(hashes_from_lines("# list\nAA bb\n\n  cc,dd  \n"), ["aa", "bb", "cc", "dd"].map(String::from).to_vec());
    }
}
