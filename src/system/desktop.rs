use super::error::SystemHandlerError;
use crate::settings::LogObscurer;
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
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
/// and executes native application chooser dialogs via DBus Desktop Portals or KDE kdialog.
pub struct DesktopIntegration;

impl DesktopIntegration {
    /// Standard Freedesktop application search directories.
    const APP_DIRS: &[&'static str] = &[
        "~/.local/share/applications",
        "/usr/share/applications",
        "/usr/local/share/applications",
        "/var/lib/flatpak/exports/share/applications",
        "~/.local/share/flatpak/exports/share/applications",
    ];

    /// Standard Freedesktop icon search directories.
    const ICON_DIRS: &[&'static str] = &[
        "/usr/share/icons/hicolor",
        "/usr/share/icons/breeze",
        "/usr/share/icons/breeze-dark",
        "/usr/share/icons/Papirus",
        "/usr/share/icons/Adwaita",
        "/usr/share/pixmaps",
        "~/.local/share/icons",
        "~/.icons",
        "/var/lib/flatpak/exports/share/icons/hicolor",
    ];

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
    pub fn launch_app<P: AsRef<Path>>(
        app: &DesktopApp,
        file_paths: &[P],
    ) -> Result<(), SystemHandlerError> {
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
            return Err(SystemHandlerError::FileNotFound(
                file_paths[0].as_ref().to_path_buf(),
            ));
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

                cmd.stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());

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
                let python_script = r#"
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
"#;

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

    /// Resolves the absolute filesystem path for a Freedesktop application icon name.
    pub fn resolve_icon_path(icon_name: &str) -> Option<PathBuf> {
        let trimmed = icon_name.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Direct absolute path check
        let as_path = Path::new(trimmed);
        if as_path.is_absolute() && as_path.is_file() {
            return Some(as_path.to_path_buf());
        }

        let clean_name = trimmed
            .strip_suffix(".png")
            .or_else(|| trimmed.strip_suffix(".svg"))
            .or_else(|| trimmed.strip_suffix(".xpm"))
            .unwrap_or(trimmed);

        let sub_sizes = &[
            "48x48/apps",
            "32x32/apps",
            "64x64/apps",
            "scalable/apps",
            "128x128/apps",
            "24x24/apps",
            "16x16/apps",
            "apps/48",
            "apps/32",
            "apps/64",
            "apps/scalable",
        ];

        let extensions = &["png", "svg"];

        for &base in Self::ICON_DIRS {
            let expanded_base = Self::expand_home(base);
            if !expanded_base.exists() {
                continue;
            }

            // Direct check in pixmaps or flat base
            for ext in extensions {
                let candidate = expanded_base.join(format!("{}.{}", clean_name, ext));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }

            // Subdirectories (theme sizes)
            for sub in sub_sizes {
                for ext in extensions {
                    let candidate = expanded_base.join(sub).join(format!("{}.{}", clean_name, ext));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }

        None
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

    fn resolve_desktop_file(desktop_id: &str) -> Option<DesktopApp> {
        for &base in Self::APP_DIRS {
            let expanded_base = Self::expand_home(base);
            if !expanded_base.exists() {
                continue;
            }

            let direct_path = expanded_base.join(desktop_id);
            if direct_path.is_file()
                && let Some(app) = Self::parse_desktop_file(&direct_path, desktop_id)
            {
                return Some(app);
            }

            // Recursive scan if not direct
            if let Ok(entries) = std::fs::read_dir(&expanded_base) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file()
                        && p.file_name().and_then(|n| n.to_str()) == Some(desktop_id)
                        && let Some(app) = Self::parse_desktop_file(&p, desktop_id)
                    {
                        return Some(app);
                    }
                }
            }
        }

        None
    }

    fn parse_desktop_file(path: &Path, desktop_id: &str) -> Option<DesktopApp> {
        let file = File::open(path).ok()?;
        let reader = BufReader::new(file);

        let mut in_desktop_entry = false;
        let mut name: Option<String> = None;
        let mut icon: Option<String> = None;
        let mut exec: Option<String> = None;
        let mut no_display = false;

        for line_res in reader.lines() {
            let line = match line_res {
                Ok(l) => l.trim().to_string(),
                Err(_) => continue,
            };

            if line == "[Desktop Entry]" {
                in_desktop_entry = true;
                continue;
            } else if line.starts_with('[') && in_desktop_entry {
                break;
            }

            if !in_desktop_entry {
                continue;
            }

            if let Some(val) = line.strip_prefix("Name=") {
                if name.is_none() {
                    name = Some(val.trim().to_string());
                }
            } else if let Some(val) = line.strip_prefix("Icon=") {
                if icon.is_none() {
                    icon = Some(val.trim().to_string());
                }
            } else if let Some(val) = line.strip_prefix("Exec=") {
                if exec.is_none() {
                    exec = Some(val.trim().to_string());
                }
            } else if let Some(val) = line.strip_prefix("NoDisplay=") {
                no_display = val.trim().eq_ignore_ascii_case("true");
            }
        }

        if no_display {
            return None;
        }

        let app_name = name.unwrap_or_else(|| {
            desktop_id
                .strip_suffix(".desktop")
                .unwrap_or(desktop_id)
                .to_string()
        });

        let app_exec = exec?;
        let app_icon = icon.unwrap_or_default();
        let icon_path = if !app_icon.is_empty() {
            Self::resolve_icon_path(&app_icon)
        } else {
            None
        };

        Some(DesktopApp {
            id: desktop_id.to_string(),
            name: app_name,
            icon_name: app_icon,
            icon_path,
            exec: app_exec,
        })
    }

    fn build_argv(exec_template: &str, paths: &[PathBuf]) -> Vec<String> {
        let mut tokens = Vec::new();
        let mut current = String::new();
        let mut in_single_quote = false;
        let mut in_double_quote = false;
        let mut escaped = false;

        for c in exec_template.chars() {
            if escaped {
                current.push(c);
                escaped = false;
                continue;
            }

            if c == '\\' && !in_single_quote {
                escaped = true;
                continue;
            }

            if c == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
                continue;
            }

            if c == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
                continue;
            }

            if c.is_whitespace() && !in_single_quote && !in_double_quote {
                if !current.is_empty() {
                    tokens.push(current);
                    current = String::new();
                }
            } else {
                current.push(c);
            }
        }

        if !current.is_empty() {
            tokens.push(current);
        }

        let mut final_argv = Vec::new();
        let mut paths_inserted = false;

        for token in tokens {
            if token.contains("%f") || token.contains("%u") {
                if let Some(first_path) = paths.first() {
                    let replacement = if token.contains("%u") {
                        format!("file://{}", first_path.to_string_lossy())
                    } else {
                        first_path.to_string_lossy().to_string()
                    };
                    let replaced = token.replace("%f", &replacement).replace("%u", &replacement);
                    final_argv.push(replaced);
                    paths_inserted = true;
                }
            } else if token.contains("%F") || token.contains("%U") {
                for p in paths {
                    let formatted = if token.contains("%U") {
                        format!("file://{}", p.to_string_lossy())
                    } else {
                        p.to_string_lossy().to_string()
                    };
                    final_argv.push(formatted);
                }
                paths_inserted = true;
            } else if token.starts_with('%') {
                continue;
            } else {
                final_argv.push(token);
            }
        }

        if !paths_inserted {
            for p in paths {
                final_argv.push(p.to_string_lossy().to_string());
            }
        }

        final_argv
    }

    fn expand_home(path_str: &str) -> PathBuf {
        if let Some(stripped) = path_str.strip_prefix("~/")
            && let Ok(home) = std::env::var("HOME")
        {
            return PathBuf::from(home).join(stripped);
        }
        PathBuf::from(path_str)
    }
}
