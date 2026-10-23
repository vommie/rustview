use super::error::SystemHandlerError;
use freedesktop_desktop_entry::{DesktopEntry, Iter, default_paths, get_languages_from_env};
use rustview_core::settings::LogObscurer;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Metadata describing an installed Freedesktop application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopApp {
    pub id: String,
    pub name: String,
    pub icon_name: String,
    pub icon_path: Option<PathBuf>,
    pub exec: String,
}

/// Centralized Freedesktop desktop integration and application discovery subsystem.
/// Identifies MIME-associated desktop applications, resolves real theme icons,
/// and executes native application chooser dialogs via `DBus` Desktop Portals or KDE kdialog.
pub struct DesktopIntegration;

impl DesktopIntegration {
    /// Queries all associated applications for a given media file path and resolves their icons.
    pub fn get_associated_apps(file_path: &Path) -> Vec<DesktopApp> {
        let t0 = Instant::now();
        let mime_type = super::clipboard::SystemClipboard::mime_type_from_path(file_path);

        tracing::debug!(
            file = %LogObscurer::path(file_path),
            mime = mime_type,
            "[DesktopIntegration] Discovering associated desktop applications..."
        );

        let desktop_ids = Self::query_desktop_ids_for_mime(mime_type);
        let mut apps = Vec::with_capacity(desktop_ids.len());
        let mut seen_names = HashSet::new();

        for desktop_id in desktop_ids {
            if let Some(app) = Self::resolve_desktop_file(&desktop_id)
                && !seen_names.contains(&app.name)
            {
                seen_names.insert(app.name.clone());
                apps.push(app);
            }
        }

        tracing::info!(
            file = %LogObscurer::path(file_path),
            apps_count = apps.len(),
            elapsed_us = t0.elapsed().as_micros(),
            "[DesktopIntegration] Discovered associated applications successfully"
        );

        apps
    }

    /// Launches a specific desktop application with target file paths asynchronously.
    pub fn launch_app<P: AsRef<Path>>(app: &DesktopApp, file_paths: &[P]) -> Result<(), SystemHandlerError> {
        if file_paths.is_empty() {
            return Ok(());
        }

        let t0 = Instant::now();
        let app_name = app.name.clone();
        let exec_template = app.exec.clone();

        let mut canonical_paths = Vec::with_capacity(file_paths.len());
        for p in file_paths {
            let pref = p.as_ref();
            if pref.exists() {
                let cp = match pref.canonicalize() {
                    Ok(c) => c,
                    Err(_) => pref.to_path_buf(),
                };
                canonical_paths.push(cp);
            }
        }

        if canonical_paths.is_empty() {
            return Err(SystemHandlerError::FileNotFound(file_paths[0].as_ref().to_path_buf()));
        }

        tracing::info!(
            app = %app_name,
            files_count = canonical_paths.len(),
            "[DesktopIntegration] Launching application asynchronously..."
        );

        std::thread::Builder::new()
            .name(format!("sys-app-launcher-{}", app.id))
            .spawn(move || {
                let t_spawn = Instant::now();
                let argv = Self::build_argv(&exec_template, &canonical_paths);

                if argv.is_empty() {
                    tracing::error!(
                        app = %app_name,
                        "[DesktopIntegration] Empty argument vector parsed from exec template"
                    );
                    return;
                }

                let mut cmd = Command::new(&argv[0]);
                if argv.len() > 1 {
                    cmd.args(&argv[1..]);
                }

                cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());

                match cmd.spawn() {
                    Ok(mut child) => {
                        let _ = child.wait();
                        tracing::info!(
                            app = %app_name,
                            elapsed_ms = t_spawn.elapsed().as_millis(),
                            "[DesktopIntegration] Application process launched and detached"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            app = %app_name,
                            error = %e,
                            "[DesktopIntegration] Failed to spawn application process"
                        );
                    }
                }
            })
            .map_err(SystemHandlerError::Io)?;

        tracing::debug!(
            app = %app.name,
            dispatch_us = t0.elapsed().as_micros(),
            "[DesktopIntegration] Application dispatch handed off to worker thread"
        );

        Ok(())
    }

    /// Triggers the native desktop application chooser dialog ("Other Application...") using KDE kdialog or Desktop Portal.
    pub fn open_with_dialog(file_path: &Path) -> Result<(), SystemHandlerError> {
        if !file_path.exists() {
            return Err(SystemHandlerError::FileNotFound(file_path.to_path_buf()));
        }

        let canonical = match file_path.canonicalize() {
            Ok(cp) => cp,
            Err(_) => file_path.to_path_buf(),
        };

        tracing::info!(
            target_file = %LogObscurer::path(&canonical),
            "[DesktopIntegration: DBus Portal] Triggering native 'Open With' application chooser..."
        );

        let path_clone = canonical.clone();

        std::thread::Builder::new()
            .name("sys-portal-open-with".into())
            .spawn(move || {
                let t_start = Instant::now();
                let path_str = path_clone.to_string_lossy().to_string();

                // 1. Primary: Native KDE Plasma kdialog --open-with
                if let Ok(mut child) = Command::new("kdialog")
                    .args(["--open-with", &path_str])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    && child.wait().is_ok_and(|s| s.success())
                {
                    tracing::info!(
                        elapsed_ms = t_start.elapsed().as_millis(),
                        target = %LogObscurer::path(&path_clone),
                        "[DesktopIntegration] Successfully opened native KDE application chooser via kdialog"
                    );
                    return;
                }

                // 2. Secondary: Portal OpenFile via Python 3 (matching img_picker.py QDBus implementation)
                let python_script = r"
import sys, os, time
try:
    from PyQt6.QtWidgets import QApplication
    from PyQt6.QtDBus import QDBusConnection, QDBusMessage, QDBusUnixFileDescriptor
    app = QApplication(['rustview'])
    bus = QDBusConnection.sessionBus()
    if not bus.isConnected():
        sys.exit(1)
    fd = os.open(sys.argv[1], os.O_RDONLY)
    dbus_fd = QDBusUnixFileDescriptor(fd)
    os.close(fd)
    msg = QDBusMessage.createMethodCall(
        'org.freedesktop.portal.Desktop',
        '/org/freedesktop/portal/desktop',
        'org.freedesktop.portal.OpenURI',
        'OpenFile'
    )
    msg.setArguments(['', dbus_fd, {'ask': True, 'handle_token': f'rustview_{int(time.time())}'}])
    res = bus.call(msg)
    sys.exit(0 if res.type() != QDBusMessage.MessageType.ErrorMessage else 1)
except Exception:
    sys.exit(1)
";

                if let Ok(mut child) = Command::new("python3")
                    .args(["-c", python_script, &path_str])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    && child.wait().is_ok_and(|s| s.success())
                {
                    tracing::info!(
                        elapsed_ms = t_start.elapsed().as_millis(),
                        target = %LogObscurer::path(&path_clone),
                        "[DesktopIntegration] Opened desktop application chooser portal via Python QDBus"
                    );
                    return;
                }

                // 3. Fallback: xdg-open
                let mut fallback_cmd = Command::new("xdg-open");
                fallback_cmd.arg(&path_clone);
                fallback_cmd
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

                if let Ok(mut child) = fallback_cmd.spawn() {
                    let _ = child.wait();
                    tracing::info!(
                        elapsed_ms = t_start.elapsed().as_millis(),
                        target = %LogObscurer::path(&path_clone),
                        "[DesktopIntegration] Opened file via xdg-open fallback"
                    );
                }
            })
            .map_err(SystemHandlerError::Io)?;

        Ok(())
    }

    /// Resolves the image file of an `Icon=` value: a file as it is, or a name looked up in the icon theme of the desktop, the
    /// fallback themes and `/usr/share/pixmaps` (Slint reads PNG and SVG, not the XPM files some themes still carry).
    pub fn resolve_icon_path(icon_name: &str) -> Option<PathBuf> {
        let icon = icon_name.trim();
        if icon.is_empty() {
            return None;
        }
        let as_path = Path::new(icon);
        if as_path.is_absolute() {
            return (as_path.is_file() && Self::is_usable_icon(as_path)).then(|| as_path.to_path_buf());
        }

        let desktop_theme = Self::desktop_icon_theme();
        for theme in desktop_theme.as_deref().into_iter().chain(["breeze", "hicolor"]) {
            if let Some(found) = freedesktop_icons::lookup(icon)
                .with_size(ICON_SIZE_PX)
                .with_scale(1)
                .with_theme(theme)
                .with_cache()
                .find()
                && Self::is_usable_icon(&found)
            {
                return Some(found);
            }
        }
        ["png", "svg"]
            .iter()
            .map(|extension| Path::new("/usr/share/pixmaps").join(format!("{icon}.{extension}")))
            .find(|candidate| candidate.is_file())
    }

    fn is_usable_icon(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "png" | "svg"))
    }

    /// The icon theme of the desktop: `[Icons] Theme=` of the KDE settings, else the GTK setting.
    fn desktop_icon_theme() -> Option<String> {
        let kdeglobals = directories::BaseDirs::new()?.config_dir().join("kdeglobals");
        ini::Ini::load_from_file(kdeglobals)
            .ok()
            .and_then(|settings| settings.get_from(Some("Icons"), "Theme").map(str::to_owned))
            .filter(|theme| !theme.is_empty())
            .or_else(freedesktop_icons::default_theme_gtk)
    }

    fn query_desktop_ids_for_mime(mime_type: &str) -> Vec<String> {
        let mut results = Vec::new();

        if let Ok(output) = Command::new("gio")
            .args(["mime", mime_type])
            .stdin(Stdio::null())
            .output()
            && output.status.success()
        {
            let text = String::from_utf8_lossy(&output.stdout);
            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.ends_with(".desktop") {
                    let parts: Vec<&str> = trimmed.split(':').collect();
                    let desktop_name = parts.last().unwrap_or(&trimmed).trim();
                    if !desktop_name.is_empty() && !results.contains(&desktop_name.to_string()) {
                        results.push(desktop_name.to_string());
                    }
                }
            }
        }

        results
    }

    /// Finds the desktop entry `desktop_id` (`org.kde.gwenview.desktop`) in the application folders of the user and the system.
    fn resolve_desktop_file(desktop_id: &str) -> Option<DesktopApp> {
        let locales = get_languages_from_env();
        let direct = default_paths()
            .map(|dir| dir.join(desktop_id))
            .find(|path| path.is_file());
        let path = direct.or_else(|| {
            Iter::new(default_paths()).find(|path| path.file_name().is_some_and(|name| name == desktop_id))
        })?;
        let entry = DesktopEntry::from_path(path, Some(&locales)).ok()?;
        Self::app_from_entry(&entry, desktop_id, &locales)
    }

    fn app_from_entry(entry: &DesktopEntry, desktop_id: &str, locales: &[String]) -> Option<DesktopApp> {
        if entry.no_display() || entry.hidden() {
            return None;
        }
        let exec = entry.exec()?.to_owned();
        let icon_name = entry.icon().unwrap_or_default().to_owned();
        let name = entry.name(locales).map_or_else(
            || desktop_id.strip_suffix(".desktop").unwrap_or(desktop_id).to_owned(),
            std::borrow::Cow::into_owned,
        );
        let icon_path = Self::resolve_icon_path(&icon_name);
        Some(DesktopApp {
            id: desktop_id.to_owned(),
            name,
            icon_name,
            icon_path,
            exec,
        })
    }

    /// The command line of an application for `paths`. The field codes of the `Exec=` line are expanded by the desktop entry
    /// crate: `%f`/`%F` get paths, `%u`/`%U` get percent-encoded `file://` URIs; a line without a file field code gets the paths appended.
    fn build_argv(exec_template: &str, paths: &[PathBuf]) -> Vec<String> {
        let wants_uris = exec_template.contains("%u") || exec_template.contains("%U");
        let arguments: Vec<String> = paths
            .iter()
            .map(|path| {
                if wants_uris {
                    super::dnd::SystemDndHandler::path_to_file_uri(path)
                } else {
                    path.to_string_lossy().into_owned()
                }
            })
            .collect();
        let argument_refs: Vec<&str> = arguments.iter().map(String::as_str).collect();

        let mut entry = DesktopEntry::from_appid(String::new());
        entry.add_desktop_entry("Exec".to_owned(), exec_template.to_owned());
        let mut argv = entry
            .parse_exec_with_uris(&argument_refs, &[] as &[&str])
            .unwrap_or_default();

        let has_file_code = ["%f", "%F", "%u", "%U"].iter().any(|code| exec_template.contains(code));
        if !argv.is_empty() && !has_file_code {
            argv.extend(arguments);
        }
        argv
    }
}

/// Icons of applications are shown at this size (pixels).
const ICON_SIZE_PX: u16 = 48;

#[cfg(test)]
mod tests {
    use super::DesktopIntegration;
    use std::path::PathBuf;

    #[test]
    fn a_path_field_code_gets_the_first_path() {
        let argv =
            DesktopIntegration::build_argv("gwenview %f", &[PathBuf::from("/a b/c.jpg"), PathBuf::from("/d.png")]);
        assert_eq!(argv, ["gwenview", "/a b/c.jpg"]);
    }

    #[test]
    fn a_url_list_field_code_gets_percent_encoded_uris() {
        let argv = DesktopIntegration::build_argv(
            "mpv --player-operation-mode=pseudo-gui -- %U",
            &[PathBuf::from("/a b/c.mp4"), PathBuf::from("/d.mkv")],
        );
        assert_eq!(
            argv,
            [
                "mpv",
                "--player-operation-mode=pseudo-gui",
                "--",
                "file:///a%20b/c.mp4",
                "file:///d.mkv"
            ]
        );
    }

    #[test]
    fn quoted_arguments_and_other_field_codes_are_handled() {
        let argv = DesktopIntegration::build_argv("\"/opt/My App/run\" --name %c %F", &[PathBuf::from("/x.jpg")]);
        assert_eq!(argv.first().map(String::as_str), Some("/opt/My App/run"));
        assert_eq!(argv.last().map(String::as_str), Some("/x.jpg"));
    }

    #[test]
    fn a_line_without_a_file_field_code_gets_the_paths_appended() {
        let argv = DesktopIntegration::build_argv("viewer --fullscreen", &[PathBuf::from("/x.jpg")]);
        assert_eq!(argv, ["viewer", "--fullscreen", "/x.jpg"]);
    }

    #[test]
    fn an_unusable_icon_is_not_resolved() {
        assert_eq!(DesktopIntegration::resolve_icon_path(""), None);
        let dir = tempfile::tempdir().expect("temp dir");
        let xpm = dir.path().join("a.xpm");
        std::fs::write(&xpm, b"x").expect("write");
        assert_eq!(
            DesktopIntegration::resolve_icon_path(xpm.to_str().expect("utf-8")),
            None
        );
        let png = dir.path().join("a.png");
        std::fs::write(&png, b"x").expect("write");
        assert_eq!(
            DesktopIntegration::resolve_icon_path(png.to_str().expect("utf-8")),
            Some(png)
        );
    }
}
