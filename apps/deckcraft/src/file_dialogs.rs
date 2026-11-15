//! Open dialogs that don't freeze the window (#2040).
//!
//! The blocking `rfd::FileDialog` ran on the UI thread from inside the frame: on Linux the
//! xdg-desktop-portal call held the event loop until the dialog closed, so GNOME marked the window
//! as not responding, and with no parent window the dialog could open behind DeckCraft. Now the
//! app's request ([`Services::open_dialog`]) is queued; the frame, which has the window, builds an
//! `rfd::AsyncFileDialog` parented to it, and a background thread waits for the answer, hands it
//! to the app and wakes the UI. Same approach as WordCraft (storytold/wordcraft#94).
//!
//! [`Services::open_dialog`]: deckcraft_ui_egui::Services::open_dialog

use std::pin::Pin;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use deckcraft_ui_egui::OpenDialogFn;

/// The purpose the app asked for, with the channel its answer goes back through.
type Request = (String, Sender<Option<String>>);

/// A picker's answer, waited for off the UI thread.
type Picking = Pin<Box<dyn Future<Output = Option<rfd::FileHandle>> + Send>>;

/// Shows the dialogs the app queued; lives in the eframe app, which has the window.
pub struct Launcher {
    queue: Receiver<Request>,
    ctx: egui::Context,
}

/// The [`Services::open_dialog`](deckcraft_ui_egui::Services::open_dialog) hook and the launcher
/// that shows what it queues.
pub fn hook(ctx: &egui::Context) -> (OpenDialogFn, Launcher) {
    let (queue_tx, queue) = channel::<Request>();
    let hook: OpenDialogFn = Box::new(move |purpose: &str| {
        let (answer, rx) = channel();
        // If the launcher is gone, the dropped answer reads as a cancel in the app.
        let _ = queue_tx.send((purpose.to_string(), answer));
        rx
    });
    (hook, Launcher { queue, ctx: ctx.clone() })
}

impl Launcher {
    /// Show the queued dialogs with the window as their parent, so the portal (or the system)
    /// keeps them on top of it. Call after every pass that may ask for one.
    pub fn show(&self, parent: &eframe::Frame) {
        while let Ok((purpose, answer)) = self.queue.try_recv() {
            // Built here, on the UI thread: macOS shows the panel as a sheet on the window now.
            let picking: Picking = Box::pin(dialog(&purpose).set_parent(parent).pick_file());
            let ctx = self.ctx.clone();
            let waiting = std::thread::Builder::new().name("file dialog".into()).spawn(move || {
                let started = Instant::now();
                let mut picked = pollster::block_on(picking);
                // Some Linux portals can't parent the dialog (xdg-foreign on Wayland) and rfd
                // reports that as a cancel: ask once more without a parent, as the old dialog did.
                // Only on Linux and the BSDs, and only quicker than anyone could dismiss a dialog.
                if picked.is_none() && failed_at_once(started.elapsed()) {
                    log::info!("the parented file dialog closed at once; showing it without a parent");
                    picked = pollster::block_on(dialog(&purpose).pick_file());
                }
                let _ = answer.send(picked.map(|f| f.path().to_string_lossy().to_string()));
                ctx.request_repaint();
            });
            // The answer's sender went with the closure, so the app sees the dialog as cancelled.
            if let Err(e) = waiting {
                log::error!("couldn't wait for the file dialog: {e}");
            }
        }
    }
}

/// Quicker than anyone can see a dialog and dismiss it (WordCraft uses the same bound): a cancel
/// this soon is the portal failing, not the user. A timing heuristic, as rfd folds the portal's
/// failure into "cancelled"; a portal failing slower than this still reads as a cancel.
const AT_ONCE: Duration = Duration::from_millis(300);

/// The parented dialog failed rather than being cancelled. Only the xdg-desktop-portal (Linux and
/// the BSDs) folds a failure into a cancel; Windows and macOS always show their dialog, so a quick
/// cancel there is the user's and is answered as one.
fn failed_at_once(elapsed: Duration) -> bool {
    cfg!(all(unix, not(target_os = "macos"))) && elapsed < AT_ONCE
}

/// An open dialog for `purpose`, offering its file types.
fn dialog(purpose: &str) -> rfd::AsyncFileDialog {
    open_filters(purpose).iter().fold(rfd::AsyncFileDialog::new(), |d, (name, ext)| d.add_filter(*name, ext))
}

/// The file types an open dialog offers for `purpose`, first filter selected.
pub fn open_filters(purpose: &str) -> &'static [(&'static str, &'static [&'static str])] {
    match purpose {
        "picture" => &[("Pictures", &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"])],
        "audio" => {
            &[("Audio", &["wav", "mp3", "m4a", "m4b", "aac", "flac", "ogg", "oga", "opus", "aif", "aiff", "aifc", "caf", "wma", "weba", "mka"])]
        }
        "video" => &[("Video", &["mp4", "m4v", "mov", "webm", "mkv", "wmv"])],
        "theme" => &[("Themes and presentations", &["thmx", "pptx", "potx", "ppsx"]), ("Office Theme", &["thmx"])],
        _ => &[
            ("Presentations", &["deckcraft", "pptx", "potx", "ppsx"]),
            ("DeckCraft", &["deckcraft"]),
            ("PowerPoint", &["pptx", "potx", "ppsx"]),
            ("Outline", &["txt", "md"]),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_immediate_portal_cancel_retries_without_a_parent() {
        let portal = cfg!(all(unix, not(target_os = "macos")));
        assert_eq!(failed_at_once(Duration::from_millis(20)), portal, "a portal that failed at once is asked again");
        assert!(!failed_at_once(Duration::from_millis(400)), "an Escape a moment after the dialog showed is a cancel");
        assert!(!failed_at_once(Duration::from_secs(3)), "a user who cancels is not asked again");
    }
}
