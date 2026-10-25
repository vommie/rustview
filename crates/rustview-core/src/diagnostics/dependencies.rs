//! The programs and libraries `RustView` uses while it runs, and what goes missing without them. Pure: made from
//! [`DependencyFacts`].

use super::{Check, Fix, Group, Status};
use std::collections::BTreeSet;

/// The window system `RustView` runs on (the app's `window_system::Choice`, without its details).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowSystemKind {
    /// An X11 session.
    #[default]
    X11Session,
    /// A Wayland session; `RustView` runs as an X11 program through `XWayland`.
    XWayland,
    /// Natively on Wayland (forced, or no X display): videos and the X11 features are not available.
    Wayland,
    /// Neither an X11 nor a Wayland display.
    NoDisplay,
}

impl WindowSystemKind {
    /// Whether the windows of `RustView` are X11 windows.
    pub fn is_x11(self) -> bool {
        matches!(self, Self::X11Session | Self::XWayland)
    }
}

/// What libmpv says about itself (a short-lived handle without a window).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MpvFacts {
    /// `mpv-version`, e.g. "mpv 0.41.0".
    pub version: String,
    /// `ffmpeg-version` of the `FFmpeg` libmpv uses.
    pub ffmpeg_version: String,
}

/// The `FFmpeg` libraries `RustView` itself decodes with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegFacts {
    /// `av_version_info()`, e.g. "n9.0.1".
    pub release: String,
    /// libavcodec in use and the one `RustView` was built with, as (major, minor).
    pub avcodec_running: (u32, u32),
    pub avcodec_built: (u32, u32),
}

/// The desktop portal on the session bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PortalFacts {
    pub session_bus: bool,
    /// `org.freedesktop.portal.Desktop` is running or can be started.
    pub desktop: bool,
    /// KDE's implementation of the portal (`org.freedesktop.impl.portal.desktop.kde`).
    pub kde_backend: bool,
}

/// What the X server offers (`None` in the facts without an X display).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X11Facts {
    pub shape: bool,
    pub xfixes: bool,
    /// A compositing manager owns `_NET_WM_CM_S<screen>`.
    pub compositor: bool,
}

/// Everything the dependency checks look at.
#[derive(Debug, Clone, Default)]
pub struct DependencyFacts {
    pub window_system: WindowSystemKind,
    /// The desktop is KDE Plasma (`XDG_CURRENT_DESKTOP`).
    pub kde: bool,
    /// The programs found in `PATH` (of [`PROGRAMS`]).
    pub programs: BTreeSet<&'static str>,
    /// `Err` = libmpv could not be started.
    pub mpv: Option<Result<MpvFacts, String>>,
    pub ffmpeg: Option<FfmpegFacts>,
    pub portal: PortalFacts,
    pub x11: Option<X11Facts>,
    /// `/proc/sys/kernel/perf_event_paranoid`.
    pub perf_event_paranoid: Option<i32>,
}

/// Every program the checks look for in `PATH`.
pub const PROGRAMS: [&str; 10] = [
    "xclip",
    "wl-copy",
    "kscreen-doctor",
    "xrandr",
    "gio",
    "xdg-open",
    "dolphin",
    "nautilus",
    "nemo",
    "samply",
];

const PERF_SYSCTL: &str = "sudo sysctl kernel.perf_event_paranoid=1";
const PERF_PERMANENT: &str = "echo 'kernel.perf_event_paranoid = 1' | sudo tee /etc/sysctl.d/99-perf.conf";

fn dep(id: &'static str, status: Status, title: impl Into<String>, detail: impl Into<String>) -> Check {
    Check::new(id, Group::Dependencies, status, title, detail)
}

/// The dependency checks, in the order of the page (the developer tools last).
pub fn evaluate(facts: &DependencyFacts) -> Vec<Check> {
    let has = |program: &str| facts.programs.contains(program);
    let mut checks = vec![mpv(facts), ffmpeg(facts)];
    checks.extend(clipboard(facts.window_system, &has));
    checks.push(monitors(facts, &has));
    checks.push(portal(facts));
    checks.push(if has("gio") {
        dep("gio", Status::Ok, "gio", "lists the applications of Open With")
    } else {
        dep(
            "gio",
            Status::Warning,
            "gio is missing",
            "Open With lists no applications",
        )
        .with_fix(Fix::packages(&["glib2"]))
    });
    checks.push(if has("xdg-open") {
        dep(
            "xdg_open",
            Status::Ok,
            "xdg-open",
            "opens files and folders with the default application",
        )
    } else {
        dep(
            "xdg_open",
            Status::Warning,
            "xdg-open is missing",
            "files and folders cannot be opened with the default application",
        )
        .with_fix(Fix::packages(&["xdg-utils"]))
    });
    checks.push(file_manager(facts.kde, &has));
    if let Some(x11) = facts.x11.filter(|_| facts.window_system.is_x11()) {
        checks.extend(x11_features(x11));
    }
    checks.extend(developer(facts, &has));
    checks
}

fn mpv(facts: &DependencyFacts) -> Check {
    match &facts.mpv {
        Some(Ok(mpv)) => dep(
            "libmpv",
            Status::Ok,
            mpv.version.clone(),
            format!(
                "plays the videos (with FFmpeg {})",
                mpv.ffmpeg_version.trim_start_matches('n')
            ),
        ),
        Some(Err(error)) => dep(
            "libmpv",
            Status::Problem,
            "libmpv does not start",
            format!("videos cannot be played: {error}"),
        )
        .with_fix(Fix::packages(&["mpv"])),
        None => dep("libmpv", Status::Info, "libmpv", "not checked"),
    }
}

fn ffmpeg(facts: &DependencyFacts) -> Check {
    let Some(ffmpeg) = &facts.ffmpeg else {
        return dep("ffmpeg", Status::Info, "FFmpeg", "not checked");
    };
    let release = ffmpeg.release.trim_start_matches('n');
    let (running, built) = (ffmpeg.avcodec_running, ffmpeg.avcodec_built);
    let detail = format!(
        "thumbnails and details of videos (libavcodec {}.{}, built with {}.{})",
        running.0, running.1, built.0, built.1
    );
    if running < built {
        dep(
            "ffmpeg",
            Status::Warning,
            format!("FFmpeg {release} is older than the one RustView was built with"),
            detail,
        )
        .with_fix(Fix::packages(&["ffmpeg"]))
    } else {
        dep("ffmpeg", Status::Ok, format!("FFmpeg {release}"), detail)
    }
}

fn clipboard(window_system: WindowSystemKind, has: &impl Fn(&str) -> bool) -> Option<Check> {
    const WHAT: &str = "copies pictures for other programs";
    const WITHOUT: &str =
        "Pictures copied with Ctrl+C may not reach Krita or GIMP (RustView's built-in fallback is used)";
    let check = match window_system {
        WindowSystemKind::NoDisplay => return None,
        WindowSystemKind::Wayland if has("wl-copy") => dep("clipboard", Status::Ok, "wl-copy", WHAT),
        WindowSystemKind::Wayland => {
            dep("clipboard", Status::Warning, "wl-copy is missing", WITHOUT).with_fix(Fix::packages(&["wl-clipboard"]))
        }
        _ if has("xclip") => dep("clipboard", Status::Ok, "xclip", WHAT),
        WindowSystemKind::XWayland if has("wl-copy") => {
            dep("clipboard", Status::Ok, "wl-copy (xclip is missing)", WHAT)
        }
        _ => dep("clipboard", Status::Warning, "xclip is missing", WITHOUT).with_fix(Fix::packages(&["xclip"])),
    };
    Some(check)
}

fn monitors(facts: &DependencyFacts, has: &impl Fn(&str) -> bool) -> Check {
    const WHAT: &str = "finds the monitors for the fullscreen";
    let x11 = facts.window_system.is_x11();
    if has("kscreen-doctor") {
        return dep("monitors", Status::Ok, "kscreen-doctor", WHAT);
    }
    if facts.kde {
        let detail = if x11 && has("xrandr") {
            "monitors are found with xrandr instead (without their names)"
        } else {
            "the fullscreen cannot be put on a chosen monitor"
        };
        return dep("monitors", Status::Warning, "kscreen-doctor is missing", detail)
            .with_fix(Fix::packages(&["libkscreen"]));
    }
    if x11 && has("xrandr") {
        return dep("monitors", Status::Ok, "xrandr", WHAT);
    }
    dep(
        "monitors",
        Status::Warning,
        "No program to find the monitors",
        "the fullscreen cannot be put on a chosen monitor",
    )
    .with_fix(Fix::packages(&["xorg-xrandr"]))
}

fn portal(facts: &DependencyFacts) -> Check {
    let portal = facts.portal;
    if !portal.session_bus {
        return dep(
            "portal",
            Status::Problem,
            "No D-Bus session bus",
            "file dialogs (root folder, Save As, the report) and opening links do not work",
        );
    }
    if !portal.desktop {
        let packages: &[&'static str] = if facts.kde {
            &["xdg-desktop-portal", "xdg-desktop-portal-kde"]
        } else {
            &["xdg-desktop-portal"]
        };
        return dep(
            "portal",
            Status::Problem,
            "The desktop portal is missing",
            "file dialogs (root folder, Save As, the report) and opening links do not work",
        )
        .with_fix(Fix::packages(packages));
    }
    if facts.kde && !portal.kde_backend {
        return dep(
            "portal",
            Status::Warning,
            "The KDE part of the desktop portal is missing",
            "file dialogs do not look like the ones of KDE",
        )
        .with_fix(Fix::packages(&["xdg-desktop-portal-kde"]));
    }
    let name = if facts.kde {
        "Desktop portal (KDE)"
    } else {
        "Desktop portal"
    };
    dep("portal", Status::Ok, name, "file dialogs and opening links")
}

fn file_manager(kde: bool, has: &impl Fn(&str) -> bool) -> Check {
    const WHAT: &str = "Show in Folder selects the file";
    for (program, name) in [("dolphin", "Dolphin"), ("nautilus", "Nautilus"), ("nemo", "Nemo")] {
        if has(program) {
            return dep("file_manager", Status::Ok, name, WHAT);
        }
    }
    dep(
        "file_manager",
        Status::Warning,
        "No file manager found",
        "Show in Folder opens the folder without selecting the file",
    )
    .with_fix(Fix::packages(if kde { &["dolphin"] } else { &["nautilus"] }))
}

fn x11_features(x11: X11Facts) -> Vec<Check> {
    let extensions = if x11.shape && x11.xfixes {
        dep(
            "x11_extensions",
            Status::Ok,
            "X11 extensions SHAPE and XFixes",
            "menus over videos, hiding the pointer",
        )
    } else {
        let missing: Vec<&str> = [(x11.shape, "SHAPE"), (x11.xfixes, "XFixes")]
            .into_iter()
            .filter(|(present, _)| !present)
            .map(|(_, name)| name)
            .collect();
        dep(
            "x11_extensions",
            Status::Problem,
            format!("The X server lacks {}", missing.join(" and ")),
            "menus and bars over videos and hiding the pointer do not work",
        )
    };
    let compositor = if x11.compositor {
        dep(
            "compositor",
            Status::Ok,
            "Compositor",
            "the bars of the fullscreen are translucent",
        )
    } else {
        dep(
            "compositor",
            Status::Warning,
            "No compositor is running",
            "the bars of the fullscreen are solid; turn compositing on in the system settings of the desktop",
        )
    };
    vec![extensions, compositor]
}

fn developer(facts: &DependencyFacts, has: &impl Fn(&str) -> bool) -> Vec<Check> {
    let samply = if has("samply") {
        Check::new("samply", Group::Developer, Status::Ok, "samply", "records CPU profiles")
    } else {
        Check::new(
            "samply",
            Group::Developer,
            Status::Info,
            "samply is not installed",
            "records CPU profiles",
        )
        .with_fix(Fix::packages(&["samply"]))
    };
    let perf = match facts.perf_event_paranoid {
        Some(value) if value <= 1 => Check::new(
            "perf_event_paranoid",
            Group::Developer,
            Status::Ok,
            format!("perf_event_paranoid = {value}"),
            "samply may record",
        ),
        Some(value) => Check::new(
            "perf_event_paranoid",
            Group::Developer,
            Status::Info,
            format!("perf_event_paranoid = {value}"),
            "samply needs 1 or less (the second command keeps it after a restart)",
        )
        .with_fix(Fix::commands(&[PERF_SYSCTL, PERF_PERMANENT])),
        None => Check::new(
            "perf_event_paranoid",
            Group::Developer,
            Status::Info,
            "perf_event_paranoid",
            "could not be read",
        ),
    };
    vec![samply, perf]
}

#[cfg(test)]
mod tests {
    use super::{DependencyFacts, FfmpegFacts, MpvFacts, PortalFacts, WindowSystemKind, X11Facts, evaluate};
    use crate::diagnostics::{CheckReport, Group, Status};

    fn healthy() -> DependencyFacts {
        DependencyFacts {
            window_system: WindowSystemKind::X11Session,
            kde: true,
            programs: [
                "xclip",
                "kscreen-doctor",
                "xrandr",
                "gio",
                "xdg-open",
                "dolphin",
                "samply",
            ]
            .into(),
            mpv: Some(Ok(MpvFacts {
                version: "mpv 0.41.0".into(),
                ffmpeg_version: "n9.0.1".into(),
            })),
            ffmpeg: Some(FfmpegFacts {
                release: "n9.0.1".into(),
                avcodec_running: (63, 1),
                avcodec_built: (63, 1),
            }),
            portal: PortalFacts {
                session_bus: true,
                desktop: true,
                kde_backend: true,
            },
            x11: Some(X11Facts {
                shape: true,
                xfixes: true,
                compositor: true,
            }),
            perf_event_paranoid: Some(1),
        }
    }

    fn status_of(facts: &DependencyFacts, id: &str) -> Status {
        evaluate(facts)
            .into_iter()
            .find(|c| c.id == id)
            .map_or_else(|| panic!("no check {id}"), |c| c.status)
    }

    #[test]
    fn a_complete_system_passes_every_check() {
        let report = CheckReport::new(evaluate(&healthy()));
        assert!(
            report.attention(Group::Dependencies).is_empty(),
            "{:?}",
            report.attention(Group::Dependencies)
        );
        assert_eq!(report.install_all(Group::Dependencies), None);
        assert_eq!(report.group(Group::Developer).count(), 2);
    }

    #[test]
    fn the_clipboard_tool_depends_on_the_window_system() {
        let mut facts = healthy();
        facts.programs.remove("xclip");
        let report = CheckReport::new(evaluate(&facts));
        assert_eq!(
            report.install_all(Group::Dependencies).as_deref(),
            Some("sudo pacman -S --needed xclip")
        );

        facts.window_system = WindowSystemKind::XWayland;
        facts.programs.insert("wl-copy");
        assert_eq!(status_of(&facts, "clipboard"), Status::Ok);

        facts.window_system = WindowSystemKind::Wayland;
        facts.programs.remove("wl-copy");
        let check = evaluate(&facts).into_iter().find(|c| c.id == "clipboard").unwrap();
        assert_eq!(check.fix.unwrap().packages, ["wl-clipboard"]);

        facts.window_system = WindowSystemKind::NoDisplay;
        assert!(evaluate(&facts).iter().all(|c| c.id != "clipboard"));
    }

    #[test]
    fn a_missing_portal_breaks_the_file_dialogs() {
        let mut facts = healthy();
        facts.portal.desktop = false;
        let report = CheckReport::new(evaluate(&facts));
        assert!(report.has_problems());
        assert_eq!(
            report.install_all(Group::Dependencies).as_deref(),
            Some("sudo pacman -S --needed xdg-desktop-portal xdg-desktop-portal-kde")
        );
        let mut facts = healthy();
        facts.portal.kde_backend = false;
        assert_eq!(status_of(&facts, "portal"), Status::Warning);
        facts.kde = false;
        assert_eq!(status_of(&facts, "portal"), Status::Ok);
    }

    #[test]
    fn monitors_fall_back_to_xrandr_outside_kde() {
        let mut facts = healthy();
        facts.programs.remove("kscreen-doctor");
        assert_eq!(status_of(&facts, "monitors"), Status::Warning);
        facts.kde = false;
        assert_eq!(status_of(&facts, "monitors"), Status::Ok);
        facts.programs.remove("xrandr");
        assert_eq!(status_of(&facts, "monitors"), Status::Warning);
    }

    #[test]
    fn x11_features_are_checked_only_for_x11_windows() {
        let mut facts = healthy();
        facts.x11 = Some(X11Facts {
            shape: false,
            xfixes: true,
            compositor: false,
        });
        assert_eq!(status_of(&facts, "x11_extensions"), Status::Problem);
        assert_eq!(status_of(&facts, "compositor"), Status::Warning);
        facts.window_system = WindowSystemKind::Wayland;
        assert!(evaluate(&facts).iter().all(|c| c.id != "compositor"));
    }

    #[test]
    fn libraries_and_versions() {
        let mut facts = healthy();
        facts.ffmpeg.as_mut().unwrap().avcodec_running = (63, 0);
        assert_eq!(status_of(&facts, "ffmpeg"), Status::Warning);
        facts.mpv = Some(Err("no".into()));
        assert_eq!(status_of(&facts, "libmpv"), Status::Problem);
        let titles: Vec<String> = evaluate(&healthy()).into_iter().map(|c| c.title).collect();
        assert!(titles.contains(&"FFmpeg 9.0.1".to_owned()), "{titles:?}");
    }

    #[test]
    fn developer_tools_are_only_information() {
        let mut facts = healthy();
        facts.programs.remove("samply");
        facts.perf_event_paranoid = Some(2);
        let report = CheckReport::new(evaluate(&facts));
        assert!(report.group(Group::Developer).all(|c| c.status == Status::Info));
        assert!(!report.has_problems());
        let perf = report
            .group(Group::Developer)
            .find(|c| c.id == "perf_event_paranoid")
            .unwrap();
        assert_eq!(perf.fix.as_ref().unwrap().lines().len(), 2);
        assert_eq!(
            report.install_all(Group::Developer).as_deref(),
            Some("sudo pacman -S --needed samply")
        );
    }
}
