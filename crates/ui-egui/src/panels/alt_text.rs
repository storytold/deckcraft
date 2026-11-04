//! Alt Text pane (Camera Format ▸ Accessibility): the description a screen reader reads for the
//! selected object, and whether it is decorative. Edits go through `shape.altText`, so each commit
//! is one undo step and the description is written to `p:cNvPr@descr` on save.

use egui::Ui;
use serde_json::json;

use crate::SlideApp;
use crate::theme::Tokens;
use crate::widgets;

use super::run;

pub(super) fn alt_text(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    let uid = st.uid;
    let Some(sh) = st.selected_shapes().first().map(|s| (*s).clone()) else {
        ui.label(egui::RichText::new("Select an object to describe it.").color(t.text_dim));
        return;
    };
    let what = if sh.is_cameo() { "camera" } else { "object" };
    ui.label(format!("How would you describe this {what} and its context to someone who is blind or has low vision?"));
    ui.add_space(4.0);

    // The field keeps what is typed while the document still holds the text it started from, as
    // the Format Shape pane's does; an undo or another edit replaces the draft.
    let id = ui.scope_id().with(("alt_text_pane", uid, sh.id.0));
    let stored: Option<(String, String)> = ui.data_mut(|d| d.get_temp(id));
    let mut draft = widgets::text_draft(stored, &sh.descr);
    let l = ui.label(egui::RichText::new("Description").color(t.text_dim));
    let r = ui
        .add_enabled(!sh.decorative, egui::TextEdit::multiline(&mut draft).hint_text("Alt text").desired_rows(5).desired_width(f32::INFINITY))
        .labelled_by(l.id);
    if r.lost_focus() && draft != sh.descr {
        run(app, "shape.altText", json!({ "text": draft.clone(), "ids": [sh.id.0] }));
    }
    ui.data_mut(|d| d.insert_temp(id, (sh.descr.clone(), draft.clone())));

    ui.add_space(6.0);
    let mut deco = sh.decorative;
    if ui.checkbox(&mut deco, "Mark as decorative").changed() {
        run(app, "shape.altText", json!({ "text": draft, "decorative": deco, "ids": [sh.id.0] }));
    }
    if sh.decorative {
        ui.label(egui::RichText::new("A decorative object needs no description: screen readers skip it.").color(t.text_dim));
    }
}
