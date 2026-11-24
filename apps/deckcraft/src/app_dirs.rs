//! Where the desktop app keeps its per-user data: `ui.json`, `prefs.json`, `logs/`, the crash
//! recovery folder and the user dictionary all live under the directory [`config_dir`] returns.
//!
//! In order:
//! 1. A selected Nightly profile root ([`crate::launch_profile`]): everything lives under it, and
//!    nothing is read from or written to a stable folder.
//! 2. The platform convention: macOS `~/Library/Application Support/DeckCraft`, Windows
//!    `%APPDATA%\DeckCraft`, Linux `$XDG_CONFIG_HOME/deckcraft` or `~/.config/deckcraft`.
//!
//! DeckCraft has no portable mode and no `DECKCRAFT_CONFIG_DIR` override, so the stable resolution
//! is exactly the one `main.rs` used before the Nightly channel; the fence module
//! ([`crate::profile_root`]) still refuses a `DeckCraftData` folder or a `portable.txt` beside a
//! DeckCraft executable, which a hand-made portable copy could carry.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The portable data folder name a hand-made portable copy could use. No DeckCraft package
/// writes it, but the fence refuses a profile inside one (see [`crate::profile_root`]).
pub const PORTABLE_DATA_DIR: &str = "DeckCraftData";

/// The marker file of a portable copy, beside its executable.
pub const PORTABLE_MARKERS: [&str; 1] = ["portable.txt"];

/// The portable marker in `dir`, when it is a regular file.
pub fn portable_marker(dir: &Path) -> Option<PathBuf> {
    PORTABLE_MARKERS.iter().map(|name| dir.join(name)).find(|path| path.is_file())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The profile root chosen by the Launcher (`--launch-profile nightly-v1`).
    Override,
    /// The platform convention (stable).
    Platform,
}

#[derive(Clone, Debug)]
pub struct DataDir {
    pub dir: Option<PathBuf>,
    pub mode: Mode,
}

/// The data directory for this process, resolved once.
pub fn current() -> &'static DataDir {
    static DIR: OnceLock<DataDir> = OnceLock::new();
    DIR.get_or_init(|| match crate::launch_profile::current().root() {
        Some(root) => DataDir { dir: Some(root.to_path_buf()), mode: Mode::Override },
        None => DataDir { dir: platform_dir(|key| std::env::var_os(key)), mode: Mode::Platform },
    })
}

/// The per-user settings directory (see the module docs).
pub fn config_dir() -> Option<PathBuf> {
    current().dir.clone()
}

/// The platform's settings directory. Read from the environment every call, so tests can vary it.
pub fn platform_dir(env: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        env("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/DeckCraft"))
    } else if cfg!(windows) {
        env("APPDATA").map(|a| PathBuf::from(a).join("DeckCraft"))
    } else {
        env("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".config"))).map(|c| c.join("deckcraft"))
    }
}

/// The crash-recovery folder: inside the profile for Nightly, the platform's own folder for stable
/// (`deckcraft_engine::recovery::default_dir`, which keeps autosaves out of the synced config).
fn recovery_dir_for(current: &DataDir) -> Option<PathBuf> {
    match current {
        DataDir { dir: Some(root), mode: Mode::Override } => Some(root.join("Recovery")),
        _ => deckcraft_engine::recovery::default_dir(),
    }
}

/// [`recovery_dir_for`] for this process.
pub fn recovery_dir() -> Option<PathBuf> {
    recovery_dir_for(current())
}

/// The user dictionary (`user.dic`, the words "Add to dictionary" keeps): inside the profile for
/// Nightly, the platform's dictionary folder for stable.
fn user_dictionary_for(current: &DataDir) -> Option<PathBuf> {
    match current {
        DataDir { dir: Some(root), mode: Mode::Override } => Some(root.join("Dictionaries").join("user.dic")),
        _ => None,
    }
}

/// [`user_dictionary_for`] for this process.
pub fn user_dictionary() -> Option<PathBuf> {
    user_dictionary_for(current())
}

/// The app's exclusive write probe: a new file created in `dir` and removed again. It never falls
/// back to another directory, so the Nightly validator can refuse a root it cannot write.
pub fn ensure_writable(dir: &Path) -> std::io::Result<()> {
    let probe = dir.join(".deckcraft-write-probe");
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(&probe)?;
    drop(file);
    match std::fs::remove_file(probe) {
        Ok(()) => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn env(pairs: &[(&str, &str)]) -> BTreeMap<String, OsString> {
        pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    #[test]
    fn platform_dirs_follow_the_platform_convention() {
        let home = if cfg!(windows) { r"C:\Users\me" } else { "/home/me" };
        let env = env(&[("HOME", home), ("APPDATA", r"C:\Users\me\AppData\Roaming"), ("XDG_CONFIG_HOME", "/xdg/config")]);
        let dir = platform_dir(|k| env.get(k).cloned()).expect("a platform directory");
        if cfg!(target_os = "macos") {
            assert_eq!(dir, PathBuf::from(home).join("Library/Application Support/DeckCraft"));
        } else if cfg!(windows) {
            assert_eq!(dir, PathBuf::from(r"C:\Users\me\AppData\Roaming").join("DeckCraft"));
        } else {
            assert_eq!(dir, PathBuf::from("/xdg/config").join("deckcraft"));
        }
    }

    /// Recovery and the dictionary follow the profile; stable keeps the engine's own folders and
    /// writes no per-profile dictionary.
    #[test]
    fn the_nightly_profile_owns_recovery_and_the_dictionary() {
        let root = PathBuf::from("/nightly-data/profiles/deckcraft");
        let profile = DataDir { dir: Some(root.clone()), mode: Mode::Override };
        assert_eq!(recovery_dir_for(&profile), Some(root.join("Recovery")));
        assert_eq!(user_dictionary_for(&profile), Some(root.join("Dictionaries").join("user.dic")));
        let stable = DataDir { dir: None, mode: Mode::Platform };
        assert_eq!(user_dictionary_for(&stable), None);
        assert_eq!(recovery_dir_for(&stable), deckcraft_engine::recovery::default_dir());
    }

    /// The process resolves its data directory once; every caller must get that same decision.
    #[test]
    fn the_data_directory_is_resolved_once() {
        let resolved = current();
        assert!(std::ptr::eq(resolved, current()));
        assert_eq!(resolved.dir, config_dir());
    }

    #[test]
    fn permanent_probe_does_not_replace_an_existing_entry() {
        let dir = std::env::temp_dir().join(format!("deckcraft-writable-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".deckcraft-write-probe"), b"someone else's file").unwrap();
        assert!(ensure_writable(&dir).is_err(), "create_new must not truncate another file");
        assert_eq!(std::fs::read(dir.join(".deckcraft-write-probe")).unwrap(), b"someone else's file");
        std::fs::remove_file(dir.join(".deckcraft-write-probe")).unwrap();
        ensure_writable(&dir).unwrap();
        assert!(!dir.join(".deckcraft-write-probe").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
