//! Insert ▸ Screenshot on macOS: open windows via `xcap`, and Screen Clipping via `screencapture`.
//!
//! Both run off the UI thread so a slow grab or the clipping drag doesn't freeze the window.
//! A failure (Screen Recording permission, a window that can't be grabbed) is a `String` for the
//! status bar — never a panic.

use std::sync::Arc;

use deckcraft_ui_egui::WindowShot;

const MAX_EDGE: u32 = 1600;
const MAX_WINDOWS: usize = 16;

const PERMISSION: &str = "Allow DeckCraft in System Settings, Privacy & Security, Screen Recording, then open Screenshot again.";

/// Open windows as PNG screenshots, front-most first. DeckCraft's own windows are skipped.
pub fn windows() -> Result<Vec<WindowShot>, String> {
    let all = xcap::Window::all().map_err(|e| e.to_string())?;
    log::info!("screenshot: {} windows listed", all.len());
    let mine = std::process::id();
    let mut shots = Vec::new();
    let mut last_err: Option<String> = None;
    for w in all {
        if shots.len() >= MAX_WINDOWS {
            break;
        }
        let title = w.title().unwrap_or_default();
        let app = w.app_name().unwrap_or_default();
        let minimized = w.is_minimized().unwrap_or(false);
        let size = match (w.width(), w.height()) {
            (Ok(width), Ok(height)) => Some((width, height)),
            _ => None,
        };
        // A failed minimized query is not a reason to hide the window.
        if w.pid().ok() == Some(mine)
            || minimized
            || title.trim().is_empty()
            || skip_chrome(&app)
            || skip_chrome(title.trim())
            || size.is_none_or(|(width, height)| width < 80 || height < 80)
        {
            log::debug!("screenshot skip title={title:?} app={app:?} minimized={minimized} size={size:?}");
            continue;
        }
        let title = title.trim();
        match w.capture_image() {
            Ok(img) => match encode_png(img) {
                Ok(png) => shots.push(WindowShot { title: title.to_string(), png: Arc::from(png) }),
                Err(e) => last_err = Some(e),
            },
            Err(e) => last_err = Some(e.to_string()),
        }
    }
    if shots.is_empty() {
        if let Some(e) = last_err {
            log::warn!("window capture: {e}");
            return Err(e);
        }
        // Without Screen Recording permission the system list is empty (other apps' windows
        // are not shareable), which looks the same as a desktop with nothing else open.
        return Err(format!("No windows to capture. {PERMISSION}"));
    }
    Ok(shots)
}

/// Menu bar, Dock and other chrome that isn't a document window.
fn skip_chrome(name: &str) -> bool {
    matches!(
        name,
        "Dock" | "Window Server" | "SystemUIServer" | "Control Center" | "Notification Center" | "WindowManager" | "Wallpaper" | "Spotlight"
    )
}

/// Drag a rectangle. `Ok(None)` when the user cancels (Escape).
pub fn screen_clip() -> Result<Option<Vec<u8>>, String> {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let path = std::env::temp_dir().join(format!("deckcraft-clip-{}-{nanos}.png", std::process::id()));
    let output = std::process::Command::new("screencapture")
        .args(["-i", "-x"])
        .arg(&path)
        .output()
        .map_err(|e| format!("Screen Clipping couldn't start: {e}"))?;
    let bytes = std::fs::read(&path).ok();
    let _ = std::fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let Some(bytes) = bytes.filter(|b| !b.is_empty() && b.starts_with(b"\x89PNG\r\n\x1a\n")) else {
        if stderr.contains("could not create image") || stderr.to_lowercase().contains("permission") {
            return Err(format!("Screen Clipping needs Screen Recording. {PERMISSION}"));
        }
        // Escape cancels the clipping and leaves no file.
        if output.status.success() {
            return Err("Screen Clipping didn't produce a picture".into());
        }
        return Ok(None);
    };
    let img = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.into_rgba8();
    encode_png(img).map(Some)
}

fn encode_png(img: image::RgbaImage) -> Result<Vec<u8>, String> {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err("empty capture".into());
    }
    let img = if w > MAX_EDGE || h > MAX_EDGE {
        let (nw, nh) = if w >= h {
            let nh = (u64::from(h) * u64::from(MAX_EDGE) / u64::from(w)).max(1);
            (MAX_EDGE, u32::try_from(nh).unwrap_or(MAX_EDGE))
        } else {
            let nw = (u64::from(w) * u64::from(MAX_EDGE) / u64::from(h)).max(1);
            (u32::try_from(nw).unwrap_or(MAX_EDGE), MAX_EDGE)
        };
        image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out)
}
