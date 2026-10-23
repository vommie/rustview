use parking_lot::RwLock;
use rustview_core::settings::FullscreenSettings;
use rustview_core::util::cast::SaturatingAs;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

mod detection;

type MonitorCacheEntry = (Instant, Vec<MonitorInfo>);
type MonitorCacheLock = RwLock<Option<MonitorCacheEntry>>;

static MONITORS_CACHE: LazyLock<MonitorCacheLock> = LazyLock::new(|| RwLock::new(None));

/// Detailed hardware and geometry information for a connected Linux monitor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub index: usize,
    pub connector: String,
    pub model: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub refresh_rate: u32,
    pub is_primary: bool,
}

impl MonitorInfo {
    /// Generates a standardized, clean, human-readable display name for UI dropdowns.
    /// Example: "1: DP-1 - 2560x1440 @ 144Hz [Primary]"
    pub fn display_name(&self) -> String {
        let mut name = format!("{}: {}", self.index + 1, self.connector);
        if !self.model.is_empty() && self.model != self.connector {
            let _ = write!(name, " ({})", self.model);
        }
        if self.width > 0 && self.height > 0 {
            let _ = write!(name, " - {}x{}", self.width, self.height);
            if self.refresh_rate > 0 {
                let _ = write!(name, " @ {}Hz", self.refresh_rate);
            }
        }
        if self.is_primary {
            name.push_str(" [Primary]");
        }
        name
    }

    /// Checks if a 2D coordinate point (e.g. window center) falls inside this monitor's boundary.
    #[inline]
    pub fn contains_point(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && px < self.x + self.width.saturating_as::<i32>()
            && py >= self.y
            && py < self.y + self.height.saturating_as::<i32>()
    }

    /// Calculates the overlapping area in square pixels between this monitor and a window rectangle.
    pub fn overlap_area(&self, wx: i32, wy: i32, ww: u32, wh: u32) -> u64 {
        let x1 = self.x.max(wx);
        let y1 = self.y.max(wy);
        let x2 = (self.x + self.width.saturating_as::<i32>()).min(wx + ww.saturating_as::<i32>());
        let y2 = (self.y + self.height.saturating_as::<i32>()).min(wy + wh.saturating_as::<i32>());

        if x2 > x1 && y2 > y1 {
            (x2 - x1).saturating_as::<u64>() * (y2 - y1).saturating_as::<u64>()
        } else {
            0
        }
    }
}

/// Central service responsible for querying, detecting, and resolving connected Linux monitors.
pub struct SystemDisplayService;

impl SystemDisplayService {
    /// Detects connected monitors using a 2-second in-memory cache to eliminate IPC freezes during rapid calls.
    pub fn detect_monitors() -> Vec<MonitorInfo> {
        {
            let guard = MONITORS_CACHE.read();
            if let Some((instant, ref cached)) = *guard
                && instant.elapsed() < Duration::from_millis(2000)
            {
                return cached.clone();
            }
        }

        Self::force_detect_monitors()
    }

    /// Bypasses the cache and executes an immediate hardware re-scan (used by the Refresh button).
    pub fn force_detect_monitors() -> Vec<MonitorInfo> {
        let t0 = Instant::now();

        // Tier 1: Try KDE Plasma kscreen-doctor -j (Works natively on both X11 and Wayland on KDE Plasma)
        if let Some(monitors) = Self::detect_via_kscreen_doctor()
            && !monitors.is_empty()
        {
            tracing::info!(
                count = monitors.len(),
                elapsed_us = t0.elapsed().as_micros(),
                backend = "kscreen-doctor",
                "[SystemDisplayService] Detected monitors via kscreen-doctor"
            );
            *MONITORS_CACHE.write() = Some((Instant::now(), monitors.clone()));
            return monitors;
        }

        // Tier 2: Try xrandr --listmonitors (Universal across X11 and XWayland sessions)
        if let Some(monitors) = Self::detect_via_xrandr_listmonitors()
            && !monitors.is_empty()
        {
            tracing::info!(
                count = monitors.len(),
                elapsed_us = t0.elapsed().as_micros(),
                backend = "xrandr --listmonitors",
                "[SystemDisplayService] Detected monitors via xrandr listmonitors"
            );
            *MONITORS_CACHE.write() = Some((Instant::now(), monitors.clone()));
            return monitors;
        }

        // Tier 3: General xrandr --current parsing
        if let Some(monitors) = Self::detect_via_xrandr()
            && !monitors.is_empty()
        {
            tracing::info!(
                count = monitors.len(),
                elapsed_us = t0.elapsed().as_micros(),
                backend = "xrandr --current",
                "[SystemDisplayService] Detected monitors via xrandr"
            );
            *MONITORS_CACHE.write() = Some((Instant::now(), monitors.clone()));
            return monitors;
        }

        // Tier 4: wlr-randr --json (for wlroots compositors: Sway, Hyprland, Wayfire)
        if let Some(monitors) = Self::detect_via_wlr_randr()
            && !monitors.is_empty()
        {
            tracing::info!(
                count = monitors.len(),
                elapsed_us = t0.elapsed().as_micros(),
                backend = "wlr-randr",
                "[SystemDisplayService] Detected monitors via wlr-randr"
            );
            *MONITORS_CACHE.write() = Some((Instant::now(), monitors.clone()));
            return monitors;
        }

        // Tier 5: Low-level Linux DRM sysfs (/sys/class/drm) fallback
        let drm_monitors = Self::detect_via_drm_sysfs();
        if !drm_monitors.is_empty() {
            tracing::info!(
                count = drm_monitors.len(),
                elapsed_us = t0.elapsed().as_micros(),
                backend = "drm_sysfs",
                "[SystemDisplayService] Detected monitors via DRM sysfs"
            );
            *MONITORS_CACHE.write() = Some((Instant::now(), drm_monitors.clone()));
            return drm_monitors;
        }

        // Tier 6: Ultimate Fallback: Default virtual primary monitor
        tracing::warn!("[SystemDisplayService] No monitors detected via any backend; using default display fallback");
        let fallback = vec![MonitorInfo {
            index: 0,
            connector: "Default".to_string(),
            model: "Default Display".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            refresh_rate: 60,
            is_primary: true,
        }];
        *MONITORS_CACHE.write() = Some((Instant::now(), fallback.clone()));
        fallback
    }

    /// Resolves the monitor onto which the fullscreen window should be placed.
    pub fn resolve_target_monitor(
        settings: &FullscreenSettings,
        window_geom: Option<(i32, i32, u32, u32)>,
    ) -> MonitorInfo {
        let monitors = Self::detect_monitors();

        // 1. Primary rule: Specific monitor enabled and configured
        if settings.enable_specific_monitor && !settings.specific_monitor.trim().is_empty() {
            let target_connector = settings.specific_monitor.trim();
            if let Some(matched) = monitors.iter().find(|m| {
                m.connector.eq_ignore_ascii_case(target_connector) || m.model.eq_ignore_ascii_case(target_connector)
            }) {
                tracing::info!(
                    connector = %matched.connector,
                    index = matched.index,
                    x = matched.x,
                    y = matched.y,
                    width = matched.width,
                    height = matched.height,
                    "[SystemDisplayService] Target monitor resolved from specific configuration"
                );
                return matched.clone();
            }
            tracing::warn!(
                requested = %target_connector,
                total_available = monitors.len(),
                "[SystemDisplayService] Specific monitor unavailable/disconnected; evaluating fallback rule"
            );
        }

        // 2. Identify the monitor currently hosting the Main UI
        let main_ui_monitor = Self::find_main_ui_monitor(&monitors, window_geom);

        // 3. Fallback rule: evaluate if enabled
        if settings.enable_fallback_rule {
            if settings.fallback_target.eq_ignore_ascii_case("different") {
                if let Some(different) = monitors.iter().find(|m| m.connector != main_ui_monitor.connector) {
                    tracing::info!(
                        main_ui_display = %main_ui_monitor.connector,
                        target_display = %different.connector,
                        target_index = different.index,
                        "[SystemDisplayService] Resolved different display from Main UI"
                    );
                    return different.clone();
                }
                tracing::info!(
                    main_ui_display = %main_ui_monitor.connector,
                    "[SystemDisplayService] 'Different display' requested but only 1 monitor connected; using main display"
                );
            } else {
                tracing::info!(
                    main_ui_display = %main_ui_monitor.connector,
                    main_ui_index = main_ui_monitor.index,
                    "[SystemDisplayService] Fallback rule explicitly set to 'Same display as Main UI'"
                );
            }
        } else {
            tracing::info!(
                main_ui_display = %main_ui_monitor.connector,
                main_ui_index = main_ui_monitor.index,
                "[SystemDisplayService] No specific or fallback options enabled; using Main UI display"
            );
        }

        main_ui_monitor
    }

    /// Emits structured startup diagnostics covering host OS, Linux kernel, session architecture, and active displays.
    /// Logs the host environment (distribution, kernel, session type). Reads two small files and the environment; no
    /// subprocess, so it is cheap enough for the start of the launch (a crash report then says where it happened).
    pub fn log_host_environment() {
        let distro = std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|content| {
                for line in content.lines() {
                    if let Some(name) = line.strip_prefix("PRETTY_NAME=") {
                        return Some(name.trim_matches('"').to_string());
                    }
                }
                None
            })
            .unwrap_or_else(|| "Unknown Linux".to_string());

        let kernel = rustix::system::uname().release().to_string_lossy().into_owned();

        let session_type = std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".into());
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_else(|_| "unknown".into());
        let wayland_display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "none".into());
        let x11_display = std::env::var("DISPLAY").unwrap_or_else(|_| "none".into());

        tracing::info!(
            pid = std::process::id(),
            distro = %distro,
            kernel = %kernel,
            session_type = %session_type,
            desktop = %desktop,
            display = %x11_display,
            wayland_display = %wayland_display,
            "[SystemDiagnostics: Startup] Linux host environment"
        );
    }

    /// Logs the connected monitors on a thread of its own. Detecting them starts subprocesses (`kscreen-doctor`, `xrandr`), so
    /// the launch calls this only after the first frame: it must neither delay the first window nor compete with it for CPU.
    pub fn spawn_monitor_diagnostics() {
        let spawned = std::thread::Builder::new()
            .name("system-diagnostics".into())
            .spawn(Self::log_monitors_blocking);
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "[SystemDiagnostics: Monitor] Failed to start the diagnostics thread");
        }
    }

    fn log_monitors_blocking() {
        let monitors = Self::detect_monitors();
        tracing::info!(
            connected_monitors = monitors.len(),
            "[SystemDiagnostics: Monitor] Display subsystem initialized"
        );

        for m in &monitors {
            tracing::info!(
                index = m.index,
                connector = %m.connector,
                resolution = format!("{}x{}", m.width, m.height),
                offset = format!("+{}+{}", m.x, m.y),
                refresh_rate_hz = m.refresh_rate,
                is_primary = m.is_primary,
                model = %m.model,
                "[SystemDiagnostics: Monitor] Active output device"
            );
        }
    }

    fn find_main_ui_monitor(monitors: &[MonitorInfo], window_geom: Option<(i32, i32, u32, u32)>) -> MonitorInfo {
        if let Some((wx, wy, ww, wh)) = window_geom {
            let center_x = wx + (ww.saturating_as::<i32>() / 2);
            let center_y = wy + (wh.saturating_as::<i32>() / 2);

            if let Some(containing) = monitors.iter().find(|m| m.contains_point(center_x, center_y)) {
                return containing.clone();
            }

            let mut best_overlap = 0u64;
            let mut best_monitor = None;
            for m in monitors {
                let area = m.overlap_area(wx, wy, ww, wh);
                if area > best_overlap {
                    best_overlap = area;
                    best_monitor = Some(m.clone());
                }
            }

            if let Some(best) = best_monitor {
                return best;
            }
        }

        if let Some(primary) = monitors.iter().find(|m| m.is_primary) {
            return primary.clone();
        }

        if let Some(first) = monitors.first() {
            return first.clone();
        }

        MonitorInfo {
            index: 0,
            connector: "Default".to_string(),
            model: "Default Display".to_string(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            refresh_rate: 60,
            is_primary: true,
        }
    }
}
