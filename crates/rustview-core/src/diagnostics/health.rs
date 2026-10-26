//! How the data and the environment of `RustView` are doing. Pure: made from [`HealthFacts`].

use super::dependencies::WindowSystemKind;
use super::{Check, Group, Status};
use crate::settings::LogObscurer;
use crate::telemetry::{format_bytes_metric, format_count};
use crate::util::cast::SaturatingAs;
use chrono::{DateTime, Local};
use std::path::{Path, PathBuf};

/// Free space below this is a warning, below [`FREE_SPACE_PROBLEM`] a problem.
pub const FREE_SPACE_WARNING: u64 = 1024 * 1024 * 1024;
pub const FREE_SPACE_PROBLEM: u64 = 200 * 1024 * 1024;

/// Folders under which drives are usually mounted; a root folder there that lies on the system disk was not mounted.
const MOUNT_BASES: [&str; 3] = ["/run/media", "/media", "/mnt"];

/// The numbers of the library database (from the database itself, never from the media).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryFacts {
    pub file_bytes: Option<u64>,
    pub media: u64,
    pub folders: u64,
    pub tags: u64,
}

/// The result of the folder audit: how many folders it read and what is out of place.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditFacts {
    pub folders: usize,
    pub elapsed_ms: u64,
    /// (count, what it is) for every kind of inconsistency found.
    pub problems: Vec<(usize, &'static str)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThumbnailFacts {
    pub thumbnails: usize,
    pub bytes: u64,
    pub media_without: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsFileFacts {
    /// `Err` = the file on the drive cannot be read or parsed now.
    pub readable: Result<(), String>,
    /// When `settings.corrupted.bak` was written (a damaged file was replaced by the defaults once).
    pub damaged_copy: Option<DateTime<Local>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpaceFacts {
    pub free: u64,
    pub total: u64,
}

/// The mount that holds the root folder, from the mount table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MountFacts {
    pub mount_point: PathBuf,
    pub source: String,
    pub fs_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RootFacts {
    pub root: Option<PathBuf>,
    pub mount: Option<MountFacts>,
}

/// What the running app knows about the display (the command line knows only the window system).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DisplayFacts {
    pub window_system: WindowSystemKind,
    /// Why `RustView` runs natively on Wayland.
    pub wayland_reason: Option<String>,
    /// `None` = not known (the command line has no window).
    pub gl_renderer: Option<String>,
    pub gl_version: Option<String>,
    /// How videos are drawn (mpv's render API and the windowing GL API; `None` = not known yet).
    pub video_path: Option<String>,
    /// Whether the facts come from the running app (otherwise graphics and mpv are not known).
    pub in_app: bool,
    /// The mpv script `ab_repeat.lua` as the players found it (`None`: its support is switched off).
    pub ab_repeat: Option<AbRepeatFacts>,
}

/// What the players (or Settings > Multiview, Check again) found about the mpv script `ab_repeat.lua`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbRepeatFacts {
    /// No video was opened yet.
    Unknown,
    /// Running with its interface (its version).
    Found(String),
    /// Running, but `RustView` cannot work with it (why).
    Incompatible(String),
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryFacts {
    pub budget_mb: u64,
    pub automatic: bool,
    /// The plan has warnings (shown on Settings > Performance & Cache only).
    pub has_warnings: bool,
}

/// Everything the health checks look at (`None` = not collected).
#[derive(Debug, Clone, Default)]
pub struct HealthFacts {
    pub library: Option<Result<LibraryFacts, String>>,
    pub audit: Option<Result<AuditFacts, String>>,
    pub thumbnails: Option<Result<ThumbnailFacts, String>>,
    pub settings_file: Option<SettingsFileFacts>,
    pub space: Option<Result<SpaceFacts, String>>,
    pub root: Option<RootFacts>,
    pub display: DisplayFacts,
    pub memory: Option<MemoryFacts>,
}

fn health(id: &'static str, status: Status, title: impl Into<String>, detail: impl Into<String>) -> Check {
    Check::new(id, Group::Health, status, title, detail)
}

fn count(value: u64) -> String {
    format_count(value.saturating_as::<usize>())
}

/// The health checks, in the order of the page.
pub fn evaluate(facts: &HealthFacts) -> Vec<Check> {
    let mut checks = Vec::new();
    if let Some(library) = &facts.library {
        checks.push(library_check(library));
    }
    if let Some(audit) = &facts.audit {
        checks.push(audit_check(audit));
    }
    if let Some(thumbnails) = &facts.thumbnails {
        checks.push(match thumbnails {
            Ok(thumbnails) => thumbnail_check(*thumbnails),
            Err(error) => health(
                "thumbnails",
                Status::Problem,
                "The thumbnails cannot be read",
                error.clone(),
            ),
        });
    }
    if let Some(settings) = &facts.settings_file {
        checks.push(settings_check(settings));
    }
    if let Some(space) = &facts.space {
        checks.push(space_check(space));
    }
    if let Some(root) = &facts.root {
        checks.push(root_check(root));
    }
    checks.extend(in_memory(&facts.display, facts.memory));
    checks
}

/// The checks made from what the app keeps in memory (shown as soon as the page opens, without a click).
pub fn in_memory(display: &DisplayFacts, memory: Option<MemoryFacts>) -> Vec<Check> {
    let mut checks = vec![window_system_check(display)];
    if display.in_app {
        checks.push(graphics_check(display));
        checks.push(match &display.video_path {
            Some(path) => health(
                "videos",
                Status::Ok,
                format!("Videos draw with {path}"),
                "into textures of the window that shows them",
            ),
            None => health("videos", Status::Info, "Videos", "the main window has not drawn yet"),
        });
    }
    if display.in_app
        && let Some(ab_repeat) = &display.ab_repeat
    {
        checks.push(ab_repeat_check(ab_repeat));
    }
    if let Some(memory) = memory {
        checks.push(memory_check(memory));
    }
    checks
}

fn ab_repeat_check(facts: &AbRepeatFacts) -> Check {
    const ID: &str = "ab_repeat";
    match facts {
        AbRepeatFacts::Found(version) => health(
            ID,
            Status::Ok,
            format!("mpv script ab_repeat.lua {version}"),
            "A-B repeats in the control bar of videos",
        ),
        AbRepeatFacts::Incompatible(reason) => health(
            ID,
            Status::Warning,
            "mpv script ab_repeat.lua is incompatible",
            format!(
                "{reason} Replace the script in mpv's scripts folder (version 2.0 or newer, api=yes) and click Check again on Settings > Multiview."
            ),
        ),
        AbRepeatFacts::NotFound => health(
            ID,
            Status::Info,
            "mpv script ab_repeat.lua",
            "not loaded by mpv: no A-B repeats in the control bar",
        ),
        AbRepeatFacts::Unknown => health(ID, Status::Info, "mpv script ab_repeat.lua", "no video was opened yet"),
    }
}

fn library_check(library: &Result<LibraryFacts, String>) -> Check {
    match library {
        Ok(lib) => {
            let size = lib
                .file_bytes
                .map(|bytes| format!("{}, ", format_bytes_metric(bytes)))
                .unwrap_or_default();
            health(
                "library",
                Status::Ok,
                "Library database",
                format!(
                    "{size}{} media, {} folders, {} tags",
                    count(lib.media),
                    count(lib.folders),
                    count(lib.tags)
                ),
            )
        }
        Err(error) => health("library", Status::Problem, "The library cannot be read", error.clone()),
    }
}

fn audit_check(audit: &Result<AuditFacts, String>) -> Check {
    match audit {
        Ok(audit) if audit.problems.is_empty() => health(
            "folder_tree",
            Status::Ok,
            "Folder tree",
            format!(
                "consistent ({} folders checked in {} ms)",
                format_count(audit.folders),
                audit.elapsed_ms
            ),
        ),
        Ok(audit) => {
            let total: usize = audit.problems.iter().map(|(n, _)| n).sum();
            let list: Vec<String> = audit
                .problems
                .iter()
                .map(|(n, what)| format!("{} {what}", format_count(*n)))
                .collect();
            health(
                "folder_tree",
                Status::Warning,
                format!("Folder tree: {} records out of place", format_count(total)),
                format!("{}; Refresh Tree (Full) repairs them", list.join(", ")),
            )
        }
        Err(error) => health(
            "folder_tree",
            Status::Problem,
            "The folder tree cannot be checked",
            error.clone(),
        ),
    }
}

fn thumbnail_check(thumbs: ThumbnailFacts) -> Check {
    let without = if thumbs.media_without > 0 {
        format!(", {} media without one", count(thumbs.media_without))
    } else {
        String::new()
    };
    health(
        "thumbnails",
        Status::Ok,
        "Thumbnails",
        format!(
            "{}, {}{without}",
            format_count(thumbs.thumbnails),
            format_bytes_metric(thumbs.bytes)
        ),
    )
}

fn settings_check(settings: &SettingsFileFacts) -> Check {
    if let Err(error) = &settings.readable {
        return health(
            "settings_file",
            Status::Problem,
            "The settings file cannot be read",
            format!("{error} (RustView keeps the settings it loaded; Save writes a valid file)"),
        );
    }
    match settings.damaged_copy {
        Some(when) => health(
            "settings_file",
            Status::Warning,
            format!("The settings file was damaged on {}", when.format("%Y-%m-%d %H:%M")),
            "RustView started with the default settings then; the damaged file is kept as settings.corrupted.bak",
        ),
        None => health("settings_file", Status::Ok, "Settings file", "valid"),
    }
}

fn space_check(space: &Result<SpaceFacts, String>) -> Check {
    match space {
        Ok(space) => {
            let detail = format!(
                "{} free of {} where the library and the thumbnails are",
                format_bytes_metric(space.free),
                format_bytes_metric(space.total)
            );
            if space.free < FREE_SPACE_PROBLEM {
                health(
                    "free_space",
                    Status::Problem,
                    "The drive of the library is full",
                    detail,
                )
            } else if space.free < FREE_SPACE_WARNING {
                health(
                    "free_space",
                    Status::Warning,
                    "Little free space for the library",
                    detail,
                )
            } else {
                health("free_space", Status::Ok, "Free space", detail)
            }
        }
        Err(error) => health(
            "free_space",
            Status::Info,
            "Free space",
            format!("could not be read: {error}"),
        ),
    }
}

/// Whether `root` lies under a usual mount folder while its mount is not there (it is on the system disk).
fn seems_unmounted(root: &Path, mount_point: &Path) -> bool {
    MOUNT_BASES
        .iter()
        .any(|base| root.starts_with(base) && !mount_point.starts_with(base))
}

fn root_check(facts: &RootFacts) -> Check {
    let Some(root) = &facts.root else {
        return health(
            "root_drive",
            Status::Info,
            "No root folder chosen",
            "choose one in Settings > General",
        );
    };
    let hidden_root = LogObscurer::path(root).render(true);
    let Some(mount) = &facts.mount else {
        return health(
            "root_drive",
            Status::Info,
            "Root folder",
            "its drive is not in the mount table",
        );
    };
    if seems_unmounted(root, &mount.mount_point) {
        let detail = |root_text: &str| format!("{root_text} lies on the system disk; mount the drive, then refresh");
        return health(
            "root_drive",
            Status::Warning,
            "The drive of the root folder seems not to be mounted",
            detail(&root.display().to_string()),
        )
        .with_private_detail(detail(&hidden_root));
    }
    let detail = |mount_text: &str| format!("on {} ({}), mounted at {mount_text}", mount.source, mount.fs_type);
    health(
        "root_drive",
        Status::Ok,
        "Drive of the root folder",
        detail(&mount.mount_point.display().to_string()),
    )
    .with_private_detail(detail(&LogObscurer::path(&mount.mount_point).render(true)))
}

fn window_system_check(display: &DisplayFacts) -> Check {
    let reason = display
        .wayland_reason
        .as_ref()
        .map(|reason| format!(" ({reason})"))
        .unwrap_or_default();
    match display.window_system {
        WindowSystemKind::X11Session => health("window_system", Status::Ok, "X11 session", "every feature works"),
        WindowSystemKind::XWayland => health(
            "window_system",
            Status::Ok,
            "XWayland",
            "Wayland session; RustView runs as an X11 program, every feature works",
        ),
        WindowSystemKind::Wayland => health(
            "window_system",
            Status::Warning,
            "Native Wayland",
            format!("videos cannot be shown and the features of X11 are missing{reason}"),
        ),
        WindowSystemKind::NoDisplay => health(
            "window_system",
            Status::Problem,
            "No display",
            "neither X11 nor Wayland answers",
        ),
    }
}

/// The GL version as the driver says it, with "OpenGL" in front unless it starts with it ("OpenGL ES 3.2 Mesa ...").
pub fn gl_version_text(version: &str) -> String {
    if version.starts_with("OpenGL") {
        version.to_owned()
    } else {
        format!("OpenGL {version}")
    }
}

/// Software OpenGL (Mesa's llvmpipe, softpipe, swrast): no graphics acceleration.
pub fn is_software_renderer(renderer: &str) -> bool {
    let renderer = renderer.to_ascii_lowercase();
    ["llvmpipe", "softpipe", "swrast", "software rasterizer"]
        .iter()
        .any(|name| renderer.contains(name))
}

fn graphics_check(display: &DisplayFacts) -> Check {
    let Some(renderer) = &display.gl_renderer else {
        return health("graphics", Status::Info, "Graphics", "not known yet");
    };
    let version = display.gl_version.as_deref().map(gl_version_text).unwrap_or_default();
    if is_software_renderer(renderer) {
        let short = renderer.split_whitespace().next().unwrap_or(renderer);
        health(
            "graphics",
            Status::Warning,
            format!("Software rendering ({short})"),
            "no graphics acceleration: videos can stay black, scrolling is slower",
        )
    } else {
        health("graphics", Status::Ok, renderer.clone(), version)
    }
}

fn memory_check(memory: MemoryFacts) -> Check {
    let how = if memory.automatic { "automatic" } else { "set by you" };
    let budget = format!("{} MB ({how})", count(memory.budget_mb));
    if memory.has_warnings {
        health(
            "memory",
            Status::Warning,
            "RAM budget",
            format!("{budget}: see Performance & Cache"),
        )
        .with_settings_page("performance")
    } else {
        health("memory", Status::Ok, "RAM budget", budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::CheckReport;

    fn display() -> DisplayFacts {
        DisplayFacts {
            window_system: WindowSystemKind::X11Session,
            gl_renderer: Some("AMD Radeon RX 6600 (radeonsi)".into()),
            gl_version: Some("4.6".into()),
            video_path: Some("mpv's render API, GLX".into()),
            in_app: true,
            ..DisplayFacts::default()
        }
    }

    fn healthy() -> HealthFacts {
        HealthFacts {
            library: Some(Ok(LibraryFacts {
                file_bytes: Some(1 << 30),
                media: 48_210,
                folders: 3_105,
                tags: 212,
            })),
            audit: Some(Ok(AuditFacts {
                folders: 3_105,
                elapsed_ms: 40,
                problems: Vec::new(),
            })),
            thumbnails: Some(Ok(ThumbnailFacts {
                thumbnails: 47_980,
                bytes: 2 << 30,
                media_without: 230,
            })),
            settings_file: Some(SettingsFileFacts {
                readable: Ok(()),
                damaged_copy: None,
            }),
            space: Some(Ok(SpaceFacts {
                free: 86 << 30,
                total: 100 << 30,
            })),
            root: Some(RootFacts {
                root: Some("/run/media/me/Photos/2024".into()),
                mount: Some(MountFacts {
                    mount_point: "/run/media/me/Photos".into(),
                    source: "/dev/sdb1".into(),
                    fs_type: "ext4".into(),
                }),
            }),
            display: display(),
            memory: Some(MemoryFacts {
                budget_mb: 4096,
                automatic: true,
                has_warnings: false,
            }),
        }
    }

    fn find(checks: &[Check], id: &str) -> Check {
        checks
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("no check {id}"))
    }

    #[test]
    fn a_healthy_setup_needs_nothing() {
        let checks = evaluate(&healthy());
        let report = CheckReport::new(checks.clone());
        assert!(
            report.attention(Group::Health).is_empty(),
            "{:?}",
            report.attention(Group::Health)
        );
        assert_eq!(
            find(&checks, "library").detail,
            "1.00 GB, 48,210 media, 3,105 folders, 212 tags"
        );
        assert!(find(&checks, "thumbnails").detail.ends_with("230 media without one"));
    }

    #[test]
    fn a_root_folder_on_the_system_disk_was_not_mounted() {
        let mut facts = healthy();
        facts.root.as_mut().unwrap().mount = Some(MountFacts {
            mount_point: "/".into(),
            source: "/dev/sda2".into(),
            fs_type: "btrfs".into(),
        });
        let check = find(&evaluate(&facts), "root_drive");
        assert_eq!(check.status, Status::Warning);
        assert!(check.detail.contains("/run/media/me/Photos/2024"));
        assert!(!check.detail_for(true).contains("Photos"), "{}", check.detail_for(true));

        facts.root.as_mut().unwrap().root = Some("/home/me/Pictures".into());
        assert_eq!(find(&evaluate(&facts), "root_drive").status, Status::Ok);
        facts.root = Some(RootFacts::default());
        assert_eq!(find(&evaluate(&facts), "root_drive").status, Status::Info);
    }

    #[test]
    fn the_audit_names_what_is_out_of_place() {
        let mut facts = healthy();
        facts.audit = Some(Ok(AuditFacts {
            folders: 10,
            elapsed_ms: 1,
            problems: vec![(2, "media without a folder"), (1, "stale path index entries")],
        }));
        let check = find(&evaluate(&facts), "folder_tree");
        assert_eq!(check.status, Status::Warning);
        assert_eq!(check.title, "Folder tree: 3 records out of place");
        assert!(
            check
                .detail
                .starts_with("2 media without a folder, 1 stale path index entries;")
        );
    }

    #[test]
    fn free_space_thresholds() {
        let mut facts = healthy();
        let with_free = |facts: &mut HealthFacts, free: u64| {
            facts.space = Some(Ok(SpaceFacts { free, total: 10 << 30 }));
            find(&evaluate(facts), "free_space").status
        };
        assert_eq!(with_free(&mut facts, 900 << 20), Status::Warning);
        assert_eq!(with_free(&mut facts, 100 << 20), Status::Problem);
        assert_eq!(with_free(&mut facts, 5 << 30), Status::Ok);
    }

    #[test]
    fn software_rendering_and_window_systems() {
        let mut display = display();
        display.gl_renderer = Some("llvmpipe (LLVM 21.1.0, 256 bits)".into());
        let checks = in_memory(&display, None);
        assert_eq!(find(&checks, "graphics").title, "Software rendering (llvmpipe)");
        assert_eq!(find(&checks, "graphics").status, Status::Warning);

        display.window_system = WindowSystemKind::Wayland;
        display.wayland_reason = Some("RUSTVIEW_WINDOW_SYSTEM=wayland".into());
        let check = find(&in_memory(&display, None), "window_system");
        assert_eq!(check.status, Status::Warning);
        assert!(check.detail.ends_with("(RUSTVIEW_WINDOW_SYSTEM=wayland)"));

        display.in_app = false;
        let ids: Vec<&str> = in_memory(&display, None).iter().map(|c| c.id).collect();
        assert_eq!(ids, ["window_system"], "the command line does not know the graphics");
    }

    #[test]
    fn the_gl_version_is_named_once() {
        assert_eq!(
            gl_version_text("4.6 (Core Profile) Mesa 26.2"),
            "OpenGL 4.6 (Core Profile) Mesa 26.2"
        );
        assert_eq!(gl_version_text("OpenGL ES 3.2 Mesa 26.2"), "OpenGL ES 3.2 Mesa 26.2");
    }

    #[test]
    fn the_ab_repeat_script_is_named_and_an_incompatible_one_warns() {
        let mut display = display();
        assert!(
            in_memory(&display, None).iter().all(|c| c.id != "ab_repeat"),
            "support switched off"
        );
        display.ab_repeat = Some(AbRepeatFacts::Incompatible("No interface.".into()));
        let check = find(&in_memory(&display, None), "ab_repeat");
        assert_eq!(check.status, Status::Warning);
        assert!(check.detail.contains("Check again"));
        display.ab_repeat = Some(AbRepeatFacts::Found("2.0".into()));
        assert_eq!(find(&in_memory(&display, None), "ab_repeat").status, Status::Ok);
    }

    #[test]
    fn settings_file_and_memory() {
        let mut facts = healthy();
        facts.settings_file = Some(SettingsFileFacts {
            readable: Ok(()),
            damaged_copy: Some(Local::now()),
        });
        facts.memory = Some(MemoryFacts {
            budget_mb: 512,
            automatic: false,
            has_warnings: true,
        });
        let checks = evaluate(&facts);
        assert_eq!(find(&checks, "settings_file").status, Status::Warning);
        let memory = find(&checks, "memory");
        assert_eq!(
            (memory.status, memory.settings_page),
            (Status::Warning, Some("performance"))
        );
        facts.settings_file = Some(SettingsFileFacts {
            readable: Err("expected value at line 1".into()),
            damaged_copy: None,
        });
        assert_eq!(find(&evaluate(&facts), "settings_file").status, Status::Problem);
    }
}
