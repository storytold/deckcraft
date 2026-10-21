//! The operating system's light or dark setting, for Preferences ▸ Dark appearance ▸ Use system
//! setting.
//!
//! winit's report of it can't be used: on Linux and the BSDs it has none, and on macOS it gives the
//! window's own appearance, which DeckCraft sets. So DeckCraft reads the setting where each system
//! keeps it:
//!
//! - Windows: Settings ▸ Personalization ▸ Colors ▸ Choose your default app mode.
//! - macOS: System Settings ▸ Appearance (Auto included).
//! - Linux and the BSDs: the desktop's colour scheme, through the XDG desktop portal that current
//!   desktops provide, else KDE's or GNOME's own settings.
//!
//! [`Watcher`] reads it before the first frame, so the window opens in the right appearance, then
//! every [`RECHECK`] on a background thread, so DeckCraft follows a change made while it runs.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use egui::Theme;

/// How often the setting is read again.
const RECHECK: Duration = Duration::from_secs(2);

/// The system's setting, kept current by a background thread.
pub struct Watcher {
    latest: Arc<Mutex<Option<Theme>>>,
}

impl Watcher {
    /// Read the setting now, then every [`RECHECK`]. `ctx` is repainted when it changes, so an idle
    /// window follows too.
    pub fn start(ctx: egui::Context) -> Watcher {
        let first = read();
        log::info!("system appearance: {first:?}");
        let latest = Arc::new(Mutex::new(first));
        let shared = Arc::clone(&latest);
        let spawned = std::thread::Builder::new().name("system-appearance".into()).spawn(move || {
            loop {
                std::thread::sleep(RECHECK);
                let now = read();
                let before = std::mem::replace(&mut *shared.lock().unwrap_or_else(PoisonError::into_inner), now);
                if before != now {
                    log::info!("system appearance changed: {now:?}");
                    ctx.request_repaint();
                }
            }
        });
        if let Err(e) = spawned {
            log::warn!("system appearance: changes won't be followed ({e})");
        }
        Watcher { latest }
    }

    /// The system's setting; `None` when it can't be read.
    pub fn get(&self) -> Option<Theme> {
        *self.latest.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The system's setting now; `None` when it can't be read.
fn read() -> Option<Theme> {
    if cfg!(windows) {
        windows::read()
    } else if cfg!(target_os = "macos") {
        macos::read()
    } else {
        freedesktop::read()
    }
}

/// What `program args…` printed, when it ran and succeeded.
fn output_of(program: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(program).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Windows: Settings ▸ Personalization ▸ Colors ▸ Choose your default app mode.
mod windows {
    use egui::Theme;

    /// `AppsUseLightTheme` is 0 for dark and 1 for light. Windows versions without a dark mode
    /// don't have it.
    #[cfg(windows)]
    pub fn read() -> Option<Theme> {
        let key = windows_registry::CURRENT_USER.open(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize").ok()?;
        let light = key.get_u32("AppsUseLightTheme").ok()?;
        Some(if light == 0 { Theme::Dark } else { Theme::Light })
    }

    /// Only Windows has a registry.
    #[cfg(not(windows))]
    pub fn read() -> Option<Theme> {
        None
    }
}

/// macOS: System Settings ▸ Appearance.
mod macos {
    use egui::Theme;

    pub fn read() -> Option<Theme> {
        let style = super::output_of("defaults", &["read", "-g", "AppleInterfaceStyle"]);
        Some(from_interface_style(style.as_deref()))
    }

    /// `AppleInterfaceStyle` is "Dark" while the system is dark, Auto included; it is absent
    /// (so `defaults` fails) while the system is light.
    pub(super) fn from_interface_style(style: Option<&str>) -> Theme {
        if style.is_some_and(|s| s.trim() == "Dark") { Theme::Dark } else { Theme::Light }
    }
}

/// Linux and the BSDs.
mod freedesktop {
    use std::path::PathBuf;

    use egui::Theme;

    use super::output_of;

    /// The XDG desktop portal first: it is the standard, answered by GNOME 42+, KDE Plasma 5.24+
    /// and most other current desktops. On a desktop without one, or when it has no preference,
    /// the desktop's own settings: KDE's or GNOME's.
    pub fn read() -> Option<Theme> {
        portal().or_else(|| if on_kde() { kde() } else { gnome() })
    }

    /// The portal's `org.freedesktop.appearance` `color-scheme`.
    fn portal() -> Option<Theme> {
        let reply = output_of(
            "gdbus",
            &[
                "call",
                "--session",
                "--timeout",
                "1",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                "org.freedesktop.portal.Settings.Read",
                "org.freedesktop.appearance",
                "color-scheme",
            ],
        )?;
        from_portal_reply(&reply)
    }

    /// gdbus prints the scheme as a `uint32` (`(<<uint32 1>>,)`): 1 prefers dark, 2 prefers
    /// light, and 0 has no preference.
    pub(super) fn from_portal_reply(reply: &str) -> Option<Theme> {
        let (_, after) = reply.split_once("uint32 ")?;
        let number: String = after.chars().take_while(char::is_ascii_digit).collect();
        match number.as_str() {
            "1" => Some(Theme::Dark),
            "2" => Some(Theme::Light),
            _ => None,
        }
    }

    fn on_kde() -> bool {
        std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|desktops| desktops.split(':').any(|d| d.eq_ignore_ascii_case("KDE")))
    }

    /// KDE's colour scheme, kept in `kdeglobals` in the user's config folder.
    fn kde() -> Option<Theme> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        from_kdeglobals(&std::fs::read_to_string(config.join("kdeglobals")).ok()?)
    }

    /// A colour scheme is dark when its window background is: `BackgroundNormal` under
    /// `[Colors:Window]`, `32,35,38` in Breeze Dark and `239,240,241` in Breeze.
    pub(super) fn from_kdeglobals(text: &str) -> Option<Theme> {
        let mut in_window_colors = false;
        for line in text.lines().map(str::trim) {
            if line.starts_with('[') {
                in_window_colors = line == "[Colors:Window]";
            } else if in_window_colors && let Some(rgb) = line.strip_prefix("BackgroundNormal=") {
                let mut channels = rgb.split(',').map(|c| c.trim().parse::<f32>().ok());
                let (r, g, b) = (channels.next()??, channels.next()??, channels.next()??);
                let luminance = (0.299 * r + 0.587 * g + 0.114 * b) / 255.0;
                return Some(if luminance < 0.5 { Theme::Dark } else { Theme::Light });
            }
        }
        None
    }

    /// GNOME's settings: `color-scheme` (GNOME 42+), then, for older versions or no preference,
    /// whether the GTK theme is a dark one.
    fn gnome() -> Option<Theme> {
        let get = |key| output_of("gsettings", &["get", "org.gnome.desktop.interface", key]);
        get("color-scheme").and_then(|v| from_gnome_color_scheme(&v)).or_else(|| get("gtk-theme").map(|v| from_gtk_theme(&v)))
    }

    /// `'prefer-dark'`, `'prefer-light'`, or `'default'` for no preference.
    pub(super) fn from_gnome_color_scheme(value: &str) -> Option<Theme> {
        match value.trim().trim_matches('\'') {
            "prefer-dark" => Some(Theme::Dark),
            "prefer-light" => Some(Theme::Light),
            _ => None,
        }
    }

    /// Dark GTK themes say so in their names: `'Adwaita-dark'`, `'Yaru-dark'`, `'Arc-Dark'`.
    pub(super) fn from_gtk_theme(name: &str) -> Theme {
        if name.to_ascii_lowercase().contains("dark") { Theme::Dark } else { Theme::Light }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_macos_interface_style() {
        assert_eq!(macos::from_interface_style(Some("Dark\n")), Theme::Dark);
        assert_eq!(macos::from_interface_style(None), Theme::Light, "absent while light");
    }

    #[test]
    fn reads_the_portal_color_scheme() {
        assert_eq!(freedesktop::from_portal_reply("(<<uint32 1>>,)\n"), Some(Theme::Dark));
        assert_eq!(freedesktop::from_portal_reply("(<<uint32 2>>,)\n"), Some(Theme::Light));
        assert_eq!(freedesktop::from_portal_reply("(<<uint32 0>>,)\n"), None, "no preference");
        assert_eq!(freedesktop::from_portal_reply("(<uint32 1>,)\n"), Some(Theme::Dark), "ReadOne's single wrapping");
        assert_eq!(freedesktop::from_portal_reply("(<<uint32 12>>,)\n"), None);
        assert_eq!(freedesktop::from_portal_reply(""), None);
    }

    #[test]
    fn reads_gnome_settings() {
        assert_eq!(freedesktop::from_gnome_color_scheme("'prefer-dark'\n"), Some(Theme::Dark));
        assert_eq!(freedesktop::from_gnome_color_scheme("'prefer-light'\n"), Some(Theme::Light));
        assert_eq!(freedesktop::from_gnome_color_scheme("'default'\n"), None);
        assert_eq!(freedesktop::from_gtk_theme("'Adwaita-dark'\n"), Theme::Dark);
        assert_eq!(freedesktop::from_gtk_theme("'Arc-Dark'\n"), Theme::Dark);
        assert_eq!(freedesktop::from_gtk_theme("'Adwaita'\n"), Theme::Light);
    }

    #[test]
    fn reads_the_kde_color_scheme() {
        let breeze =
            |rgb: &str| format!("[General]\nColorScheme=Mine\n\n[Colors:View]\nBackgroundNormal=0,0,0\n\n[Colors:Window]\nBackgroundNormal={rgb}\n");
        assert_eq!(freedesktop::from_kdeglobals(&breeze("32,35,38")), Some(Theme::Dark));
        assert_eq!(freedesktop::from_kdeglobals(&breeze("239,240,241")), Some(Theme::Light));
        assert_eq!(freedesktop::from_kdeglobals(&breeze("not a colour")), None);
        assert_eq!(freedesktop::from_kdeglobals("[General]\nColorScheme=BreezeDark\n"), None, "no window colours");
    }
}
