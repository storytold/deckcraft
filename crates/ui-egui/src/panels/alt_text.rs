//! Alt Text pane (Camera Format ▸ Accessibility): the description a screen reader reads for the
//! selected object, and whether it is decorative. Edits go through `shape.altText`, so each commit
//! is one undo step and the description is written to `p:cNvPr@descr` on save.

use egui::Ui;
use serde_json::json;

use crate::SlideApp;
use crate::state::AltTextDraft;
use crate::theme::Tokens;
use crate::widgets;

use super::run;

/// The object the pane would describe now, if the selection has one.
fn selected(app: &SlideApp) -> Option<(u64, u32)> {
    let st = app.session.active()?;
    let id = st.selected_shapes().first()?.id.0;
    Some((st.uid, id))
}

/// Commit a description the pane holds but the document does not, when the pane is closing, a
/// command is about to move its object, or another object takes over. The field cannot report
/// `lost_focus` once it is no longer drawn, so without this a typed description would be dropped.
pub(crate) fn flush(app: &mut SlideApp) {
    let Some(draft) = app.ui.alt_text_draft.clone() else { return };
    let Some(st) = app.session.active() else { return };
    let id = deckcraft_model::ShapeId(draft.shape);
    if st.uid != draft.uid {
        // The document the description belongs to is gone; there is nothing left to write it to.
        app.ui.alt_text_draft = None;
        return;
    }
    let Some(sh) = st.shape(id) else {
        // The object is on a slide the user has since opened elsewhere in the document: keep the
        // draft for the pass that is back on it, and drop it only once the object is gone.
        if !st.doc.slides.iter().any(|sl| sl.shape(id).is_some()) {
            app.ui.alt_text_draft = None;
        }
        return;
    };
    // Only a draft typed over the description the document still holds is written: an undo or
    // another edit wins, and a draft equal to the document's text is nothing to write.
    if sh.descr != draft.base || draft.text == draft.base {
        app.ui.alt_text_draft = None;
        return;
    }
    app.ui.alt_text_draft = None;
    run(app, "shape.altText", json!({ "text": draft.text, "ids": [draft.shape] }));
}

pub(super) fn alt_text(app: &mut SlideApp, ui: &mut Ui) {
    app.ui.alt_text_shown = true;
    let t = Tokens::get(ui.ctx());
    // The draft belongs to the object it was typed for: another selection commits it first.
    let keeps = match (&app.ui.alt_text_draft, selected(app)) {
        (Some(d), Some((uid, id))) => d.uid == uid && d.shape == id,
        _ => false,
    };
    if !keeps {
        flush(app);
    }
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
    let stored = app.ui.alt_text_draft.as_ref().filter(|d| d.uid == uid && d.shape == sh.id.0).map(|d| (d.base.clone(), d.text.clone()));
    let mut draft = widgets::text_draft(stored, &sh.descr);
    let l = ui.label(egui::RichText::new("Description").color(t.text_dim));
    let r = ui
        .add_enabled(!sh.decorative, egui::TextEdit::multiline(&mut draft).hint_text("Alt text").desired_rows(5).desired_width(f32::INFINITY))
        .labelled_by(l.id);
    let text_commit = r.lost_focus() && draft != sh.descr;
    app.ui.alt_text_draft = Some(AltTextDraft { uid, shape: sh.id.0, base: sh.descr.clone(), text: draft.clone() });

    ui.add_space(6.0);
    let mut deco = sh.decorative;
    let toggled = ui.checkbox(&mut deco, "Mark as decorative").changed();
    // One command carries the typed description and the flag: the click that takes the field's
    // focus must not spend two undo steps.
    if toggled {
        run(app, "shape.altText", json!({ "text": draft, "decorative": deco, "ids": [sh.id.0] }));
    } else if text_commit {
        run(app, "shape.altText", json!({ "text": draft, "ids": [sh.id.0] }));
    }
    if sh.decorative || toggled && deco {
        ui.label(egui::RichText::new("A decorative object needs no description: screen readers skip it.").color(t.text_dim));
    }
}
