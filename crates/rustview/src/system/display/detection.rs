//! The monitor detection backends (kscreen-doctor, xrandr, wlr-randr, DRM sysfs) and their output parsers.

use super::{MonitorInfo, SystemDisplayService};
use rustview_core::util::cast::{FloatToInt, SaturatingAs};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

pub(super) fn parse_listmonitors_geom(part: &str) -> Option<(i32, i32, u32, u32)> {
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

pub(super) fn parse_xrandr_geom(part: &str) -> Option<(i32, i32, u32, u32)> {
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

impl SystemDisplayService {
    pub(super) fn detect_via_kscreen_doctor() -> Option<Vec<MonitorInfo>> {
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
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let is_enabled = out.get("enabled").and_then(serde_json::Value::as_bool).unwrap_or(true);
            if !is_connected || !is_enabled {
                continue;
            }

            let name = out
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
                .to_string();
            let is_primary = out.get("primary").and_then(serde_json::Value::as_bool).unwrap_or(false);

            let x = (out
                .get("pos")
                .and_then(|p| p.get("x"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0))
            .saturating_as::<i32>();
            let y = (out
                .get("pos")
                .and_then(|p| p.get("y"))
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0))
            .saturating_as::<i32>();

            let (width, height, refresh_rate) = if let Some(modes) = out.get("modes").and_then(|m| m.as_array()) {
                let current_mode_id = out.get("currentModeId").and_then(|v| v.as_str()).unwrap_or("");
                let mut found_w = 0u32;
                let mut found_h = 0u32;
                let mut found_rate = 60u32;

                for mode in modes {
                    let m_id = mode.get("id").and_then(|v| v.as_str()).unwrap_or("");
                    if m_id == current_mode_id || current_mode_id.is_empty() {
                        if let Some(size) = mode.get("size") {
                            found_w = size
                                .get("width")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0)
                                .saturating_as::<u32>();
                            found_h = size
                                .get("height")
                                .and_then(serde_json::Value::as_u64)
                                .unwrap_or(0)
                                .saturating_as::<u32>();
                        }
                        if let Some(rate) = mode.get("refreshRate").and_then(serde_json::Value::as_f64) {
                            found_rate = rate.round().to_int::<u32>();
                        }
                        break;
                    }
                }
                (found_w, found_h, found_rate)
            } else {
                let w = (out
                    .get("size")
                    .and_then(|s| s.get("width"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1920))
                .saturating_as::<u32>();
                let h = (out
                    .get("size")
                    .and_then(|s| s.get("height"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(1080))
                .saturating_as::<u32>();
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
        for (idx, (x, y, name, width, height, refresh_rate, is_primary)) in raw_monitors.into_iter().enumerate() {
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

    pub(super) fn detect_via_xrandr_listmonitors() -> Option<Vec<MonitorInfo>> {
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

        if monitors.is_empty() { None } else { Some(monitors) }
    }

    pub(super) fn get_xrandr_rates() -> HashMap<String, u32> {
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
                        rates.insert(conn.clone(), rate_f.round().to_int::<u32>());
                        break;
                    }
                }
            }
        }

        rates
    }

    pub(super) fn detect_via_xrandr() -> Option<Vec<MonitorInfo>> {
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
                if let (Some(conn), Some((x, y, width, height))) = (current_connector.take(), current_geom.take()) {
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
            } else if line.starts_with("   ") && current_connector.is_some() && line.contains('*') {
                for part in line.split_whitespace() {
                    if part.contains('*')
                        && let Ok(rate_f) = part.trim_end_matches(['*', '+']).parse::<f32>()
                    {
                        current_rate = rate_f.round().to_int::<u32>();
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

    pub(super) fn detect_via_wlr_randr() -> Option<Vec<MonitorInfo>> {
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
            let model = out.get("model").and_then(|v| v.as_str()).unwrap_or(&name).to_string();

            let x = out
                .get("x")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
                .saturating_as::<i32>();
            let y = out
                .get("y")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0)
                .saturating_as::<i32>();

            let mut width = 0u32;
            let mut height = 0u32;
            let mut refresh_rate = 60u32;

            if let Some(modes) = out.get("modes").and_then(|m| m.as_array()) {
                for m in modes {
                    let is_current = m.get("current").and_then(serde_json::Value::as_bool).unwrap_or(false);
                    if is_current {
                        width = m
                            .get("width")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0)
                            .saturating_as::<u32>();
                        height = m
                            .get("height")
                            .and_then(serde_json::Value::as_u64)
                            .unwrap_or(0)
                            .saturating_as::<u32>();
                        refresh_rate = m
                            .get("refresh")
                            .and_then(serde_json::Value::as_f64)
                            .map_or(60, |r| r.round().to_int::<u32>());
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

    pub(super) fn detect_via_drm_sysfs() -> Vec<MonitorInfo> {
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
                            && let (Ok(w), Ok(h)) = (parts[0].trim().parse(), parts[1].trim().parse())
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
