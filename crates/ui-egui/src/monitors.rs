//! Multi-monitor detection and presentation viewport coordinate calculation and routing.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub name: String,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Query connected monitors using `xrandr --listmonitors`.
pub fn detect_monitors() -> Vec<MonitorInfo> {
    let mut monitors = Vec::new();
    #[cfg(all(unix, not(target_os = "macos")))]
        match std::process::Command::new("xrandr").arg("--listmonitors").output() {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines().skip(1) {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        let name = parts.last().unwrap_or(&"").to_string();
                        let is_primary = parts[1].contains('+') && parts[1].contains('*');
                        let geom = parts[2]; // e.g. "1920/340x1080/190+0+0" or "1920/1150x1080/650+1920+0"
                        if let Some((w, h, x, y)) = parse_geometry(geom) {
                            monitors.push(MonitorInfo {
                                name,
                                primary: is_primary,
                                x,
                                y,
                                width: w,
                                height: h,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    if monitors.is_empty() {
        monitors.push(MonitorInfo {
            name: "Primary".to_string(),
            primary: true,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        });
    }
    monitors
}

pub fn has_second_display() -> bool {
    let monitors = detect_monitors();
    if monitors.len() > 1 {
        return true;
    }
    let primary = get_primary_monitor(&monitors);
    monitors.iter().any(|m| m.name != primary.name || m.x != primary.x || m.y != primary.y)
}

pub fn get_primary_monitor(monitors: &[MonitorInfo]) -> MonitorInfo {
    monitors
        .iter()
        .find(|m| m.primary)
        .or_else(|| monitors.iter().find(|m| m.x == 0 && m.y == 0))
        .or_else(|| monitors.first())
        .cloned()
        .unwrap_or(MonitorInfo {
            name: "Primary".into(),
            primary: true,
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        })
}

pub fn get_extended_monitor(monitors: &[MonitorInfo], primary: &MonitorInfo) -> Option<MonitorInfo> {
    monitors
        .iter()
        .find(|m| m.name != primary.name && (m.x != primary.x || m.y != primary.y || !m.primary))
        .cloned()
}

pub fn parse_geometry(geom: &str) -> Option<(u32, u32, i32, i32)> {
    // geom e.g. "1920/340x1080/190+1920+0" or "1920x1080+0+0"
    let plus_idx = geom.find('+')?;
    let wh = &geom[..plus_idx];
    let offset = &geom[plus_idx + 1..];

    let mut offset_parts = offset.split('+');
    let x: i32 = offset_parts.next()?.parse().ok()?;
    let y: i32 = offset_parts.next()?.parse().ok()?;

    let mut wh_parts = wh.split('x');
    let w_part = wh_parts.next()?.split('/').next()?;
    let h_part = wh_parts.next()?.split('/').next()?;
    let w: u32 = w_part.parse().ok()?;
    let h: u32 = h_part.parse().ok()?;
    Some((w, h, x, y))
}

/// Human-friendly display label for a monitor (e.g. "eDP-1: 1920×1080 (Primary)").
pub fn format_monitor_label(m: &MonitorInfo) -> String {
    let role = if m.primary { "Primary" } else { "Extended" };
    format!("{}: {}×{} ({role})", m.name, m.width, m.height)
}

/// Resolves which monitor gets the audience Slide Show and which gets Presenter View using the given monitors slice.
/// Returns `(audience_monitor, presenter_monitor)`.
pub fn get_active_routing_from(monitors: &[MonitorInfo], choice: &str, swapped: bool) -> (MonitorInfo, MonitorInfo) {
    let primary = get_primary_monitor(monitors);
    let extended = get_extended_monitor(monitors, &primary);

    let (mut audience_mon, mut presenter_mon) = if let Some(chosen) = monitors.iter().find(|m| m.name.eq_ignore_ascii_case(choice)) {
        let other = if chosen.name == primary.name {
            extended.clone().unwrap_or_else(|| primary.clone())
        } else {
            primary.clone()
        };
        (chosen.clone(), other)
    } else {
        match choice {
            "primary" => (primary.clone(), extended.clone().unwrap_or_else(|| primary.clone())),
            "extended" => {
                if let Some(ext) = extended.clone() {
                    (ext, primary.clone())
                } else {
                    (primary.clone(), primary.clone())
                }
            }
            _ => {
                // "auto" (default): if extended exists, audience goes to extended, presenter stays on primary
                if let Some(ext) = extended.clone() {
                    (ext, primary.clone())
                } else {
                    (primary.clone(), primary.clone())
                }
            }
        }
    };

    if swapped {
        std::mem::swap(&mut audience_mon, &mut presenter_mon);
    }

    (audience_mon, presenter_mon)
}

/// Resolves which monitor gets the audience Slide Show and which gets Presenter View.
/// Returns `(audience_monitor, presenter_monitor)`.
pub fn get_active_routing(choice: &str, swapped: bool) -> (MonitorInfo, MonitorInfo) {
    let monitors = detect_monitors();
    get_active_routing_from(&monitors, choice, swapped)
}

/// Calculate audience and presenter window coordinates using a specific monitor list.
pub fn presentation_placement_from(monitors: &[MonitorInfo], choice: &str, swapped: bool) -> (egui::Pos2, egui::Pos2) {
    let (aud, pres) = get_active_routing_from(monitors, choice, swapped);
    (
        egui::pos2(aud.x as f32, aud.y as f32),
        egui::pos2(pres.x as f32, pres.y as f32),
    )
}

/// Calculate audience and presenter window coordinates based on presentation_monitor setting and swapped state.
/// Returns (audience_pos, presenter_pos).
pub fn presentation_placement(choice: &str, swapped: bool) -> (egui::Pos2, egui::Pos2) {
    let monitors = detect_monitors();
    presentation_placement_from(&monitors, choice, swapped)
}

/// Returns whether programmatic window routing is supported on this platform.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn can_route_windows() -> bool {
    std::process::Command::new("xdotool")
        .arg("-v")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Returns whether programmatic window routing is supported on this platform.
#[cfg(not(all(unix, not(target_os = "macos"))))]
pub fn can_route_windows() -> bool {
    true
}

use std::sync::atomic::{AtomicU64, Ordering};

static ROUTING_GENERATION: AtomicU64 = AtomicU64::new(0);

/// Invalidate any pending routing workers so they exit immediately without moving windows.
pub fn cancel_pending_routing() {
    ROUTING_GENERATION.fetch_add(1, Ordering::SeqCst);
}

/// Spawns a background thread to route audience and presenter windows on Linux X11/Xwayland.
/// Retries progressively for 1.5 seconds so newly-opened viewports are captured and positioned.
pub fn route_presentation(choice: &str, swapped: bool) {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let choice_owned = choice.to_string();
        let generation = ROUTING_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::spawn(move || {
            for attempt in 0..15 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                if ROUTING_GENERATION.load(Ordering::SeqCst) != generation {
                    break;
                }
                let done = do_route(&choice_owned, swapped);
                if done && attempt >= 2 {
                    break;
                }
            }
        });
    }
}

/// Restores the main DeckCraft editor window back to the primary display when presentation ends.
pub fn restore_editor_window() {
    cancel_pending_routing();
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let audience_wids = get_window_ids("DeckCraft");
            let presenter_wids = get_window_ids("Presenter View");
            for wid in audience_wids {
                if !presenter_wids.contains(&wid) {
                    let _ = std::process::Command::new("xdotool")
                        .args(["windowmove", &wid, "50", "50"])
                        .output();
                }
            }
        });
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn get_window_ids(name_pattern: &str) -> Vec<String> {
    let pid = std::process::id().to_string();
    match std::process::Command::new("xdotool")
        .args(["search", "--all", "--onlyvisible", "--pid", &pid, "--name", name_pattern])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
        }
        _ => vec![],
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn do_route(choice: &str, swapped: bool) -> bool {
    let monitors = detect_monitors();
    let primary = get_primary_monitor(&monitors);
    let extended = get_extended_monitor(&monitors, &primary);

    // If only one monitor exists, keep normal single-monitor placement
    if extended.is_none() && monitors.len() <= 1 {
        return true;
    }

    let (audience_screen, presenter_screen) = get_active_routing(choice, swapped);

    let presenter_wids = get_window_ids("Presenter View");
    let all_deckcraft = get_window_ids("DeckCraft");
    let audience_wids: Vec<String> = all_deckcraft.into_iter().filter(|w| !presenter_wids.contains(w)).collect();

    let mut routed_any = false;

    // 1. Route Audience window to target screen
    for wid in &audience_wids {
        let _ = std::process::Command::new("xdotool")
            .args(["windowmove", wid, &audience_screen.x.to_string(), &audience_screen.y.to_string()])
            .output();
        routed_any = true;
    }

    // 2. Route Presenter View window to target screen
    for wid in &presenter_wids {
        let _ = std::process::Command::new("xdotool")
            .args(["windowmove", wid, &presenter_screen.x.to_string(), &presenter_screen.y.to_string()])
            .output();
        let _ = std::process::Command::new("xdotool")
            .args(["windowsize", wid, &presenter_screen.width.to_string(), &presenter_screen.height.to_string()])
            .output();
        let _ = std::process::Command::new("xdotool")
            .args(["windowactivate", wid])
            .output();
        routed_any = true;
    }

    routed_any && !presenter_wids.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_geometry() {
        assert_eq!(parse_geometry("1920/340x1080/190+0+0"), Some((1920, 1080, 0, 0)));
        assert_eq!(parse_geometry("1920/1150x1080/650+1920+0"), Some((1920, 1080, 1920, 0)));
        assert_eq!(parse_geometry("1920x1080+1920+1080"), Some((1920, 1080, 1920, 1080)));
    }

    #[test]
    fn test_monitors_detection_fallback() {
        let monitors = detect_monitors();
        assert!(!monitors.is_empty());
        let primary = get_primary_monitor(&monitors);
        assert!(!primary.name.is_empty());
    }

    #[test]
    fn test_format_monitor_label() {
        let m = MonitorInfo {
            name: "HDMI-1".into(),
            primary: false,
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        };
        let label = format_monitor_label(&m);
        assert!(label.contains("HDMI-1"));
        assert!(label.contains("1920×1080"));
        assert!(label.contains("Extended"));
    }

    #[test]
    fn test_get_active_routing() {
        let (aud, pres) = get_active_routing("auto", false);
        let (swapped_aud, swapped_pres) = get_active_routing("auto", true);
        assert_eq!(aud.name, swapped_pres.name);
        assert_eq!(pres.name, swapped_aud.name);
    }
}
