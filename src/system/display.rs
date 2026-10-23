use crate::settings::FullscreenSettings;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

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
            name.push_str(&format!(" ({})", self.model));
        }
        if self.width > 0 && self.height > 0 {
            name.push_str(&format!(" - {}x{}", self.width, self.height));
            if self.refresh_rate > 0 {
                name.push_str(&format!(" @ {}Hz", self.refresh_rate));
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
            && px < self.x + (self.width as i32)
            && py >= self.y
            && py < self.y + (self.height as i32)
    }

    /// Calculates the overlapping area in square pixels between this monitor and a window rectangle.
    pub fn overlap_area(&self, wx: i32, wy: i32, ww: u32, wh: u32) -> u64 {
        let x1 = self.x.max(wx);
        let y1 = self.y.max(wy);
        let x2 = (self.x + self.width as i32).min(wx + ww as i32);
        let y2 = (self.y + self.height as i32).min(wy + wh as i32);

        if x2 > x1 && y2 > y1 {
            (x2 - x1) as u64 * (y2 - y1) as u64
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
        tracing::warn!(
            "[SystemDisplayService] No monitors detected via any backend; using default display fallback"
        );
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
                m.connector.eq_ignore_ascii_case(target_connector)
                    || m.model.eq_ignore_ascii_case(target_connector)
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
            } else {
                tracing::warn!(
                    requested = %target_connector,
                    total_available = monitors.len(),
                    "[SystemDisplayService] Specific monitor unavailable/disconnected; evaluating fallback rule"
                );
            }
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
                } else {
                    tracing::info!(
                        main_ui_display = %main_ui_monitor.connector,
                        "[SystemDisplayService] 'Different display' requested but only 1 monitor connected; using main display"
                    );
                }
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

    fn find_main_ui_monitor(
        monitors: &[MonitorInfo],
        window_geom: Option<(i32, i32, u32, u32)>,
    ) -> MonitorInfo {
        if let Some((wx, wy, ww, wh)) = window_geom {
            let center_x = wx + (ww as i32 / 2);
            let center_y = wy + (wh as i32 / 2);

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

    fn detect_via_kscreen_doctor() -> Option<Vec<MonitorInfo>> {
        let output = Command::new("kscreen-doctor").args(["-j"]).output().ok()?;

        if !output.status.success() {
            return None;
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let outputs = json.get("outputs")?.as_array()?;

        let mut raw_monitors = Vec::new();
        for out in outputs {
            let is_connected = out
                .get("connected")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let is_enabled = out
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            if !is_connected || !is_enabled {
                continue;
            }

            let name = out
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let is_primary = out
                .get("primary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let x = out
                .get("pos")
                .and_then(|p| p.get("x"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;
            let y = out
                .get("pos")
                .and_then(|p| p.get("y"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32;

            let (width, height, refresh_rate) =
                if let Some(modes) = out.get("modes").and_then(|m| m.as_array()) {
                    let current_mode_id = out
                        .get("currentModeId")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let mut found_w = 0u32;
                    let mut found_h = 0u32;
                    let mut found_rate = 60u32;

                    for mode in modes {
                        let m_id = mode.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        if m_id == current_mode_id || current_mode_id.is_empty() {
                            if let Some(size) = mode.get("size") {
                                found_w = size
                                    .get("width")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                                found_h = size
                                    .get("height")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0)
                                    as u32;
                            }
                            if let Some(rate) =
                                mode.get("refreshRate").and_then(|v| v.as_f64())
                            {
                                found_rate = rate.round() as u32;
                            }
                            break;
                        }
                    }
                    (found_w, found_h, found_rate)
                } else {
                    let w = out
                        .get("size")
                        .and_then(|s| s.get("width"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1920) as u32;
                    let h = out
                        .get("size")
                        .and_then(|s| s.get("height"))
                        .and_then(|v| v.as_u64())
                        .unwrap_or(1080) as u32;
                    (w, h, 60)
                };

            if width > 0 && height > 0 {
                raw_monitors.push((x, y, name, width, height, refresh_rate, is_primary));
            }
        }

        if raw_monitors.is_empty() {
            return None;
        }

        // Sort monitors strictly by physical position (x, y) so index 0 = left/top, index 1 = right (matches X11 Xinerama)
        raw_monitors.sort_by_key(|m| (m.0, m.1));

        let mut monitors = Vec::with_capacity(raw_monitors.len());
        for (idx, (x, y, name, width, height, refresh_rate, is_primary)) in
            raw_monitors.into_iter().enumerate()
        {
            monitors.push(MonitorInfo {
                index: idx,
                connector: name.clone(),
                model: name,
                x,
                y,
                width,
                height,
                refresh_rate,
                is_primary,
            });
        }

        Some(monitors)
    }

    fn detect_via_xrandr_listmonitors() -> Option<Vec<MonitorInfo>> {
        let output = Command::new("xrandr").arg("--listmonitors").output().ok()?;

        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let rates = Self::get_xrandr_rates();

        let mut monitors = Vec::new();
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("Monitors:") || trimmed.is_empty() {
                continue;
            }

            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }

            let idx_str = parts[0].trim_end_matches(':');
            let Ok(index) = idx_str.parse::<usize>() else {
                continue;
            };

            let connector = parts.last()?.to_string();
            let is_primary = parts[1].contains('*');

            let geom_part = if parts.len() >= 4 { parts[2] } else { parts[1] };
            if let Some((x, y, width, height)) = parse_listmonitors_geom(geom_part) {
                let refresh_rate = rates.get(&connector).copied().unwrap_or(60);
                monitors.push(MonitorInfo {
                    index,
                    connector: connector.clone(),
                    model: connector,
                    x,
                    y,
                    width,
                    height,
                    refresh_rate,
                    is_primary,
                });
            }
        }

        if monitors.is_empty() {
            None
        } else {
            Some(monitors)
        }
    }

    fn get_xrandr_rates() -> HashMap<String, u32> {
        let mut rates = HashMap::new();
        let Ok(output) = Command::new("xrandr").arg("--current").output() else {
            return rates;
        };

        if !output.status.success() {
            return rates;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut current_conn: Option<String> = None;

        for line in text.lines() {
            if line.contains(" connected") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if !parts.is_empty() {
                    current_conn = Some(parts[0].to_string());
                }
            } else if line.starts_with("   ") && current_conn.is_some() && line.contains('*') {
                for part in line.split_whitespace() {
                    if part.contains('*')
                        && let Ok(rate_f) = part.trim_end_matches(['*', '+']).parse::<f32>()
                        && let Some(ref conn) = current_conn
                    {
                        rates.insert(conn.clone(), rate_f.round() as u32);
                        break;
                    }
                }
            }
        }

        rates
    }

    fn detect_via_xrandr() -> Option<Vec<MonitorInfo>> {
        let output = Command::new("xrandr")
            .arg("--current")
            .output()
            .or_else(|_| Command::new("xrandr").output())
            .ok()?;

        if !output.status.success() {
            return None;
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let mut monitors = Vec::new();

        let mut current_connector: Option<String> = None;
        let mut current_is_primary = false;
        let mut current_geom: Option<(i32, i32, u32, u32)> = None;
        let mut current_rate = 60u32;

        for line in text.lines() {
            if line.contains(" connected") {
                if let (Some(conn), Some((x, y, width, height))) =
                    (current_connector.take(), current_geom.take())
                {
                    let idx = monitors.len();
                    monitors.push(MonitorInfo {
                        index: idx,
                        connector: conn.clone(),
                        model: conn,
                        x,
                        y,
                        width,
                        height,
                        refresh_rate: current_rate,
                        is_primary: current_is_primary,
                    });
                }

                current_is_primary = false;
                current_rate = 60;
                current_geom = None;

                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }
                let connector = parts[0].to_string();
                current_connector = Some(connector);

                if line.contains(" primary ") {
                    current_is_primary = true;
                }

                for part in parts.iter().skip(2) {
                    if let Some(geom) = parse_xrandr_geom(part) {
                        current_geom = Some(geom);
                        break;
                    }
                }
            } else if line.starts_with("   ")
                && current_connector.is_some()
                && line.contains('*')
            {
                for part in line.split_whitespace() {
                    if part.contains('*')
                        && let Ok(rate_f) = part.trim_end_matches(['*', '+']).parse::<f32>()
                    {
                        current_rate = rate_f.round() as u32;
                    }
                }
            }
        }

        if let (Some(conn), Some((x, y, width, height))) = (current_connector, current_geom) {
            let idx = monitors.len();
            monitors.push(MonitorInfo {
                index: idx,
                connector: conn.clone(),
                model: conn,
                x,
                y,
                width,
                height,
                refresh_rate: current_rate,
                is_primary: current_is_primary,
            });
        }

        if monitors.is_empty() {
            None
        } else {
            monitors.sort_by_key(|m| (m.x, m.y));
            for (idx, m) in monitors.iter_mut().enumerate() {
                m.index = idx;
            }
            Some(monitors)
        }
    }

    fn detect_via_wlr_randr() -> Option<Vec<MonitorInfo>> {
        let output = Command::new("wlr-randr").args(["--json"]).output().ok()?;

        if !output.status.success() {
            return None;
        }

        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let outputs = json.as_array()?;

        let mut monitors = Vec::new();
        for out in outputs {
            let name = out
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let model = out
                .get("model")
                .and_then(|v| v.as_str())
                .unwrap_or(&name)
                .to_string();

            let x = out.get("x").and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let y = out.get("y").and_then(|v| v.as_i64()).unwrap_or(0) as i32;

            let mut width = 0u32;
            let mut height = 0u32;
            let mut refresh_rate = 60u32;

            if let Some(modes) = out.get("modes").and_then(|m| m.as_array()) {
                for m in modes {
                    let is_current = m
                        .get("current")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    if is_current {
                        width = m
                            .get("width")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        height = m
                            .get("height")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0) as u32;
                        refresh_rate = m
                            .get("refresh")
                            .and_then(|v| v.as_f64())
                            .map(|r| r.round() as u32)
                            .unwrap_or(60);
                        break;
                    }
                }
            }

            if width > 0 && height > 0 {
                let idx = monitors.len();
                monitors.push(MonitorInfo {
                    index: idx,
                    connector: name,
                    model,
                    x,
                    y,
                    width,
                    height,
                    refresh_rate,
                    is_primary: idx == 0,
                });
            }
        }

        if monitors.is_empty() {
            None
        } else {
            monitors.sort_by_key(|m| (m.x, m.y));
            for (idx, m) in monitors.iter_mut().enumerate() {
                m.index = idx;
            }
            Some(monitors)
        }
    }

    fn detect_via_drm_sysfs() -> Vec<MonitorInfo> {
        let drm_path = Path::new("/sys/class/drm");
        let mut monitors = Vec::new();

        if let Ok(entries) = std::fs::read_dir(drm_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                let status_path = path.join("status");
                if status_path.is_file()
                    && let Ok(status) = std::fs::read_to_string(&status_path)
                    && status.trim() == "connected"
                {
                    let dir_name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();

                    let connector = if let Some(idx) = dir_name.find('-') {
                        dir_name[idx + 1..].to_string()
                    } else {
                        dir_name.clone()
                    };

                    let modes_path = path.join("modes");
                    let mut width = 1920u32;
                    let mut height = 1080u32;

                    if modes_path.is_file()
                        && let Ok(modes_str) = std::fs::read_to_string(&modes_path)
                        && let Some(first_mode) = modes_str.lines().next()
                    {
                        let parts: Vec<&str> = first_mode.split('x').collect();
                        if parts.len() == 2
                            && let (Ok(w), Ok(h)) =
                                (parts[0].trim().parse(), parts[1].trim().parse())
                        {
                            width = w;
                            height = h;
                        }
                    }

                    let idx = monitors.len();
                    monitors.push(MonitorInfo {
                        index: idx,
                        connector: connector.clone(),
                        model: connector,
                        x: 0,
                        y: 0,
                        width,
                        height,
                        refresh_rate: 60,
                        is_primary: idx == 0,
                    });
                }
            }
        }

        monitors
    }
}

fn parse_listmonitors_geom(part: &str) -> Option<(i32, i32, u32, u32)> {
    let x_idx = part.find('x')?;
    let w_part = &part[..x_idx];
    let w_str = w_part.split('/').next()?;
    let width = w_str.parse::<u32>().ok()?;

    let rest = &part[x_idx + 1..];
    let sign_pos = rest.find(['+', '-'])?;
    let h_part = &rest[..sign_pos];
    let h_str = h_part.split('/').next()?;
    let height = h_str.parse::<u32>().ok()?;

    let pos_str = &rest[sign_pos..];
    let second_sign_pos = pos_str[1..].find(['+', '-']).map(|p| p + 1)?;
    let x_str = &pos_str[..second_sign_pos];
    let y_str = &pos_str[second_sign_pos..];

    let x = x_str.strip_prefix('+').unwrap_or(x_str).parse::<i32>().ok()?;
    let y = y_str.strip_prefix('+').unwrap_or(y_str).parse::<i32>().ok()?;

    Some((x, y, width, height))
}

fn parse_xrandr_geom(part: &str) -> Option<(i32, i32, u32, u32)> {
    let x_pos = part.find('x')?;
    let w_str = &part[..x_pos];
    let width = w_str.parse::<u32>().ok()?;

    let rest = &part[x_pos + 1..];
    let sign_pos = rest.find(['+', '-'])?;
    let h_str = &rest[..sign_pos];
    let height = h_str.parse::<u32>().ok()?;

    let pos_str = &rest[sign_pos..];
    let second_sign_pos = pos_str[1..].find(['+', '-']).map(|p| p + 1)?;
    let x_str = &pos_str[..second_sign_pos];
    let y_str = &pos_str[second_sign_pos..];

    let x = x_str.strip_prefix('+').unwrap_or(x_str).parse::<i32>().ok()?;
    let y = y_str.strip_prefix('+').unwrap_or(y_str).parse::<i32>().ok()?;

    Some((x, y, width, height))
}
