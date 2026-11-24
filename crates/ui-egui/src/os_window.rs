//! The app id every DeckCraft OS window carries, so docks show the DeckCraft icon on them and
//! Wayland compositors' window rules can match them.

/// Matches the `.desktop` file and hicolor icon name, so Wayland docks pick up the icon.
pub const APP_ID: &str = "ai.storyteller.deckcraft";

/// Immutable startup identity. The desktop selects it before any UI service is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchIdentity {
    Stable,
    NightlyV1,
}

static IDENTITY: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn select_identity(identity: LaunchIdentity) -> Result<(), String> {
    let value = match identity {
        LaunchIdentity::Stable => 1,
        LaunchIdentity::NightlyV1 => 2,
    };
    IDENTITY
        .compare_exchange(0, value, std::sync::atomic::Ordering::AcqRel, std::sync::atomic::Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| "the window launch identity was already selected".into())
}

pub fn identity() -> LaunchIdentity {
    if IDENTITY.load(std::sync::atomic::Ordering::Acquire) == 2 { LaunchIdentity::NightlyV1 } else { LaunchIdentity::Stable }
}

pub const fn id_for(identity: LaunchIdentity) -> &'static str {
    match identity {
        LaunchIdentity::Stable => APP_ID,
        LaunchIdentity::NightlyV1 => "ai.storyteller.deckcraft.nightly-v1",
    }
}

pub fn app_id() -> &'static str {
    id_for(identity())
}

pub const fn name_for(identity: LaunchIdentity) -> &'static str {
    match identity {
        LaunchIdentity::Stable => "DeckCraft",
        LaunchIdentity::NightlyV1 => "DeckCraft Nightly",
    }
}

pub const fn stable_services_for(identity: LaunchIdentity) -> bool {
    matches!(identity, LaunchIdentity::Stable)
}

pub fn stable_services_allowed() -> bool {
    stable_services_for(identity())
}

/// A builder for an extra OS window, carrying the selected app id.
pub fn builder() -> egui::ViewportBuilder {
    egui::ViewportBuilder::default().with_app_id(app_id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_windows_carry_the_app_id() {
        assert_eq!(builder().app_id.as_deref(), Some(APP_ID));
    }

    #[test]
    fn nightly_identity_is_distinct_and_never_enables_stable_services() {
        assert_eq!(id_for(LaunchIdentity::Stable), APP_ID);
        assert_ne!(id_for(LaunchIdentity::NightlyV1), APP_ID);
        assert_eq!(id_for(LaunchIdentity::NightlyV1), "ai.storyteller.deckcraft.nightly-v1");
        assert_eq!(name_for(LaunchIdentity::NightlyV1), "DeckCraft Nightly");
        assert!(stable_services_for(LaunchIdentity::Stable));
        assert!(!stable_services_for(LaunchIdentity::NightlyV1));
    }
}
