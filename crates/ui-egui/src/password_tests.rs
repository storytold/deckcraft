//! Opening a password-protected deck (#435): the prompt, a rejected password that stays under the
//! field, the deck opening with its password and the save caveat, and a legacy `.ppt` that must not
//! be mistaken for an encrypted package.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use deckcraft_engine::Session;
use egui_kittest::Harness;
use serde_json::{Value, json};

use crate::{Services, SlideApp};

/// An encrypted package from `crates/pptx/tests/fixtures` (see its `make.py`).
const ENCRYPTED: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../pptx/tests/fixtures/standard-aes128-sha1.pptx");
/// The password that fixture was encrypted with.
const PASSWORD: &str = "DeckCraft-Test-Passw0rd!";

fn harness() -> Harness<'static, SlideApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, app: &mut SlideApp| {
            app.logic(ui.ctx());
            app.ui(ui);
        },
        SlideApp::new(Session::new(), Services::default()),
    );
    h.run_steps(2);
    h
}

/// A locked deck cannot open unprompted, so the prompt is the message — not a status-bar error, and
/// not a document.
#[test]
fn opening_a_locked_deck_asks_for_the_password() {
    let mut h = harness();
    assert!(h.state_mut().open_path(ENCRYPTED).is_err(), "an encrypted deck cannot open unprompted");
    h.run_steps(2);

    let d = h.state().dialog.as_ref().expect("the password dialog is open");
    assert_eq!(d.id, "password");
    assert_eq!(d.params.get("path").and_then(Value::as_str), Some(ENCRYPTED));
    assert_eq!(d.params.get("name").and_then(Value::as_str), Some(ENCRYPTED));
    assert!(h.state().session.documents().is_empty(), "nothing is opened yet");
    assert!(h.state().status.is_none(), "asking for a password is not an error to report");
}

/// A rejected password keeps the dialog and explains itself under the field.
#[test]
fn the_wrong_password_is_explained_in_the_dialog() {
    let mut h = harness();
    assert!(h.state_mut().open_path(ENCRYPTED).is_err());
    h.run_steps(2);

    let err = h.state_mut().run("file.open", json!({"path": ENCRYPTED, "password": "nope"})).unwrap_err();
    h.run_steps(2);
    let d = h.state().dialog.as_ref().expect("the dialog stays open after a bad password");
    assert_eq!(d.id, "password");
    assert_eq!(d.fields.get("error").map(String::as_str), Some("Incorrect password. Please try again."), "{err}");
    assert!(h.state().status.is_none(), "the message belongs under the field, not in the status bar");
    assert!(h.state().session.documents().is_empty(), "a wrong password opens nothing");
}

/// The right password opens the deck with no file to save over, and warns that saving needs a new
/// name.
#[test]
fn the_right_password_opens_the_deck_and_warns_about_saving() {
    let mut h = harness();
    assert!(h.state_mut().open_path(ENCRYPTED).is_err());
    h.run_steps(2);

    h.state_mut().run("file.open", json!({"path": ENCRYPTED, "password": PASSWORD})).unwrap();
    assert!(h.state().dialog.is_none(), "the dialog closes once the deck opens");
    // DeckCraft cannot write an encrypted package back, so the user is told before they save.
    // (Read before the next frames: an installed-font notice may take the toast slot itself.)
    let toast = h.state().toast.as_ref().expect("the save caveat is shown").text.clone();
    assert!(toast.contains("can't encrypt on save"), "{toast}");
    h.run_steps(2);
    let doc = h.state().session.active().expect("the deck opened");
    assert_eq!(doc.doc.slides.len(), 3);
    assert_eq!(doc.doc.slides[0].title(), "Encrypted fixture");
    // No path: Save must ask for a name rather than write an unencrypted package over the
    // protected original.
    assert_eq!(doc.path, None, "an encrypted source is not a save target");
    assert_eq!(doc.title, "standard-aes128-sha1");
    assert_eq!(h.state_mut().run("file.save", json!({})).unwrap(), json!({"needsPath": true}));
}

/// A legacy binary `.ppt` is an OLE compound file too, but carries no encryption streams: it reports
/// its own reason instead of asking for a password.
#[test]
fn a_legacy_ppt_is_not_mistaken_for_an_encrypted_deck() {
    let mut h = harness();
    let mut bytes = vec![0xD0u8, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    bytes.resize(4096, 0);
    let data = deckcraft_engine::cmd::base64_encode(&bytes);
    h.state_mut().run("file.openBytes", json!({"name": "lecture.ppt", "data": data})).expect_err("a legacy .ppt is still unsupported");
    h.run_steps(2);
    assert!(h.state().dialog.is_none(), "a legacy .ppt must not prompt for a password");
    let (msg, _) = h.state().status.clone().expect("the failure is reported");
    assert!(msg.contains("legacy binary PowerPoint"), "{msg}");
}

/// The dialog itself starts the read in the background: clicking Open closes the prompt and leaves a
/// worker thread behind, and a rejection brings the prompt back with the text the user typed.
#[test]
fn the_dialog_opens_in_the_background_and_restores_the_typed_password() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    let mut h = harness();
    assert!(h.state_mut().open_path(ENCRYPTED).is_err());
    h.run_steps(2);
    // A masked field is a `PasswordInput` to accessibility, not a `TextInput`.
    let field = h.get_by_role(Role::PasswordInput);
    field.focus();
    h.run_steps(1);
    h.get_by_role(Role::PasswordInput).type_text("nope");
    h.run_steps(2);
    // What the click hands to the read is the dialog's own copy of the field.
    let typed = h.state().dialog.as_ref().and_then(|d| d.fields.get("password").cloned());
    assert_eq!(typed.as_deref(), Some("nope"), "the dialog's field took the typed password");
    // The start screen has an "Open" too: click the dialog's, the button beside its Cancel.
    let cancel = h.get_by_label("Cancel").rect();
    let open = h.get_all_by_label("Open").find(|n| (n.rect().center().y - cancel.center().y).abs() < 8.0).expect("the dialog's Open button");
    open.click();
    h.run_steps(1);
    // The click handed the read to a thread and closed the prompt: the frame was not blocked by a
    // 100,000-round key derivation.
    assert!(h.state().dialog.is_none(), "the prompt closes while the read runs");
    assert!(h.state().opening.is_some(), "the read runs on a worker thread");

    for _ in 0..500 {
        h.run_steps(1);
        if h.state().opening.is_none() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.state().opening.is_none(), "the read finished");
    let d = h.state().dialog.as_ref().expect("a rejected password brings the prompt back");
    assert_eq!(d.id, "password");
    assert_eq!(d.fields.get("password").map(String::as_str), Some("nope"), "the typed text is still there");
    assert_eq!(d.fields.get("error").map(String::as_str), Some("Incorrect password. Please try again."));
    assert!(h.state().session.documents().is_empty(), "a wrong password opens nothing");
}

/// A locked deck opened in the background (#350's path, used by the File tab and the recent list) is
/// read on its thread and comes back as the prompt — never as a bare status-bar error.
#[test]
fn a_locked_deck_opened_in_the_background_asks_for_the_password() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    app.open_path_in_background(ENCRYPTED);
    assert!(app.opening.is_some(), "the read runs on a thread");
    finish_opening(&mut app);
    let d = app.dialog.as_ref().expect("the prompt is up");
    assert_eq!(d.id, "password");
    assert_eq!(d.params.get("name").and_then(Value::as_str), Some(ENCRYPTED));
    assert!(app.status.is_none(), "a locked file is a question, not an error to report");
    assert!(app.session.documents().is_empty(), "nothing is opened yet");

    // The prompt answers it: the deck opens, with no path to save over, and warns about saving.
    app.run("file.open", json!({"path": ENCRYPTED, "password": PASSWORD})).unwrap();
    assert!(app.dialog.is_none(), "the prompt is dismissed");
    let doc = app.session.active().expect("the deck opened");
    assert_eq!(doc.doc.slides.len(), 3);
    assert_eq!(doc.path, None);
    assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("can't encrypt on save")), "{:?}", app.toast.map(|t| t.text));
}

/// Poll the background read until it finishes: the derivation can take a while on a slow machine.
fn finish_opening(app: &mut SlideApp) {
    for _ in 0..500 {
        app.poll_opening();
        if app.opening.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(app.opening.is_none(), "the background read finished");
}

/// The retry after a rejection runs on a worker thread too: a spin count of 100,000 must not freeze
/// the window, and the prompt comes back with the text typed and the reason under it.
#[test]
fn a_password_retry_runs_on_a_worker_thread() {
    let mut app = SlideApp::new(Session::new(), Services::default());
    let started = std::time::Instant::now();
    app.open_path_with_password_in_background(ENCRYPTED, "nope");
    assert!(app.opening.is_some(), "the retry is a background read, not a synchronous one");
    assert!(started.elapsed() < std::time::Duration::from_secs(1), "the call returned at once");
    finish_opening(&mut app);

    let d = app.dialog.as_ref().expect("the prompt is back");
    assert_eq!(d.id, "password");
    assert_eq!(d.params.get("path").and_then(Value::as_str), Some(ENCRYPTED));
    assert_eq!(d.fields.get("password").map(String::as_str), Some("nope"), "the typed text is kept");
    assert_eq!(d.fields.get("error").map(String::as_str), Some("Incorrect password. Please try again."));
    assert!(app.status.is_none(), "the reason belongs under the field");
    assert!(app.session.documents().is_empty(), "a wrong password opens nothing");

    // The same route with the right password opens the deck, with no file to save over.
    app.open_path_with_password_in_background(ENCRYPTED, PASSWORD);
    finish_opening(&mut app);
    assert!(app.dialog.is_none(), "the prompt is dismissed");
    let doc = app.session.active().expect("the deck opened");
    assert_eq!(doc.doc.slides.len(), 3);
    assert_eq!(doc.path, None);
}
