use super::{SlideApp, Tokens, Ui, json, run, theme};

/// Review › Thesaurus: look a word up; clicking a synonym puts it in place of the selected word
/// when editing text, otherwise looks that synonym up in turn.
pub(super) fn thesaurus(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let mut q = app.thesaurus_query.clone();
    let edit = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search for a word").desired_width(f32::INFINITY));
    if edit.changed() {
        app.thesaurus_query = q.clone();
    }
    ui.add_space(6.0);
    let senses = deckcraft_engine::thesaurus::lookup(&q);
    if q.trim().is_empty() {
        ui.label(egui::RichText::new("Select a word on the slide, or type one above.").color(t.text_dim));
        return;
    }
    if senses.is_empty() {
        ui.label(egui::RichText::new(format!("No synonyms for \"{}\".", q.trim())).color(t.text_dim));
        return;
    }
    let editing = app.session.active().is_some_and(|d| d.selection.text.is_some());
    for sense in senses {
        ui.label(egui::RichText::new(&sense.word).font(theme::bold(13.0)));
        for syn in sense.synonyms {
            let r = ui.add(egui::Button::new(&syn).frame(false)).on_hover_text(if editing { "Insert" } else { "Look up" });
            if r.clicked() {
                if editing {
                    run(app, "review.replaceWord", json!({"with": syn}));
                }
                app.thesaurus_query = syn;
            }
        }
        ui.add_space(6.0);
    }
}
