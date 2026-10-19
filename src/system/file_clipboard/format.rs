//! The data formats of files on the clipboard, shared with the file managers of the Linux desktop (pure, no I/O).
//!
//! A file manager that copies or cuts files offers the same list in several formats, and every file manager reads the one it knows:
//!
//! * `text/uri-list`: the list as `file://` URIs, one per line (RFC 2483, CRLF). Every desktop knows it; it cannot tell copy from cut.
//! * `application/x-kde-cutselection`: `1` when the files were cut, `0` when copied (Dolphin and other KDE applications).
//! * `x-special/gnome-copied-files`: `copy` or `cut` on the first line, then the URIs, one per line, LF, no trailing newline
//!   (Nautilus, Nemo, Caja, Thunar, PCManFM).
//! * `text/plain` and friends: the paths, so that a paste into a terminal or an editor gives something useful.
//!
//! What this module reads is untrusted data of another application: every entry is validated and a bad one is dropped and counted.

use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Component, Path, PathBuf};

pub const MIME_URI_LIST: &str = "text/uri-list";
pub const MIME_GNOME_COPIED_FILES: &str = "x-special/gnome-copied-files";
pub const MIME_KDE_CUT_SELECTION: &str = "application/x-kde-cutselection";
pub const MIME_TEXT_PLAIN: &str = "text/plain";
pub const MIME_TEXT_PLAIN_UTF8: &str = "text/plain;charset=utf-8";

/// More entries than this are refused: a clipboard of this size is not a selection of media but a mistake.
pub const MAX_FILES: usize = 20_000;
/// The longest clipboard payload that is read (bytes).
pub const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Copy,
    Cut,
}

/// Files on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardFiles {
    pub paths: Vec<PathBuf>,
    pub mode: ClipboardMode,
    /// Entries of the clipboard that were not usable files here (other schemes such as `smb:` or `trash:`, malformed lines).
    pub rejected: usize,
}

/// The result of parsing one list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedUris {
    pub paths: Vec<PathBuf>,
    pub rejected: usize,
}

const HEX: &[u8; 16] = b"0123456789ABCDEF";

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/')
}

/// `/home/me/a b.jpg` -> `file:///home/me/a%20b.jpg`
pub fn path_to_uri(path: &Path) -> String {
    let mut uri = String::with_capacity(path.as_os_str().len() + 8);
    uri.push_str("file://");
    for &byte in path.as_os_str().as_bytes() {
        if is_unreserved(byte) {
            uri.push(byte as char);
        } else {
            uri.push('%');
            uri.push(HEX[(byte >> 4) as usize] as char);
            uri.push(HEX[(byte & 0x0f) as usize] as char);
        }
    }
    uri
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decodes `%XX` sequences. `None` for a malformed sequence.
fn percent_decode(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if text[i] == b'%' {
            let high = hex_value(*text.get(i + 1)?)?;
            let low = hex_value(*text.get(i + 2)?)?;
            out.push(high << 4 | low);
            i += 3;
        } else {
            out.push(text[i]);
            i += 1;
        }
    }
    Some(out)
}

/// One `file:` URI to a path; `None` for anything that is not an absolute, local, well-formed file path.
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file:")?;
    // `file:///path`, `file://localhost/path` and the sloppy `file:/path`.
    let encoded = if let Some(after) = rest.strip_prefix("//") {
        let slash = after.find('/')?;
        let host = &after[..slash];
        if !(host.is_empty() || host.eq_ignore_ascii_case("localhost")) {
            return None;
        }
        &after[slash..]
    } else if rest.starts_with('/') {
        rest
    } else {
        return None;
    };
    let bytes = percent_decode(encoded.as_bytes())?;
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    // `PathBuf::components` hides `.` segments, so the raw bytes are checked.
    if bytes.first() != Some(&b'/') || bytes.split(|&b| b == b'/').any(|segment| segment == b"." || segment == b"..") {
        return None;
    }
    let path = PathBuf::from(std::ffi::OsString::from_vec(bytes));
    if path.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return None;
    }
    // A trailing slash names a folder; the entry is the folder itself.
    let raw = path.as_os_str().as_bytes();
    let trimmed = raw.strip_suffix(b"/").unwrap_or(raw);
    // The root itself is never a file to paste.
    (!trimmed.is_empty()).then(|| PathBuf::from(OsStr::from_bytes(trimmed)))
}

fn lines(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    data.split(|&b| b == b'\n').map(|line| line.strip_suffix(b"\r").unwrap_or(line))
}

/// A list of URIs, one per line. Blank lines and `#` comments are skipped; entries that are no usable local files are counted.
/// Duplicates are dropped, the order is kept.
pub fn parse_uri_lines(data: &[u8]) -> ParsedUris {
    let mut parsed = ParsedUris::default();
    let mut seen = std::collections::HashSet::new();
    for line in lines(data) {
        let Ok(line) = std::str::from_utf8(line) else {
            // Raw non-UTF-8 bytes are not valid in a URI (a file name with such bytes is sent percent-encoded).
            parsed.rejected += 1;
            continue;
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match uri_to_path(line) {
            Some(path) if parsed.paths.len() < MAX_FILES => {
                if seen.insert(path.clone()) {
                    parsed.paths.push(path);
                }
            }
            Some(_) | None => parsed.rejected += 1,
        }
    }
    parsed
}

/// `text/uri-list` body.
pub fn encode_uri_list(paths: &[PathBuf]) -> String {
    let mut out = String::new();
    for path in paths {
        out.push_str(&path_to_uri(path));
        out.push_str("\r\n");
    }
    out
}

/// `x-special/gnome-copied-files` body: `copy` or `cut`, then the URIs; LF between the lines, none at the end.
pub fn encode_gnome_copied_files(paths: &[PathBuf], mode: ClipboardMode) -> String {
    let mut out = String::from(match mode {
        ClipboardMode::Copy => "copy",
        ClipboardMode::Cut => "cut",
    });
    for path in paths {
        out.push('\n');
        out.push_str(&path_to_uri(path));
    }
    out
}

/// `application/x-kde-cutselection` body.
pub fn encode_kde_cut_selection(mode: ClipboardMode) -> &'static str {
    match mode {
        ClipboardMode::Copy => "0",
        ClipboardMode::Cut => "1",
    }
}

/// `text/plain` body: the paths, one per line.
pub fn encode_plain_text(paths: &[PathBuf]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(path.as_os_str().as_bytes());
    }
    out
}

/// The mode a `x-special/gnome-copied-files` body asks for and its list; `None` when the first line is neither `copy` nor `cut`.
pub fn parse_gnome_copied_files(data: &[u8]) -> Option<(ClipboardMode, ParsedUris)> {
    let split = data.iter().position(|&b| b == b'\n').unwrap_or(data.len());
    let mode = match std::str::from_utf8(&data[..split]).ok()?.trim() {
        "copy" => ClipboardMode::Copy,
        "cut" => ClipboardMode::Cut,
        _ => return None,
    };
    Some((mode, parse_uri_lines(data.get(split + 1..).unwrap_or(&[]))))
}

/// What a clipboard offers, as far as files go: the bodies of the formats above (`None` where the owner does not offer one).
#[derive(Debug, Clone, Default)]
pub struct FilePayloads {
    pub uri_list: Option<Vec<u8>>,
    pub gnome_copied_files: Option<Vec<u8>>,
    pub kde_cut_selection: Option<Vec<u8>>,
}

/// Files on the clipboard, or `None` when it holds no file list. The list comes from `text/uri-list`, else from the GNOME format; a cut
/// is announced by the KDE flag (`1`), else by the GNOME format; anything else is a copy.
pub fn files_from_payloads(payloads: &FilePayloads) -> Option<ClipboardFiles> {
    let gnome = payloads.gnome_copied_files.as_deref().and_then(parse_gnome_copied_files);
    let uri_list = payloads.uri_list.as_deref().map(parse_uri_lines);

    let (parsed, gnome_mode) = match (uri_list, gnome) {
        (Some(list), gnome) => (list, gnome.map(|(mode, _)| mode)),
        (None, Some((mode, list))) => (list, Some(mode)),
        (None, None) => return None,
    };

    let kde_mode = payloads
        .kde_cut_selection
        .as_deref()
        .map(|data| if String::from_utf8_lossy(data).trim() == "1" { ClipboardMode::Cut } else { ClipboardMode::Copy });
    let mode = kde_mode.or(gnome_mode).unwrap_or(ClipboardMode::Copy);

    Some(ClipboardFiles { paths: parsed.paths, mode, rejected: parsed.rejected })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<PathBuf> {
        items.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn uris_encode_everything_that_is_not_plain() {
        assert_eq!(path_to_uri(Path::new("/home/me/a b.jpg")), "file:///home/me/a%20b.jpg");
        assert_eq!(path_to_uri(Path::new("/m/Ümlaut #1 100%.png")), "file:///m/%C3%9Cmlaut%20%231%20100%25.png");
        assert_eq!(path_to_uri(Path::new("/plain/a-b_c.d~e")), "file:///plain/a-b_c.d~e");
    }

    #[test]
    fn paths_survive_the_round_trip_including_odd_bytes() {
        for text in ["/a/b.jpg", "/a b/c%20d.jpg", "/m/Ümlaut/日本語.mp4", "/m/new\nline.jpg", "/m/q?x#y&z=1;.png", "/m/+plus.png"] {
            let path = PathBuf::from(text);
            assert_eq!(uri_to_path(&path_to_uri(&path)).as_deref(), Some(path.as_path()), "{text:?}");
        }
        let odd = PathBuf::from(std::ffi::OsString::from_vec(vec![b'/', b'm', b'/', 0xff, 0xfe, b'.', b'j', b'p', b'g']));
        assert_eq!(uri_to_path(&path_to_uri(&odd)), Some(odd), "file names are bytes, not always UTF-8");
    }

    #[test]
    fn uri_forms_of_other_producers_are_accepted() {
        assert_eq!(uri_to_path("file:///a/b"), Some(PathBuf::from("/a/b")));
        assert_eq!(uri_to_path("file://localhost/a/b"), Some(PathBuf::from("/a/b")));
        assert_eq!(uri_to_path("file://LOCALHOST/a/b"), Some(PathBuf::from("/a/b")));
        assert_eq!(uri_to_path("file:/a/b"), Some(PathBuf::from("/a/b")));
        assert_eq!(uri_to_path("file:///a/b%2Fc"), Some(PathBuf::from("/a/b/c")));
        assert_eq!(uri_to_path("file:///a/dir/"), Some(PathBuf::from("/a/dir")));
    }

    #[test]
    fn bad_uris_are_refused() {
        for bad in [
            "http://example.com/a.jpg",
            "smb://server/share/a.jpg",
            "trash:///a.jpg",
            "file://otherhost/a/b.jpg",
            "file:relative/a.jpg",
            "file:///a/%zz.jpg",
            "file:///a/%2",
            "file:///a/%00b",
            "file:///a/../b.jpg",
            "file:///a/./b.jpg",
            "file://",
            "file:///",
            "/plain/path",
            "",
        ] {
            let result = uri_to_path(bad);
            assert!(result.is_none(), "{bad:?} must not give a path, got {result:?}");
        }
    }

    #[test]
    fn a_uri_list_drops_comments_blanks_duplicates_and_counts_what_it_cannot_use() {
        let data = b"# a comment\r\nfile:///a/1.jpg\r\n\r\nfile:///a/1.jpg\r\nsmb://s/2.jpg\r\nfile:///a/3%20x.jpg\nfile:///a/..%2F..%2Fb\n  file:///a/4.jpg  \n";
        let parsed = parse_uri_lines(data);
        assert_eq!(parsed.paths, paths(&["/a/1.jpg", "/a/3 x.jpg", "/a/4.jpg"]));
        assert_eq!(parsed.rejected, 2, "the smb entry and the one that climbs out");
    }

    #[test]
    fn a_huge_list_is_cut_at_the_limit_and_counted() {
        let mut data = String::new();
        for i in 0..MAX_FILES + 5 {
            data.push_str(&format!("file:///a/{i}.jpg\n"));
        }
        let parsed = parse_uri_lines(data.as_bytes());
        assert_eq!(parsed.paths.len(), MAX_FILES);
        assert_eq!(parsed.rejected, 5);
    }

    #[test]
    fn encoders_match_what_the_file_managers_write() {
        let list = paths(&["/a/1 x.jpg", "/a/2.png"]);
        assert_eq!(encode_uri_list(&list), "file:///a/1%20x.jpg\r\nfile:///a/2.png\r\n");
        assert_eq!(encode_gnome_copied_files(&list, ClipboardMode::Cut), "cut\nfile:///a/1%20x.jpg\nfile:///a/2.png");
        assert_eq!(encode_gnome_copied_files(&list, ClipboardMode::Copy), "copy\nfile:///a/1%20x.jpg\nfile:///a/2.png");
        assert_eq!(encode_kde_cut_selection(ClipboardMode::Cut), "1");
        assert_eq!(encode_kde_cut_selection(ClipboardMode::Copy), "0");
        assert_eq!(encode_plain_text(&list), b"/a/1 x.jpg\n/a/2.png");
    }

    #[test]
    fn everything_we_write_reads_back_the_same() {
        let list = paths(&["/a/1 x.jpg", "/m/Ümlaut.png"]);
        for mode in [ClipboardMode::Copy, ClipboardMode::Cut] {
            let payloads = FilePayloads {
                uri_list: Some(encode_uri_list(&list).into_bytes()),
                gnome_copied_files: Some(encode_gnome_copied_files(&list, mode).into_bytes()),
                kde_cut_selection: Some(encode_kde_cut_selection(mode).as_bytes().to_vec()),
            };
            let read = files_from_payloads(&payloads).unwrap();
            assert_eq!((read.paths, read.mode, read.rejected), (list.clone(), mode, 0));
        }
    }

    #[test]
    fn dolphin_cut_and_copy_are_told_apart_by_the_kde_flag() {
        let uris = Some(b"file:///a/1.jpg\r\n".to_vec());
        let cut = FilePayloads { uri_list: uris.clone(), kde_cut_selection: Some(b"1".to_vec()), ..Default::default() };
        let copy = FilePayloads { uri_list: uris.clone(), kde_cut_selection: Some(b"0".to_vec()), ..Default::default() };
        let plain = FilePayloads { uri_list: uris, ..Default::default() };
        assert_eq!(files_from_payloads(&cut).unwrap().mode, ClipboardMode::Cut);
        assert_eq!(files_from_payloads(&copy).unwrap().mode, ClipboardMode::Copy);
        assert_eq!(files_from_payloads(&plain).unwrap().mode, ClipboardMode::Copy, "a bare uri list is a copy");
    }

    #[test]
    fn nautilus_style_clipboards_work_without_the_uri_list() {
        let payloads = FilePayloads { gnome_copied_files: Some(b"cut\nfile:///a/1.jpg\nfile:///a/2.jpg".to_vec()), ..Default::default() };
        let read = files_from_payloads(&payloads).unwrap();
        assert_eq!((read.paths, read.mode), (paths(&["/a/1.jpg", "/a/2.jpg"]), ClipboardMode::Cut));
        let crlf = FilePayloads { gnome_copied_files: Some(b"copy\r\nfile:///a/1.jpg\r\n".to_vec()), ..Default::default() };
        assert_eq!(files_from_payloads(&crlf).unwrap().mode, ClipboardMode::Copy);
    }

    #[test]
    fn the_uri_list_wins_over_the_gnome_list_but_the_gnome_mode_counts_without_a_kde_flag() {
        let payloads = FilePayloads {
            uri_list: Some(b"file:///a/1.jpg\n".to_vec()),
            gnome_copied_files: Some(b"cut\nfile:///other/2.jpg".to_vec()),
            ..Default::default()
        };
        let read = files_from_payloads(&payloads).unwrap();
        assert_eq!((read.paths, read.mode), (paths(&["/a/1.jpg"]), ClipboardMode::Cut));
        let kde_wins = FilePayloads { kde_cut_selection: Some(b"0".to_vec()), ..payloads };
        assert_eq!(files_from_payloads(&kde_wins).unwrap().mode, ClipboardMode::Copy);
    }

    #[test]
    fn garbage_and_missing_formats_are_not_files() {
        assert!(files_from_payloads(&FilePayloads::default()).is_none());
        let bad_mode = FilePayloads { gnome_copied_files: Some(b"move\nfile:///a/1.jpg".to_vec()), ..Default::default() };
        assert!(files_from_payloads(&bad_mode).is_none(), "an unknown mode line is not a file list");
        let only_remote = FilePayloads { uri_list: Some(b"smb://s/a.jpg\nhttps://x/y.png\n".to_vec()), ..Default::default() };
        let read = files_from_payloads(&only_remote).unwrap();
        assert!(read.paths.is_empty());
        assert_eq!(read.rejected, 2, "the caller can tell the user that the files are not on this computer");
    }

    #[test]
    fn raw_invalid_utf8_in_a_line_is_dropped_and_does_not_spoil_the_others() {
        let mut data = b"file:///a/1.jpg\n".to_vec();
        data.extend_from_slice(b"file:///a/\xff\xfe.jpg\n");
        data.extend_from_slice(b"file:///a/2.jpg\n");
        let parsed = parse_uri_lines(&data);
        assert_eq!(parsed.paths, paths(&["/a/1.jpg", "/a/2.jpg"]));
        assert_eq!(parsed.rejected, 1);
    }

    #[test]
    fn unencoded_unicode_is_accepted() {
        assert_eq!(parse_uri_lines("file:///a/Ümlaut 日本.jpg\n".as_bytes()).paths, paths(&["/a/Ümlaut 日本.jpg"]));
    }
}
