//! The compiled channel, shared by the About dialog and the window name. The build script sets
//! `cfg(deckcraft_nightly_v1)` and `DECKCRAFT_BUILT_VERSION` from `DECKCRAFT_BUILD_CHANNEL`, the
//! same switch the app crate's build script reads, so both crates agree in one build.

/// The display name: the same string the Launcher and the macOS bundle use.
pub const fn app_name() -> &'static str {
    if is_nightly() { "DeckCraft Nightly" } else { "DeckCraft" }
}

pub const fn is_nightly() -> bool {
    cfg!(deckcraft_nightly_v1)
}

/// The version the binary was built as: the workspace version for stable, the prepared Nightly
/// feed identity for Nightly (never the bare Cargo version).
pub const fn version() -> &'static str {
    env!("DECKCRAFT_BUILT_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn nightly_and_stable_names_match_the_build_channel() {
        assert_eq!(super::is_nightly(), cfg!(deckcraft_nightly_v1));
        assert_eq!(super::app_name(), if cfg!(deckcraft_nightly_v1) { "DeckCraft Nightly" } else { "DeckCraft" });
        assert!(super::version().starts_with(env!("CARGO_PKG_VERSION").split('-').next().unwrap_or_default()));
    }

    #[test]
    fn a_nightly_build_never_reports_the_bare_workspace_version() {
        if cfg!(deckcraft_nightly_v1) {
            assert_ne!(super::version(), env!("CARGO_PKG_VERSION"));
            assert!(super::version().contains("-nightly."));
        }
    }
}
