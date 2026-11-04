//! Copy and paste between DeckCraft windows. Each window is its own process, so what a window
//! copies (shapes, slides, with their pictures) is written to a file the windows share, and the
//! others take it from there; see `deckcraft_engine::clipboard_share` for the packet. Pasting then
//! behaves as it does inside one window. Only where the host gives a folder
//! ([`crate::Services::shared_clipboard_dir`]).
//!
//! A copy made before this window started is ignored, so a stale copy from an earlier session never
//! turns up in a paste.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::SlideApp;

/// The file the windows share, in the host's folder.
const FILE: &str = "clipboard.json";
/// How often a window looks for a copy from another (seconds).
const POLL_SECS: f64 = 0.25;

/// What a window remembers about the shared clipboard.
pub struct State {
    /// The session's clipboard serial when it was last written to or read from the file.
    serial: u64,
    /// The copy number last written or read.
    seq: u64,
    /// The file's modification time when it was last looked at.
    mtime: Option<SystemTime>,
    /// This window's start: older copies are not taken.
    started: SystemTime,
    next_check: f64,
}

impl Default for State {
    fn default() -> Self {
        State { serial: 0, seq: 0, mtime: None, started: SystemTime::now(), next_check: 0.0 }
    }
}

fn file_in(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Write `bytes` to `file` all at once: another window never reads half a copy.
fn write_atomically(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = file.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, file).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

impl SlideApp {
    /// Share this window's latest copy, and take a newer one made in another window. Called every
    /// frame; the file is only looked at a few times a second.
    pub(crate) fn sync_shared_clipboard(&mut self, now_ms: f64) {
        let Some(dir) = self.services.shared_clipboard_dir.clone() else { return };
        let file = file_in(&dir);
        let me = deckcraft_engine::recovery::instance_id();

        // This window copied or cut: write it.
        let serial = self.session.clipboard_serial();
        if serial != self.shared_clip.serial {
            self.shared_clip.serial = serial;
            // Unique and increasing across windows: nanoseconds since the epoch.
            let seq = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
            if let Some(bytes) = self.session.clipboard_packet(me, seq) {
                match write_atomically(&file, &bytes) {
                    Ok(()) => {
                        self.shared_clip.seq = seq;
                        self.shared_clip.mtime = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
                    }
                    Err(e) => log::warn!("shared clipboard: {e}"),
                }
            }
        }

        // Another window copied or cut: take it.
        if now_ms < self.shared_clip.next_check {
            return;
        }
        self.shared_clip.next_check = now_ms + POLL_SECS * 1000.0;
        let Ok(mtime) = std::fs::metadata(&file).and_then(|m| m.modified()) else { return };
        if self.shared_clip.mtime == Some(mtime) {
            return;
        }
        self.shared_clip.mtime = Some(mtime);
        if mtime < self.shared_clip.started {
            return;
        }
        let Ok(bytes) = std::fs::read(&file) else { return };
        let Some((owner, seq)) = deckcraft_engine::clipboard_share::packet_origin(&bytes) else { return };
        if owner == me || seq == self.shared_clip.seq {
            return;
        }
        if self.session.load_clipboard_packet(&bytes).is_some() {
            self.shared_clip.seq = seq;
            // Taking it is not a copy of this window's: don't write it back.
            self.shared_clip.serial = self.session.clipboard_serial();
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Services, SlideApp};
    use deckcraft_engine::Session;
    use serde_json::json;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("deckcraft-clip-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn window(dir: &std::path::Path) -> SlideApp {
        SlideApp::new(Session::with_new(), Services { shared_clipboard_dir: Some(dir.to_path_buf()), ..Default::default() })
    }

    /// Two windows in one process stand for two processes: they share only the folder. The second
    /// has another instance id in real life; here the owner check is what is under test, so the
    /// copy is written as a stranger's.
    fn copy_a_text_box(app: &mut SlideApp, text: &str) {
        app.session.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": text})).unwrap();
        app.session.execute("text.exit", &json!({})).unwrap();
        app.session.execute("edit.copy", &json!({})).unwrap();
    }

    #[test]
    fn without_a_folder_nothing_is_written_or_read() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        copy_a_text_box(&mut app, "x");
        app.sync_shared_clipboard(1000.0);
    }

    #[test]
    fn a_copy_is_written_for_the_other_windows() {
        let dir = temp_dir("write");
        let mut a = window(&dir);
        copy_a_text_box(&mut a, "from A");
        a.sync_shared_clipboard(1000.0);
        let bytes = std::fs::read(dir.join(super::FILE)).expect("the copy was shared");
        let (owner, _) = deckcraft_engine::clipboard_share::packet_origin(&bytes).expect("a packet");
        assert_eq!(owner, deckcraft_engine::recovery::instance_id());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn another_window_takes_a_newer_copy_and_a_window_ignores_its_own() {
        let dir = temp_dir("take");
        // The writer: a packet from another instance, written after this window started.
        let mut writer = Session::with_new();
        writer.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "from B"})).unwrap();
        writer.execute("text.exit", &json!({})).unwrap();
        writer.execute("edit.copy", &json!({})).unwrap();
        let mut b = window(&dir);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let packet = writer.clipboard_packet(deckcraft_engine::recovery::instance_id().wrapping_add(1), 5).unwrap();
        super::write_atomically(&dir.join(super::FILE), &packet).unwrap();

        assert!(b.session.clipboard.shapes.is_empty());
        b.sync_shared_clipboard(1000.0);
        assert_eq!(b.session.clipboard.plain, "from B", "the other window's copy is here now");
        let serial = b.session.clipboard_serial();
        b.sync_shared_clipboard(5000.0);
        assert_eq!(b.session.clipboard_serial(), serial, "it is not written back or taken twice");

        let before = b.session.active().unwrap().current_slide().unwrap().shapes.len();
        b.session.execute("edit.paste", &json!({})).unwrap();
        assert_eq!(b.session.active().unwrap().current_slide().unwrap().shapes.len(), before + 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_copy_from_before_this_window_started_is_ignored() {
        let dir = temp_dir("stale");
        let mut writer = Session::with_new();
        writer.execute("insert.textBox", &json!({"rect": [40, 40, 400, 80], "text": "old"})).unwrap();
        writer.execute("text.exit", &json!({})).unwrap();
        writer.execute("edit.copy", &json!({})).unwrap();
        let packet = writer.clipboard_packet(deckcraft_engine::recovery::instance_id().wrapping_add(1), 5).unwrap();
        super::write_atomically(&dir.join(super::FILE), &packet).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let mut b = window(&dir);
        b.sync_shared_clipboard(1000.0);
        assert!(b.session.clipboard.shapes.is_empty(), "an earlier session's copy does not turn up");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_damaged_file_is_ignored() {
        let dir = temp_dir("bad");
        let mut b = window(&dir);
        std::thread::sleep(std::time::Duration::from_millis(20));
        super::write_atomically(&dir.join(super::FILE), b"{ not a packet").unwrap();
        b.sync_shared_clipboard(1000.0);
        assert!(b.session.clipboard.shapes.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
