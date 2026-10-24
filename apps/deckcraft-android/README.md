# deckcraft-android

DeckCraft on Android: a small `apps/` crate that hosts the existing `deckcraft-ui-egui` interface in
an `android-activity` (GameActivity) window, on top of `deckcraft-engine`. The desktop entry crate is
not compiled, so `rfd`, `arboard` and the macOS/Windows/Linux platform code never enter the
dependency graph.

## What the shell does

- **Entry point** — builds as `libmain.so` (`[lib] name = "main"`, `crate-type = ["cdylib"]`), loaded
  with `System.loadLibrary`; `android_main` is the usual `android-activity` entry.
- **Storage** — `AndroidApp::internal_data_path()` is the data directory. `ui.json` and `prefs.json` are
  loaded before the first frame and written back in `on_exit`, the same pair `apps/deckcraft` keeps.
- **File pickers** — DeckCraft's `Services` hooks are filled from the Storage Access Framework.
  `pick_open` starts `ACTION_OPEN_DOCUMENT` through the Activity (six JNI calls in `src/saf.rs`); the
  picked file is copied into `<filesDir>/import/` and its real path is sent back over the app's own
  control channel, so the *desktop* `app.open` command does the opening. `pick_save` answers with a
  scratch path in `<filesDir>/export/`, then a worker thread shows `ACTION_CREATE_DOCUMENT` and copies
  the finished file out once its size has been stable for 1.5 s.
- **Reads and writes** — `Services::read`/`write` are plain `std::fs`, because the SAF layer has
  already turned the user's choice into a path inside the app's storage.
- **Fonts** — Android does not expose the Unix font directories, so the CJK face from `/system/fonts`
  is appended to every family *after* DeckCraft installs its theme (earlier, and the theme's named
  families would be dropped).
- **Touch** — hit targets are raised to 40 pt, sliders widened, and tooltips limited to a still
  finger, applied after the theme's style exists. The layout is not zoomed: it is designed for
  ≥900 pt wide and a phone gives ~411 pt, so the next step is a touch rework, not a scale factor.

## Known gaps

- The system clipboard is not wired (`arboard` has no Android backend); DeckCraft's own clipboard is
  still used.
- No print backend: the Print dialog is unreachable rather than broken.
- The pickers take real paths out of the SAF copy, so a document is opened from the app's own
  directory; "save back to the original document" is not wired.
- The launcher icons are the Android Studio template's placeholder art.

## Build

```bash
cargo ndk -t arm64-v8a -o apps/deckcraft-android/jniLibs build --release -p deckcraft-android
cd apps/deckcraft-android/android && ./gradlew assembleRelease
```

`jniLibs/` and the Gradle output are gitignored; the APK is not part of this change.
