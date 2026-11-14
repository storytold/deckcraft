//! Which windowing system to start on, on Linux and the BSDs.
//!
//! DeckCraft prefers X11 when `DISPLAY` is set (see `main`), except in a Wayland session
//! (`XDG_SESSION_TYPE=wayland`), where it starts on Wayland: full-screen presenting through
//! Xwayland can show each slide change seconds late. `DECKCRAFT_X11` keeps X11 there, and
//! `DECKCRAFT_WAYLAND` asks for Wayland from any session. A stale `DISPLAY` (left over from
//! an ended `ssh -X` session or a desktop with Xwayland turned off) used to make start-up fail
//! with winit's "failed to open display" even though a Wayland compositor was running. When a
//! local `DISPLAY` names an X server whose socket isn't there and Wayland is available, start on
//! Wayland instead. With no display at all, say so plainly.

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

/// Choose from the environment. `socket_exists` checks an X socket path (the filesystem in
/// [`choose_from_env`], a stub in tests).
pub fn choose(display: Option<&str>, wayland: Option<&str>, force_wayland: bool, socket_exists: impl Fn(&Path) -> bool) -> Choice {
    let display = display.filter(|d| !d.is_empty());
    let wayland = wayland.is_some_and(|w| !w.is_empty());
    match display {
        None if wayland => Choice::Default,
        None => Choice::None,
        Some(_) if force_wayland && wayland => Choice::Default,
        Some(d) => match local_display_number(d) {
            Some(n) if wayland && !socket_exists(&Path::new("/tmp/.X11-unix").join(format!("X{n}"))) => {
                log::warn!("DISPLAY={d} has no X server; starting on Wayland");
                Choice::Default
            }
            _ => Choice::X11,
        },
    }
}

/// Whether to start on Wayland although `DISPLAY` is set: asked for with `DECKCRAFT_WAYLAND`, or
/// the session is a Wayland one and `DECKCRAFT_X11` doesn't say otherwise.
fn prefers_wayland(force: bool, session_type: Option<&str>, force_x11: bool) -> bool {
    force || (!force_x11 && session_type == Some("wayland"))
}

/// [`choose`] for this process.
pub fn choose_from_env() -> Choice {
    let display = std::env::var("DISPLAY").ok();
    let wayland = std::env::var("WAYLAND_DISPLAY").ok();
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    let prefer = prefers_wayland(
        std::env::var_os("DECKCRAFT_WAYLAND").is_some(),
        session.as_deref(),
        std::env::var_os("DECKCRAFT_X11").is_some(),
    );
    choose(display.as_deref(), wayland.as_deref(), prefer, |p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stale_local_display_falls_back_to_wayland() {
        let none = |_: &Path| false;
        let all = |_: &Path| true;
        assert_eq!(choose(Some(":1"), Some("wayland-0"), false, none), Choice::Default);
        assert_eq!(choose(Some(":1.0"), Some("wayland-0"), false, none), Choice::Default);
        assert_eq!(choose(Some("unix:0"), Some("wayland-0"), false, none), Choice::Default);
        // A live X server, no Wayland, or a remote DISPLAY: X11 as before.
        assert_eq!(choose(Some(":1"), Some("wayland-0"), false, all), Choice::X11);
        assert_eq!(choose(Some(":1"), None, false, none), Choice::X11);
        assert_eq!(choose(Some("host:10.0"), Some("wayland-0"), false, none), Choice::X11);
        // The socket checked is the display's own.
        assert_eq!(choose(Some(":3"), Some("w"), false, |p: &Path| p.ends_with("X3")), Choice::X11);
        assert_eq!(choose(Some(":3"), Some("w"), false, |p: &Path| p.ends_with("X0")), Choice::Default);
    }

    #[test]
    fn wayland_only_forced_and_no_display_at_all() {
        let all = |_: &Path| true;
        assert_eq!(choose(None, Some("wayland-0"), false, all), Choice::Default);
        assert_eq!(choose(Some(""), Some("wayland-0"), false, all), Choice::Default);
        assert_eq!(choose(Some(":0"), Some("wayland-0"), true, all), Choice::Default);
        // Forcing Wayland without a compositor keeps X11 rather than failing.
        assert_eq!(choose(Some(":0"), None, true, all), Choice::X11);
        assert_eq!(choose(None, None, false, all), Choice::None);
        assert_eq!(choose(Some(""), Some(""), false, all), Choice::None);
    }

    #[test]
    fn a_wayland_session_starts_on_wayland_unless_x11_is_asked_for() {
        assert!(prefers_wayland(false, Some("wayland"), false));
        assert!(!prefers_wayland(false, Some("wayland"), true));
        assert!(!prefers_wayland(false, Some("x11"), false));
        assert!(!prefers_wayland(false, None, false));
        assert!(prefers_wayland(true, Some("x11"), false));
        assert!(prefers_wayland(true, Some("wayland"), true));
    }
}
