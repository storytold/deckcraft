//! The system's animation setting, which Preferences ▸ Animation effects follows while it is set
//! to Use system setting.
//!
//! - Windows: Settings ▸ Accessibility ▸ Visual effects ▸ Animation effects ("Show animations in
//!   Windows" in the Control Panel), read through the Windows Runtime's `UISettings`.
//! - macOS: System Settings ▸ Accessibility ▸ Display ▸ Reduce motion, read through `NSWorkspace`.
//! - Linux and the BSDs: the desktop's animations setting ([`desktop`]).
//!
//! The UI asks every frame, so the answer always comes from memory: Windows and macOS are asked
//! again at most once a second, and the Linux/BSD answer is kept current by a thread of its own.
//! All three are safe calls (the workspace forbids `unsafe`, which the Win32 call would need).

// Built for tests on every platform, so its parsing is tested everywhere.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
mod desktop;

/// Whether the system has animation effects on (`None` while it doesn't say), for
/// `Services::animations_enabled`. `ctx` repaints when an answer that arrives later changes it.
pub fn animations_enabled(ctx: &egui::Context) -> Box<dyn Fn() -> Option<bool>> {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let _ = ctx;
        Box::new(remembered(logged(ask_system), REFRESH))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Box::new(desktop::watch(ctx.clone()))
    }
    #[cfg(not(any(target_os = "windows", unix)))]
    {
        let _ = ctx;
        Box::new(|| None)
    }
}

/// How long an answer from Windows or macOS is kept before the system is asked again.
#[cfg(any(target_os = "windows", target_os = "macos"))]
const REFRESH: std::time::Duration = std::time::Duration::from_secs(1);

/// `read`, asked at most once per `every` and answered from memory in between.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn remembered(read: impl Fn() -> Option<bool>, every: std::time::Duration) -> impl Fn() -> Option<bool> {
    let last = std::cell::Cell::new(None::<(std::time::Instant, Option<bool>)>);
    move || {
        if let Some((at, answer)) = last.get()
            && at.elapsed() < every
        {
            return answer;
        }
        let answer = read();
        last.set(Some((std::time::Instant::now(), answer)));
        answer
    }
}

/// `read`, with its answer logged whenever it changes: the line that tells whether Use system
/// setting found a setting to follow.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
fn logged(read: impl Fn() -> Option<bool>) -> impl Fn() -> Option<bool> {
    let last = std::cell::Cell::new(None::<Option<bool>>);
    move || {
        let answer = read();
        if last.replace(Some(answer)) != Some(answer) {
            log_answer(answer);
        }
        answer
    }
}

fn log_answer(answer: Option<bool>) {
    let said = match answer {
        Some(true) => "on",
        Some(false) => "off",
        None => "not reported",
    };
    log::info!("system animation effects: {said}");
}

#[cfg(target_os = "windows")]
fn ask_system() -> Option<bool> {
    ui_settings_animations().map_err(|e| log::debug!("system animation effects: {e}")).ok()
}

#[cfg(target_os = "windows")]
fn ui_settings_animations() -> windows::core::Result<bool> {
    windows::UI::ViewManagement::UISettings::new()?.AnimationsEnabled()
}

#[cfg(target_os = "macos")]
fn ask_system() -> Option<bool> {
    // Animations play while Reduce motion is off.
    Some(!objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::time::Duration;

    use super::{logged, remembered};

    /// The UI asks every frame; the system is asked once per interval and the answer kept between.
    #[test]
    fn the_system_is_asked_once_per_interval() {
        let asked = Cell::new(0);
        let system = Cell::new(Some(false));
        let read = || {
            asked.set(asked.get() + 1);
            system.get()
        };
        let kept = remembered(read, Duration::from_secs(3600));
        assert_eq!((kept(), kept(), kept()), (Some(false), Some(false), Some(false)));
        system.set(Some(true));
        assert_eq!(kept(), Some(false), "within the interval the remembered answer stands");
        assert_eq!(asked.get(), 1);
        // Once the interval has passed (here: at once) the system is asked again.
        let fresh = remembered(read, Duration::ZERO);
        assert_eq!(fresh(), Some(true));
        system.set(None);
        assert_eq!(fresh(), None);
        assert_eq!(asked.get(), 3);
    }

    /// Logging the answer never changes it.
    #[test]
    fn a_logged_answer_passes_through() {
        let system = Cell::new(None);
        let read = logged(|| system.get());
        assert_eq!(read(), None);
        for answer in [Some(false), Some(false), Some(true), None] {
            system.set(answer);
            assert_eq!(read(), answer);
        }
    }

    /// The Animation effects choice saved in the registry, or `None` when `reg` can't read it.
    /// Windows keeps its on/off system parameters as the bits of `UserPreferencesMask`: bit n is
    /// the parameter `SPI_GET…` 0x1000 + 2n, and client-area animation is 0x1042, so bit 33.
    #[cfg(target_os = "windows")]
    fn saved_choice() -> Option<bool> {
        let out = std::process::Command::new("reg").args(["query", r"HKCU\Control Panel\Desktop", "/v", "UserPreferencesMask"]).output().ok()?;
        if !out.status.success() {
            return None;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let hex = text.split_whitespace().skip_while(|word| *word != "REG_BINARY").nth(1)?;
        let byte = u8::from_str_radix(hex.get(8..10)?, 16).ok()?;
        Some(byte & 0b10 != 0)
    }

    /// On this machine, the answer is the one Settings ▸ Accessibility ▸ Visual effects shows.
    #[test]
    #[cfg(target_os = "windows")]
    fn the_answer_is_windows_animation_effects_setting() {
        match super::ui_settings_animations() {
            Ok(enabled) => {
                assert_eq!(super::ask_system(), Some(enabled));
                // Only this way round: the session's own value may be off for other reasons (a
                // policy, a remote session) while the saved choice is on.
                if saved_choice() == Some(false) {
                    assert!(!enabled, "Animation effects are off in Settings, but the answer was that animations are on");
                }
            }
            Err(e) => {
                eprintln!("UISettings isn't available here ({e}); not compared with the saved choice");
                assert_eq!(super::ask_system(), None);
            }
        }
    }
}
