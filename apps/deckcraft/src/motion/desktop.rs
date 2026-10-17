//! The desktop's animations setting on Linux and the BSDs.
//!
//! The XDG Desktop Portal's Settings interface is asked first (a sandboxed Flatpak can read it
//! too). Desktops keep the setting in different places, and any one of them asking for no
//! animations is enough:
//! - `org.freedesktop.appearance` `reduced-motion`: the portal's own key, on newer portals;
//! - `org.gnome.desktop.interface` `enable-animations`: GNOME, and the desktops built on GTK
//!   that share its settings;
//! - `org.kde.kdeglobals.KDE` `AnimationDurationFactor`: KDE Plasma's animation speed, where 0
//!   is "instant".
//!
//! Where no portal answers (a bare window manager, many BSD set-ups) the files those settings
//! are saved in are read instead: KDE's `kdeglobals` and GTK's `settings.ini`
//! (`gtk-enable-animations`).
//!
//! D-Bus calls block, so all of it runs on a thread of its own, which asks again every few
//! seconds: a setting changed while DeckCraft runs is followed within that time.

/// One answer from the desktop's settings, as plain data.
#[derive(Clone, Debug, PartialEq)]
enum Answer {
    Flag(bool),
    Number(f64),
    Text(String),
}

/// What an [`Answer`] says about animations: on, off, or nothing.
type Meaning = fn(&Answer) -> Option<bool>;

/// The portal's `reduced-motion`: 0 is no preference, 1 is reduced motion.
fn reduced_motion(answer: &Answer) -> Option<bool> {
    match answer {
        Answer::Number(level) if *level == 0.0 => Some(true),
        Answer::Number(level) if *level == 1.0 => Some(false),
        _ => None,
    }
}

/// GNOME's `enable-animations` and GTK's `gtk-enable-animations` (`1`/`0` or `true`/`false` in
/// `settings.ini`).
fn enable_animations(answer: &Answer) -> Option<bool> {
    match answer {
        Answer::Flag(on) => Some(*on),
        Answer::Number(n) if *n == 0.0 || *n == 1.0 => Some(*n == 1.0),
        Answer::Text(text) => match text.trim().to_ascii_lowercase().as_str() {
            "1" | "true" => Some(true),
            "0" | "false" => Some(false),
            _ => None,
        },
        Answer::Number(_) => None,
    }
}

/// KDE's `AnimationDurationFactor`: how long animations take against their designed length. 0
/// is "instant", which is how Plasma turns them off.
fn duration_factor(answer: &Answer) -> Option<bool> {
    let factor = match answer {
        Answer::Number(factor) => *factor,
        Answer::Text(text) => text.trim().parse::<f64>().ok()?,
        Answer::Flag(_) => return None,
    };
    (factor.is_finite() && factor >= 0.0).then_some(factor > 0.0)
}

/// Several settings put together: off when any one says off, on when at least one says on,
/// nothing when none says anything.
fn combine(answers: impl IntoIterator<Item = Option<bool>>) -> Option<bool> {
    let mut any = None;
    for on in answers.into_iter().flatten() {
        if !on {
            return Some(false);
        }
        any = Some(true);
    }
    any
}

/// The value of `key` in `[section]` of an INI-style settings file.
fn ini_value<'a>(text: &'a str, section: &str, key: &str) -> Option<&'a str> {
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            inside = name == section;
        } else if inside
            && let Some((name, value)) = line.split_once('=')
            && name.trim() == key
        {
            return Some(value.trim());
        }
    }
    None
}

/// The files the setting is saved in, under the user's configuration folder: (file, section,
/// key, what its value means).
const FILES: [(&str, &str, &str, Meaning); 3] = [
    ("kdeglobals", "KDE", "AnimationDurationFactor", duration_factor),
    ("gtk-4.0/settings.ini", "Settings", "gtk-enable-animations", enable_animations),
    ("gtk-3.0/settings.ini", "Settings", "gtk-enable-animations", enable_animations),
];

/// What the saved settings say; `read` returns a file's text by its path under the
/// configuration folder.
fn from_files(read: impl Fn(&str) -> Option<String>) -> Option<bool> {
    combine(FILES.iter().map(|(file, section, key, meaning)| {
        let text = read(file)?;
        meaning(&Answer::Text(ini_value(&text, section, key)?.to_string()))
    }))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub use watcher::watch;

#[cfg(all(unix, not(target_os = "macos")))]
mod watcher {
    use std::io::Read;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, PoisonError};
    use std::time::Duration;

    use zbus::blocking::Connection;
    use zbus::zvariant::{OwnedValue, Value};

    use super::{Answer, Meaning, combine, duration_factor, enable_animations, from_files, reduced_motion};

    /// How often the desktop is asked again. The portal also publishes a change signal, but one
    /// round trip every few seconds is cheap and follows a changed setting soon enough.
    const POLL: Duration = Duration::from_secs(4);

    /// Settings files are a few hundred bytes; nothing longer is read.
    const MAX_FILE: u64 = 256 * 1024;

    /// The portal settings asked: (namespace, key, what its answer means).
    const PORTAL_KEYS: [(&str, &str, Meaning); 3] = [
        ("org.freedesktop.appearance", "reduced-motion", reduced_motion),
        ("org.gnome.desktop.interface", "enable-animations", enable_animations),
        ("org.kde.kdeglobals.KDE", "AnimationDurationFactor", duration_factor),
    ];

    /// Starts watching the desktop's setting and returns the UI's way to read the last answer:
    /// `None` until the desktop has answered, and where it says nothing. `ctx` repaints when the
    /// answer changes, so a window at rest follows it too.
    pub fn watch(ctx: egui::Context) -> impl Fn() -> Option<bool> {
        let answer = Arc::new(Mutex::new(None));
        let shared = answer.clone();
        let started = std::thread::Builder::new().name("deckcraft-motion".into()).spawn(move || {
            let bus = Connection::session().map_err(|e| log::debug!("the desktop's animations setting: no session bus ({e})")).ok();
            let mut last = None;
            // Until the UI has dropped its side.
            while Arc::strong_count(&shared) > 1 {
                let now = bus.as_ref().and_then(from_portal).or_else(|| from_files(read_config));
                if last != Some(now) {
                    last = Some(now);
                    crate::motion::log_answer(now);
                    *shared.lock().unwrap_or_else(PoisonError::into_inner) = now;
                    ctx.request_repaint();
                }
                std::thread::sleep(POLL);
            }
        });
        if let Err(e) = started {
            log::warn!("the desktop's animations setting isn't watched: {e}");
        }
        move || *answer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// What the settings portal says.
    fn from_portal(bus: &Connection) -> Option<bool> {
        combine(PORTAL_KEYS.iter().map(|(namespace, key, meaning)| meaning(&ask(bus, namespace, key)?)))
    }

    /// One portal setting. `ReadOne` is the current call; portals from before it only have
    /// `Read`, which wraps its answer in one more variant.
    fn ask(bus: &Connection, namespace: &str, key: &str) -> Option<Answer> {
        ["ReadOne", "Read"].iter().find_map(|method| {
            let reply = bus
                .call_method(
                    Some("org.freedesktop.portal.Desktop"),
                    "/org/freedesktop/portal/desktop",
                    Some("org.freedesktop.portal.Settings"),
                    *method,
                    &(namespace, key),
                )
                .ok()?;
            let (value,) = reply.body().deserialize::<(OwnedValue,)>().ok()?;
            plain(&value)
        })
    }

    /// The value inside the variants the portal wraps it in.
    fn plain(value: &Value<'_>) -> Option<Answer> {
        let mut value = value;
        while let Value::Value(inner) = value {
            value = inner.as_ref();
        }
        match value {
            Value::Bool(flag) => Some(Answer::Flag(*flag)),
            Value::U32(n) => Some(Answer::Number(f64::from(*n))),
            Value::I32(n) => Some(Answer::Number(f64::from(*n))),
            Value::F64(n) => Some(Answer::Number(*n)),
            Value::Str(text) => Some(Answer::Text(text.to_string())),
            _ => None,
        }
    }

    /// A settings file's text, by its path under `$XDG_CONFIG_HOME` (`~/.config`).
    fn read_config(relative: &str) -> Option<String> {
        let dir = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        let mut text = String::new();
        std::fs::File::open(dir.join(relative)).ok()?.take(MAX_FILE).read_to_string(&mut text).ok()?;
        Some(text)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// The portal wraps a value in a variant (`ReadOne`) or two (`Read`).
        #[test]
        fn portal_values_are_read_through_their_variants() {
            let wrapped = |value: Value<'static>| Value::Value(Box::new(Value::Value(Box::new(value))));
            assert_eq!(plain(&Value::Bool(false)), Some(Answer::Flag(false)));
            assert_eq!(plain(&wrapped(Value::U32(1))), Some(Answer::Number(1.0)));
            assert_eq!(plain(&wrapped(Value::F64(0.5))), Some(Answer::Number(0.5)));
            assert_eq!(plain(&wrapped(Value::Str("0".into()))), Some(Answer::Text("0".into())));
            assert_eq!(plain(&Value::U8(1)), None);
        }

        /// The UI's side never waits for the desktop, with or without a session bus or portal.
        #[test]
        fn the_watcher_answers_from_memory_at_once() {
            let read = watch(egui::Context::default());
            let asked = std::time::Instant::now();
            for _ in 0..1000 {
                let _ = read();
            }
            assert!(asked.elapsed() < Duration::from_millis(250), "{:?}", asked.elapsed());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_setting_reads_as_on_off_or_nothing() {
        let (flag, number, text) = (Answer::Flag, Answer::Number, |t: &str| Answer::Text(t.to_string()));
        // The portal's reduced-motion level.
        assert_eq!(reduced_motion(&number(0.0)), Some(true));
        assert_eq!(reduced_motion(&number(1.0)), Some(false));
        assert_eq!(reduced_motion(&number(2.0)), None);
        assert_eq!(reduced_motion(&flag(true)), None);
        // GNOME's and GTK's switch.
        assert_eq!(enable_animations(&flag(true)), Some(true));
        assert_eq!(enable_animations(&flag(false)), Some(false));
        assert_eq!(enable_animations(&text(" 0 ")), Some(false));
        assert_eq!(enable_animations(&text("True")), Some(true));
        assert_eq!(enable_animations(&number(1.0)), Some(true));
        assert_eq!(enable_animations(&number(0.5)), None);
        assert_eq!(enable_animations(&text("")), None);
        // KDE's animation speed: "instant" is 0.
        assert_eq!(duration_factor(&text("0")), Some(false));
        assert_eq!(duration_factor(&text("0.5")), Some(true));
        assert_eq!(duration_factor(&number(1.0)), Some(true));
        assert_eq!(duration_factor(&number(0.0)), Some(false));
        for hostile in ["", "fast", "NaN", "inf", "-1", "1e999"] {
            assert_eq!(duration_factor(&text(hostile)), None, "{hostile:?}");
        }
        assert_eq!(duration_factor(&number(f64::NAN)), None);
        assert_eq!(duration_factor(&flag(true)), None);
    }

    #[test]
    fn any_setting_saying_off_is_enough() {
        assert_eq!(combine([]), None);
        assert_eq!(combine([None, None]), None);
        assert_eq!(combine([None, Some(true)]), Some(true));
        assert_eq!(combine([Some(true), None, Some(false)]), Some(false));
        assert_eq!(combine([Some(false), Some(true)]), Some(false));
    }

    #[test]
    fn settings_files_are_read_by_section_and_key() {
        let kde = "[General]\nAnimationDurationFactor=3\n\n[KDE]\nLookAndFeelPackage=org.kde.breeze\nAnimationDurationFactor = 0\n";
        assert_eq!(ini_value(kde, "KDE", "AnimationDurationFactor"), Some("0"));
        assert_eq!(ini_value(kde, "General", "AnimationDurationFactor"), Some("3"));
        assert_eq!(ini_value(kde, "KDE", "Missing"), None);
        assert_eq!(ini_value(kde, "Missing", "AnimationDurationFactor"), None);
        assert_eq!(ini_value("AnimationDurationFactor=0\n", "KDE", "AnimationDurationFactor"), None, "outside any section");
        assert_eq!(ini_value("[KDE]\r\nAnimationDurationFactor=0\r\n", "KDE", "AnimationDurationFactor"), Some("0"));
        assert_eq!(ini_value("", "KDE", "AnimationDurationFactor"), None);
    }

    #[test]
    fn saved_settings_answer_where_no_portal_does() {
        let files = |kde: Option<&'static str>, gtk4: Option<&'static str>, gtk3: Option<&'static str>| {
            from_files(|file| {
                match file {
                    "kdeglobals" => kde,
                    "gtk-4.0/settings.ini" => gtk4,
                    "gtk-3.0/settings.ini" => gtk3,
                    _ => None,
                }
                .map(str::to_string)
            })
        };
        assert_eq!(files(None, None, None), None);
        assert_eq!(files(Some("[KDE]\nAnimationDurationFactor=0\n"), None, None), Some(false));
        assert_eq!(files(Some("[KDE]\nAnimationDurationFactor=1\n"), None, None), Some(true));
        assert_eq!(files(None, None, Some("[Settings]\ngtk-enable-animations=0\n")), Some(false));
        assert_eq!(files(None, Some("[Settings]\ngtk-enable-animations=true\n"), None), Some(true));
        // A file without the key, or with nonsense in it, says nothing.
        assert_eq!(files(Some("[KDE]\nSingleClick=false\n"), Some("[Settings]\ngtk-enable-animations=maybe\n"), None), None);
        // Any one saying off is enough.
        assert_eq!(files(Some("[KDE]\nAnimationDurationFactor=1\n"), None, Some("[Settings]\ngtk-enable-animations=0\n")), Some(false));
    }
}
