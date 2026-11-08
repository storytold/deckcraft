//! macOS file dialogs as sheets. Shown in line, the panel's modal loop runs inside winit's event
//! handler; an event winit held back meanwhile (a resize of the panel ending) re-enters the handler
//! and winit's panic there, inside an AppKit callback, aborts the app. A sheet returns at once and
//! the window keeps running; a thread of its own waits for the answer.

use std::sync::{Arc, mpsc};

type Answer = std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send>>;

/// `start_pick` for [`deckcraft_ui_egui::Services`]: `(save, argument)` → the receiver of the answer.
pub fn start_pick(window: Arc<winit::window::Window>, ui: egui::Context) -> deckcraft_ui_egui::StartPickFn {
    Box::new(move |save, arg| {
        let (sheet_tx, sheet_rx) = mpsc::channel::<Answer>();
        let (tx, rx) = mpsc::channel();
        let ui = ui.clone();
        // The thread first, so the answer of a sheet once shown always has somewhere to go.
        std::thread::Builder::new()
            .name("file-dialog".into())
            .spawn(move || {
                if let Ok(answer) = sheet_rx.recv() {
                    // The app gone meanwhile has no use for the answer.
                    let _ = tx.send(pollster::block_on(answer));
                    ui.request_repaint();
                }
            })
            .ok()?;
        // rfd builds the panel and begins the sheet here, on the main thread (as it must).
        let dialog = dialog(arg, save).set_parent(&*window);
        let path = |f: rfd::FileHandle| f.path().to_string_lossy().to_string();
        let answer: Answer = if save {
            let picked = dialog.save_file();
            Box::pin(async move { picked.await.map(path) })
        } else {
            let picked = dialog.pick_file();
            Box::pin(async move { picked.await.map(path) })
        };
        let _ = sheet_tx.send(answer);
        Some(rx)
    })
}

/// The dialog for `arg`: a save dialog suggesting that file name, or an open dialog for that purpose.
fn dialog(arg: &str, save: bool) -> rfd::AsyncFileDialog {
    let d = rfd::AsyncFileDialog::new();
    if save {
        return d.set_file_name(arg);
    }
    crate::open_filters(arg).iter().fold(d, |d, (name, exts)| d.add_filter(*name, exts))
}
