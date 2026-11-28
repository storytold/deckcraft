//! Which windowing system to start on, on Linux and the BSDs.
//!
//! In a Wayland session DeckCraft runs natively on Wayland: the patched winit it is built with
//! reports files dragged from the file manager there (they used to need Xwayland). `DECKCRAFT_X11`
//! forces X11 instead. Without a Wayland compositor it starts on X11, and with no display at all it
//! says so plainly.

use std::path::Path;

/// The windowing system to ask winit for.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice {
    X11,
    /// Let winit decide (Wayland when `WAYLAND_DISPLAY` is set).
    Default,
    /// Neither `DISPLAY` nor `WAYLAND_DISPLAY`: there is nothing to open a window on.
    None,
}

/// The X server number of a local `DISPLAY` (`:1`, `:1.0`, `unix:1`); `None` for a remote one
/// (`host:10.0`, as `ssh -X` sets) or one we can't read, which are left to X11 itself.
fn local_display_number(display: &str) -> Option<u32> {
    let rest = display.strip_prefix(':').or_else(|| display.strip_prefix("unix:"))?;
    rest.split('.').next()?.parse().ok()
}

/// Choose from the environment (`DISPLAY`, `WAYLAND_DISPLAY`, and whether X11 is forced).
pub fn choose(display: Option<&str>, wayland: Option<&str>, force_x11: bool) -> Choice {
    let display = display.filter(|d| !d.is_empty());
    let wayland = wayland.is_some_and(|w| !w.is_empty());
    match display {
        None if wayland => Choice::Default,
        None => Choice::None,
        Some(_) if wayland && !force_x11 => Choice::Default,
        Some(_) => Choice::X11,
    }
}

/// [`choose`] for this process.
pub fn choose_from_env() -> Choice {
    let display = std::env::var("DISPLAY").ok();
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    choose(display.as_deref(), wayland.as_deref(), std::env::var_os("DECKCRAFT_X11").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wayland_session_starts_on_wayland_even_with_xwayland() {
        assert_eq!(choose(Some(":0"), Some("wayland-0"), false), Choice::Default);
        assert_eq!(choose(Some(":1.0"), Some("wayland-0"), false), Choice::Default);
        assert_eq!(choose(None, Some("wayland-0"), false), Choice::Default);
        assert_eq!(choose(Some(""), Some("wayland-0"), false), Choice::Default);
    }

    #[test]
    fn x11_is_used_when_forced_or_when_there_is_no_compositor() {
        assert_eq!(choose(Some(":0"), Some("wayland-0"), true), Choice::X11);
        assert_eq!(choose(Some(":0"), None, false), Choice::X11);
        assert_eq!(choose(Some("host:10.0"), None, false), Choice::X11);
        // Forcing X11 with no X display at all is still Wayland rather than failing.
        assert_eq!(choose(None, Some("wayland-0"), true), Choice::Default);
    }

    #[test]
    fn no_display_at_all() {
        assert_eq!(choose(None, None, false), Choice::None);
        assert_eq!(choose(Some(""), Some(""), false), Choice::None);
    }
}
