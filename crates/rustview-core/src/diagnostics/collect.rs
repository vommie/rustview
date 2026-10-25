//! Collects the facts of the checks from the system. Runs only when the user asked for a check (the check button, the
//! diagnostic report, `rustview check`), on a worker thread: it looks up programs in `PATH`, asks the session bus, reads the
//! size of the database file, the settings file and the free space, and reads numbers from the database. It never walks the
//! media and never touches the drive of the root folder (that one is judged from the mount table).

use super::dependencies::{DependencyFacts, FfmpegFacts, MpvFacts, PROGRAMS, PortalFacts, WindowSystemKind, X11Facts};
use super::health::{AuditFacts, LibraryFacts, MountFacts, RootFacts, SettingsFileFacts, SpaceFacts, ThumbnailFacts};
use crate::library::{FolderDiagnostics, Library};
use crate::settings::AppSettings;
use crate::thumbnails::ThumbnailStorageStats;
use crate::util::cast::SaturatingAs;
use chrono::{DateTime, Local};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_KDE: &str = "org.freedesktop.impl.portal.desktop.kde";
const PERF_EVENT_PARANOID: &str = "/proc/sys/kernel/perf_event_paranoid";
const SETTINGS_FILE: &str = "settings.json";
const DAMAGED_SETTINGS_FILE: &str = "settings.corrupted.bak";
const DATABASE_FILE: &str = "data.mdb";

/// Everything the dependency checks need; the window system and the X server's answers come from the caller (the app or
/// the command line asks the X display itself).
pub fn dependencies(window_system: WindowSystemKind, x11: Option<X11Facts>) -> DependencyFacts {
    DependencyFacts {
        window_system,
        kde: is_kde(),
        programs: programs(),
        mpv: Some(mpv()),
        ffmpeg: Some(ffmpeg()),
        portal: portal(),
        x11,
        perf_event_paranoid: perf_event_paranoid(),
    }
}

/// The programs of [`PROGRAMS`] found in `PATH`.
pub fn programs() -> BTreeSet<&'static str> {
    PROGRAMS
        .into_iter()
        .filter(|program| which::which(program).is_ok())
        .collect()
}

/// The desktop is KDE Plasma.
pub fn is_kde() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP")
        .is_ok_and(|desktop| desktop.split(':').any(|part| part.eq_ignore_ascii_case("KDE")))
}

/// Whether the desktop portal and KDE's implementation of it run or can be started.
pub fn portal() -> PortalFacts {
    let Ok(connection) = zbus::blocking::Connection::session() else {
        return PortalFacts::default();
    };
    let Ok(bus) = zbus::blocking::fdo::DBusProxy::new(&connection) else {
        return PortalFacts {
            session_bus: true,
            ..PortalFacts::default()
        };
    };
    let activatable: Vec<String> = bus
        .list_activatable_names()
        .map(|names| names.into_iter().map(|name| name.to_string()).collect())
        .unwrap_or_default();
    let available = |name: &str| {
        activatable.iter().any(|n| n == name)
            || zbus::names::BusName::try_from(name)
                .ok()
                .and_then(|bus_name| bus.name_has_owner(bus_name).ok())
                .unwrap_or(false)
    };
    PortalFacts {
        session_bus: true,
        desktop: available(PORTAL),
        kde_backend: available(PORTAL_KDE),
    }
}

pub fn perf_event_paranoid() -> Option<i32> {
    std::fs::read_to_string(PERF_EVENT_PARANOID)
        .ok()
        .and_then(|text| text.trim().parse().ok())
}

/// The `FFmpeg` in use and the one `RustView` was built with.
pub fn ffmpeg() -> FfmpegFacts {
    // SAFETY: `av_version_info` returns a pointer to a static, NUL-terminated string of the library (or null).
    let release = unsafe {
        let info = ffmpeg_next::ffi::av_version_info();
        if info.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(info).to_string_lossy().into_owned()
        }
    };
    let running = ffmpeg_next::codec::version();
    let built = |value: std::os::raw::c_int| u32::try_from(value).unwrap_or(0);
    FfmpegFacts {
        release,
        avcodec_running: (running >> 16, (running >> 8) & 0xff),
        avcodec_built: (
            built(ffmpeg_next::ffi::LIBAVCODEC_VERSION_MAJOR),
            built(ffmpeg_next::ffi::LIBAVCODEC_VERSION_MINOR),
        ),
    }
}

/// Starts a libmpv handle without a window or sound and asks it for its version.
pub fn mpv() -> Result<MpvFacts, String> {
    let mpv = libmpv2::Mpv::with_initializer(|init| {
        init.set_property("config", false)?;
        init.set_property("vo", "null")?;
        init.set_property("ao", "null")?;
        Ok(())
    })
    .map_err(|error| error.to_string())?;
    let read = |name: &str| mpv.get_property::<String>(name).unwrap_or_default();
    Ok(MpvFacts {
        version: read("mpv-version"),
        ffmpeg_version: read("ffmpeg-version"),
    })
}

/// The numbers of the library (database only) and the size of its file in `db_dir` (the LMDB folder).
pub fn library(library: &Library, db_dir: &Path) -> Result<LibraryFacts, String> {
    let db = library.raw_db();
    let rtxn = db.env.read_txn().map_err(|e| e.to_string())?;
    let len = |result: heed::Result<u64>| result.map_err(|e| e.to_string());
    Ok(LibraryFacts {
        file_bytes: std::fs::metadata(db_dir.join(DATABASE_FILE)).ok().map(|m| m.len()),
        media: len(db.media.len(&rtxn))?,
        folders: len(db.folders.len(&rtxn))?,
        tags: len(db.tags.len(&rtxn))?,
    })
}

/// Runs the folder audit (reads the database) and lists what is out of place.
pub fn audit(library: &Library) -> Result<AuditFacts, String> {
    let report = FolderDiagnostics::run_audit(library.raw_db()).map_err(|e| e.to_string())?;
    let problems: Vec<(usize, &'static str)> = [
        (report.media.unresolvable_media, "media without a folder"),
        (report.dangling_parent_links, "folders whose parent is missing"),
        (report.duplicate_path_records, "folders recorded twice"),
        (report.parent_index_mismatches, "wrong entries in the folder index"),
        (report.stale_path_index_entries, "stale path index entries"),
        (report.unindexed_folders, "folders missing in the path index"),
        (report.undecodable_folders, "unreadable folder records"),
        (report.media.undecodable_media, "unreadable media records"),
    ]
    .into_iter()
    .filter(|(count, _)| *count > 0)
    .collect();
    Ok(AuditFacts {
        folders: report.total_folders,
        elapsed_ms: report.elapsed_ms.saturating_as::<u64>(),
        problems,
    })
}

/// The thumbnail numbers from the store's statistics and the number of media.
pub fn thumbnails(stats: &ThumbnailStorageStats, media: u64) -> ThumbnailFacts {
    ThumbnailFacts {
        thumbnails: stats.lmdb_thumbnails_count,
        bytes: stats.total_cache_bytes,
        media_without: media.saturating_sub(stats.lmdb_thumbnails_count.saturating_as::<u64>()),
    }
}

/// Reads `settings.json` again and looks for the copy of a damaged one.
pub fn settings_file(config_dir: &Path) -> SettingsFileFacts {
    let readable = std::fs::read_to_string(config_dir.join(SETTINGS_FILE))
        .map_err(|e| e.to_string())
        .and_then(|text| {
            serde_json::from_str::<AppSettings>(&text)
                .map(|_| ())
                .map_err(|e| e.to_string())
        });
    let damaged_copy = std::fs::metadata(config_dir.join(DAMAGED_SETTINGS_FILE))
        .and_then(|meta| meta.modified())
        .ok()
        .map(DateTime::<Local>::from);
    SettingsFileFacts { readable, damaged_copy }
}

/// Free and total space of the file system that holds `dir`.
pub fn space(dir: &Path) -> Result<SpaceFacts, String> {
    let stat = rustix::fs::statvfs(dir).map_err(|e| e.to_string())?;
    Ok(SpaceFacts {
        free: stat.f_bavail.saturating_mul(stat.f_frsize),
        total: stat.f_blocks.saturating_mul(stat.f_frsize),
    })
}

/// The mount that holds `root`: the mount point with the longest common start (from the mount table, the drive is not
/// touched).
pub fn root(root: Option<&Path>) -> RootFacts {
    let Some(root) = root else {
        return RootFacts::default();
    };
    let mounts: Vec<(PathBuf, String, String)> = proc_mounts::MountIter::new_from_file("/proc/self/mounts")
        .map(|iter| {
            iter.filter_map(Result::ok)
                .map(|m| (m.dest, m.source.to_string_lossy().into_owned(), m.fstype))
                .collect()
        })
        .unwrap_or_default();
    RootFacts {
        root: Some(root.to_path_buf()),
        mount: mount_of(root, &mounts),
    }
}

/// The deepest mount point that contains `path` (the last one mounted wins when several are stacked on one point).
fn mount_of(path: &Path, mounts: &[(PathBuf, String, String)]) -> Option<MountFacts> {
    mounts
        .iter()
        .enumerate()
        .filter(|(_, (point, _, _))| path.starts_with(point))
        .max_by_key(|(index, (point, _, _))| (point.components().count(), *index))
        .map(|(_, (point, source, fs_type))| MountFacts {
            mount_point: point.clone(),
            source: source.clone(),
            fs_type: fs_type.clone(),
        })
}

#[cfg(test)]
mod tests {
    use super::{mount_of, settings_file, space};
    use std::path::{Path, PathBuf};

    #[test]
    fn the_deepest_and_newest_mount_holds_the_path() {
        let mount = |point: &str, source: &str| (PathBuf::from(point), source.to_owned(), "ext4".to_owned());
        let mounts = vec![
            mount("/", "/dev/sda2"),
            mount("/run/media/me/Photos", "/dev/sdb1"),
            mount("/run/media/me/Photos", "/dev/sdc1"),
            mount("/run/media/me/PhotosOld", "/dev/sdd1"),
        ];
        let found = mount_of(Path::new("/run/media/me/Photos/2024"), &mounts).unwrap();
        assert_eq!(found.source, "/dev/sdc1");
        assert_eq!(mount_of(Path::new("/home/me"), &mounts).unwrap().source, "/dev/sda2");
        assert_eq!(
            mount_of(Path::new("/run/media/me/Photos"), &mounts[..1])
                .unwrap()
                .source,
            "/dev/sda2"
        );
    }

    #[test]
    fn the_settings_file_and_its_damaged_copy() {
        let dir = tempfile::tempdir().unwrap();
        assert!(settings_file(dir.path()).readable.is_err());
        std::fs::write(dir.path().join("settings.json"), "{}").unwrap();
        let facts = settings_file(dir.path());
        assert_eq!(facts.readable, Ok(()));
        assert_eq!(facts.damaged_copy, None);
        std::fs::write(dir.path().join("settings.json"), "{ broken").unwrap();
        std::fs::write(dir.path().join("settings.corrupted.bak"), "x").unwrap();
        let facts = settings_file(dir.path());
        assert!(facts.readable.is_err() && facts.damaged_copy.is_some());
    }

    #[test]
    fn free_space_of_a_real_folder() {
        let dir = tempfile::tempdir().unwrap();
        let space = space(dir.path()).unwrap();
        assert!(space.total > 0 && space.free <= space.total);
    }
}
