use egui::Color32;

use super::{Dialog, SlideApp, Ui, Value, buttons, vec2};

/// What the user is told when a deck opened with a password is saved: DeckCraft cannot write an
/// encrypted package, so saving drops the protection the file had.
pub(crate) fn drops_password_note(name: &str) -> String {
    let label = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
    format!("{label}: opened decrypted — DeckCraft can't encrypt on save, so Save will ask for a new file name; the original stays protected")
}

/// The message under a rejected password field. The engine reports a bad password as
/// `<name>: incorrect password`; any other failure is not the password's fault and is shown as is.
pub(crate) fn rejected_password(name: &str, err: &str) -> String {
    match err.strip_prefix(name).and_then(|rest| rest.strip_prefix(": ")) {
        Some("incorrect password") => "Incorrect password. Please try again.".into(),
        _ => err.to_string(),
    }
}

/// Returns true when the dialog should close.
pub(super) fn password_dialog(app: &mut SlideApp, ui: &mut Ui, d: &mut Dialog) -> bool {
    let name = d.params.get("name").and_then(Value::as_str).unwrap_or("Presentation").to_string();
    let file_label = std::path::Path::new(&name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.clone());
    ui.label(format!("“{file_label}” is protected by a password."));
    ui.add_space(4.0);
    let mut pwd = d.get("password", "");
    ui.horizontal(|ui| {
        ui.add_sized(vec2(80.0, 20.0), egui::Label::new("Password:"));
        let resp = ui.add(egui::TextEdit::singleline(&mut pwd).password(true).desired_width(200.0));
        if !d.fields.contains_key("focused") {
            resp.request_focus();
            d.fields.insert("focused".into(), "1".into());
        }
    });
    d.fields.insert("password".into(), pwd.clone());

    if let Some(err) = d.fields.get("error").cloned() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(err).color(Color32::from_rgb(0xD9, 0x38, 0x3A)));
    }

    let (ok, cancel) = buttons(ui, "Open");
    if !ok {
        return cancel;
    }
    let opened = d.params.get("path").and_then(Value::as_str).map(String::from);
    let data = d.params.get("data").and_then(Value::as_str).map(String::from);
    open_with_password(app, d, &name, &pwd, opened, data)
}

/// Desktop: hand the open to the worker thread (a key derivation can take seconds), so a rejected
/// password comes back through `poll_opening` as this prompt again, with the text typed.
#[cfg(not(target_arch = "wasm32"))]
fn open_with_password(app: &mut SlideApp, d: &mut Dialog, name: &str, pwd: &str, opened: Option<String>, data: Option<String>) -> bool {
    match (opened, data) {
        (Some(path), _) => {
            app.open_path_with_password_in_background(&path, pwd);
            true
        }
        (None, Some(data)) => match deckcraft_engine::cmd::base64_decode(&data) {
            Some(bytes) => {
                app.open_bytes_with_password_in_background(name, &bytes, pwd);
                true
            }
            None => {
                d.fields.insert("error".into(), "This file's data could not be read.".into());
                false
            }
        },
        (None, None) => {
            d.fields.insert("error".into(), "missing file path or data".into());
            false
        }
    }
}

/// The web build has no threads: read here, so an inline error still works.
#[cfg(target_arch = "wasm32")]
fn open_with_password(app: &mut SlideApp, d: &mut Dialog, name: &str, pwd: &str, opened: Option<String>, data: Option<String>) -> bool {
    use super::json;
    let res = match (&opened, data) {
        (Some(path), _) => app.run("file.open", json!({"path": path, "password": pwd})),
        (None, Some(data)) => app.run("file.openBytes", json!({"name": name, "data": data, "password": pwd})),
        (None, None) => Err("missing file path or data".into()),
    };
    match res {
        Ok(_) => {
            if let Some(path) = &opened {
                app.note_recent(path);
            }
            true
        }
        Err(e) => {
            // `run` put the failure in the status bar: this one belongs under the field the user is
            // typing in, and the dialog is drawn over the bar.
            app.status = None;
            d.fields.insert("error".into(), rejected_password(name, &e));
            d.fields.remove("focused");
            false
        }
    }
}
