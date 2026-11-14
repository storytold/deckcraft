//! Which windowing system to start on, on Linux and the BSDs.
//!
//! In a Wayland session DeckCraft starts on Wayland, even when Xwayland also sets `DISPLAY`.
//! Under Xwayland the compositor can ignore where DeckCraft puts its windows: a slide show's
//! slides stayed on the presenter's screen, over Presenter View, and the covered window stalled
//! the show. A stale `DISPLAY` (an ended `ssh -X` session, Xwayland turned off) no longer matters
//! either. `DECKCRAFT_X11` asks for X11 anyway; without Wayland, X11 is used. With no display at
//! all, say so plainly.

/// The windowing system to ask winit for.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice {
    X11,
    /// Let winit decide (Wayland when `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` is set).
    Default,
    /// Neither `DISPLAY` nor Wayland: there is nothing to open a window on.
    None,
}

/// Choose from the environment: `DISPLAY`, the Wayland connection ([`wayland`]) and whether X11
/// is asked for.
pub fn choose(display: Option<&str>, wayland: Option<&str>, force_x11: bool) -> Choice {
    let display = display.is_some_and(|d| !d.is_empty());
    let wayland = wayland.is_some_and(|w| !w.is_empty());
    match (display, wayland) {
        (true, true) if force_x11 => Choice::X11,
        (_, true) => Choice::Default,
        (true, false) => Choice::X11,
        (false, false) => Choice::None,
    }
}

/// The Wayland connection winit would use: `WAYLAND_DISPLAY`, or else `WAYLAND_SOCKET` (a
/// connection handed over by whoever started DeckCraft).
fn wayland(display: Option<String>, socket: Option<String>) -> Option<String> {
    display.filter(|d| !d.is_empty()).or(socket)
}

/// [`choose`] for this process.
pub fn choose_from_env() -> Choice {
    let display = std::env::var("DISPLAY").ok();
    let wayland = wayland(std::env::var("WAYLAND_DISPLAY").ok(), std::env::var("WAYLAND_SOCKET").ok());
    choose(display.as_deref(), wayland.as_deref(), std::env::var_os("DECKCRAFT_X11").is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wayland_session_starts_on_wayland() {
        // Xwayland's DISPLAY, live or stale, doesn't take it over.
        assert_eq!(choose(Some(":0"), Some("wayland-0"), false), Choice::Default);
        assert_eq!(choose(Some(":1.0"), Some("wayland-1"), false), Choice::Default);
        assert_eq!(choose(None, Some("wayland-0"), false), Choice::Default);
        assert_eq!(choose(Some(""), Some("wayland-0"), false), Choice::Default);
    }

    #[test]
    fn a_wayland_socket_is_a_wayland_session() {
        let socket = wayland(None, Some("5".into()));
        assert_eq!(choose(Some(":0"), socket.as_deref(), false), Choice::Default);
        assert_eq!(wayland(Some(String::new()), Some("5".into())).as_deref(), Some("5"));
        assert_eq!(wayland(Some("wayland-0".into()), Some("5".into())).as_deref(), Some("wayland-0"));
        assert_eq!(wayland(None, None), None);
    }

    #[test]
    fn x11_when_asked_or_alone_and_nothing_without_a_display() {
        assert_eq!(choose(Some(":0"), Some("wayland-0"), true), Choice::X11);
        assert_eq!(choose(Some(":0"), None, false), Choice::X11);
        assert_eq!(choose(Some("host:10.0"), Some(""), false), Choice::X11);
        // Asking for X11 without an X server keeps Wayland rather than failing.
        assert_eq!(choose(None, Some("wayland-0"), true), Choice::Default);
        assert_eq!(choose(None, None, false), Choice::None);
        assert_eq!(choose(Some(""), Some(""), true), Choice::None);
    }
}
