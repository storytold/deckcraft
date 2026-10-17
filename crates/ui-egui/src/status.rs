//! Status bar (slide counter, language, notes/comments toggles, view buttons, zoom slider) and the
//! notes pane under the slide.

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::{SlideApp, ViewMode};

pub fn status_bar(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::Panel::bottom("status").exact_size(28.0).frame(egui::Frame::NONE.fill(t.chrome)).show(ui, |ui| {
        let rect = ui.max_rect();
        let Some(st) = app.session.active() else { return };
        let n = st.doc.slides.len();
        let left = match st.selection.target {
            deckcraft_engine::Target::Slides => {
                crate::i18n::format("Slide {} of {}", &[((st.selection.slide + 1).min(n.max(1))).to_string(), (n).to_string()])
            }
            _ => crate::i18n::tr("Slide Master").to_string(),
        };
        let p = ui.painter();
        p.text(pos2(rect.min.x + 14.0, rect.center().y), Align2::LEFT_CENTER, &left, theme::font(12.0), t.text_dim);
        // The selection's proofing language; a click opens Review › Language.
        let (lang, no_proof) = crate::dialogs::selected_language(app);
        let name = crate::i18n::tr(&crate::squiggles::lang_name(&lang)).to_string() + if no_proof { crate::i18n::tr(" (not checked)") } else { "" };
        let lr = p.text(pos2(rect.min.x + 120.0, rect.center().y), Align2::LEFT_CENTER, &name, theme::font(12.0), t.text_dim);
        if ui.interact(lr.expand(3.0), ui.id().with("status_lang"), Sense::click()).on_hover_text(crate::i18n::tr("Language")).clicked() {
            app.dialog = Some(crate::dialogs::Dialog::new("language"));
        }
        // Transient status message.
        if let Some(msg) = app.fresh_status() {
            p.text(pos2(rect.min.x + 290.0, rect.center().y), Align2::LEFT_CENTER, &msg, theme::font(12.0), t.accent);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(rect.max.x - 560.0, rect.min.y), pos2(rect.max.x - 8.0, rect.max.y)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 4.0;
        if small_icon(&mut right, Icon::FitSmall, "Fit slide to current window", false).clicked() {
            app.ui.zoom = None;
        }
        let pct = (app.canvas_scale * 100.0).round();
        let lbl = right.add(egui::Label::new(egui::RichText::new(format!("{pct}%")).size(12.0).color(t.text_dim)).sense(Sense::click()));
        if lbl.clicked() {
            let _ = app.run("view.zoomDialog", json!({}));
        }
        if small_icon(&mut right, Icon::ZoomIn, "Zoom In", false).clicked() {
            let _ = app.run("view.zoomIn", json!({}));
        }
        // Slider 10%–400% (log scale).
        let mut z = app.ui.zoom.unwrap_or(app.canvas_scale).clamp(0.1, 4.0);
        let mut lz = z.ln();
        let s = right.add_sized(vec2(110.0, 18.0), egui::Slider::new(&mut lz, (0.1f32).ln()..=(4.0f32).ln()).show_value(false));
        if s.changed() {
            z = lz.exp();
            app.ui.zoom = Some(z);
        }
        if small_icon(&mut right, Icon::ZoomOut, "Zoom Out", false).clicked() {
            let _ = app.run("view.zoomOut", json!({}));
        }
        right.add_space(8.0);
        let view = app.ui.view;
        if small_icon(&mut right, Icon::ViewShowSmall, "Slide Show", false).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
        }
        if small_icon(&mut right, Icon::ViewReadingSmall, "Reading View", false).clicked() {
            let _ = app.run("view.reading", json!({}));
        }
        if small_icon(&mut right, Icon::ViewSorterSmall, "Slide Sorter", view == ViewMode::Sorter).clicked() {
            app.ui.view = ViewMode::Sorter;
        }
        if small_icon(&mut right, Icon::ViewNormalSmall, "Normal", view == ViewMode::Normal).clicked() {
            app.ui.view = ViewMode::Normal;
        }
        right.add_space(8.0);
        let c_on = app.ui.pane.as_deref() == Some("comments");
        if labeled(&mut right, Icon::CommentsSmall, "Comments", c_on).clicked() {
            let _ = app.run("view.pane", json!({"pane": "comments", "toggle": true}));
        }
        let n_on = app.ui.notes;
        if labeled(&mut right, Icon::NotesSmall, "Notes", n_on).clicked() {
            app.ui.notes = !n_on;
        }
        let _ = Stroke::NONE;
    });
}

fn small_icon(ui: &mut Ui, icon: Icon, tip: &str, on: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(4), if on { t.selected } else { t.hover });
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, t.text_dim, false);
    resp.on_hover_text(crate::i18n::tr(tip))
}

fn labeled(ui: &mut Ui, icon: Icon, label: &str, on: bool) -> egui::Response {
    let label = crate::i18n::tr(label);
    let t = Tokens::get(ui.ctx());
    let w = 22.0 + ui.fonts_mut(|f| f.layout_no_wrap(label.into(), theme::font(12.0), t.text).size().x) + 8.0;
    let (r, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(4), if on { t.selected } else { t.hover });
    }
    icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 11.0, r.center().y), vec2(15.0, 15.0)), icon, t.text_dim, false);
    ui.painter().text(pos2(r.min.x + 22.0, r.center().y), Align2::LEFT_CENTER, crate::i18n::tr(label), theme::font(12.0), t.text_dim);
    resp
}

/// The language of a slide's notes: the first notes run that has one.
fn notes_lang(s: &deckcraft_model::Slide) -> Option<String> {
    s.notes.paragraphs.iter().flat_map(|p| &p.runs).find_map(|r| r.props.lang.clone())
}

/// The speaker notes editor under the slide.
pub fn notes_pane(app: &mut SlideApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let Some(st) = app.session.active() else { return };
    if st.selection.target != deckcraft_engine::Target::Slides {
        return;
    }
    let (idx, uid) = (st.selection.slide, st.uid);
    // Save an edited draft once the field has lost focus or another slide is shown (before
    // anything reloads it), to the slide it was written on.
    let id = egui::Id::new("notes_edit");
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    if app.notes_buf.2.edited && (!focused || app.notes_buf.0 != uid || app.notes_buf.1 != idx) {
        app.notes_buf.2.edited = false;
        if app.notes_buf.0 == uid {
            let _ = app.run("slide.notes", json!({"body": app.notes_buf.2.body, "index": app.notes_buf.1}));
        }
    }
    let notes = app.session.active().and_then(|st| st.current_slide()).map(|s| s.notes.clone()).unwrap_or_default();
    egui::Panel::bottom("notes")
        .resizable(true)
        .default_size(app.ui.notes_height)
        .size_range(36.0..=400.0)
        .frame(egui::Frame::NONE.fill(t.chrome).inner_margin(egui::Margin::symmetric(8, 6)))
        .show(ui, |ui| {
            app.ui.notes_height = ui.available_height() + 12.0;
            let rect = ui.max_rect();
            ui.painter()
                .line_segment([pos2(rect.min.x - 8.0, rect.min.y - 6.0), pos2(rect.max.x + 8.0, rect.min.y - 6.0)], Stroke::new(1.0, t.separator));
            // Keep a local draft per slide, following the notes while the field isn't focused.
            if app.notes_buf.0 != uid || app.notes_buf.1 != idx || (!focused && app.notes_buf.2.body != notes) {
                app.notes_buf = (uid, idx, crate::notes::Draft::new(&notes));
            }
            let mut draft = app.notes_buf.2.clone();
            let look = crate::notes::Look { size: 13.0, color: None };
            // Long notes scroll inside the pane instead of spilling over the slide (#221).
            let out = egui::ScrollArea::vertical()
                .max_height(ui.available_height())
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    crate::notes::editor(ui, &mut draft, &look, |e| {
                        e.id(id)
                            .hint_text(crate::i18n::tr("Click to add notes"))
                            .frame(egui::Frame::NONE)
                            .desired_width(f32::INFINITY)
                            .desired_rows(2)
                    })
                })
                .inner;
            let r = out.response.response.clone();
            let mut buf = draft.text.clone();
            // Spelling squiggles under misspelt words of the notes.
            let lang = app.session.active().and_then(|d| d.doc.slides.get(idx)).and_then(|s| notes_lang(s)).unwrap_or_else(|| "en-US".into());
            let caret = out.cursor_range.map(|c| c.primary.index.0);
            for (a, b) in app.squiggles.notes(&buf, &lang, caret, Some(ui.ctx())) {
                let (ra, rb) = (out.galley.pos_from_cursor(egui::text::CCursor::new(a)), out.galley.pos_from_cursor(egui::text::CCursor::new(b)));
                // One underline per row the word is on (a word doesn't wrap, so normally one).
                if (ra.bottom() - rb.bottom()).abs() < 1.0 {
                    let y = ra.bottom() - 1.0;
                    let o = out.galley_pos.to_vec2();
                    crate::squiggles::paint(ui.painter(), egui::pos2(ra.left(), y) + o, egui::pos2(rb.left(), y) + o, 1.0);
                }
            }
            // Right-click on a flagged word: its corrections, Ignore All, Add to Dictionary.
            if r.secondary_clicked()
                && let Some(pos) = r.interact_pointer_pos()
            {
                let at = out.galley.cursor_from_pos(pos - out.galley_pos).index.0;
                let hit = app.squiggles.notes(&buf, &lang, None, None).into_iter().find(|(a, b)| at >= *a && at <= *b);
                app.squiggles.notes_menu = hit.and_then(|(a, _)| {
                    let before: String = buf.chars().take(a).collect();
                    let line_start = before.rfind('\n').map_or(0, |i| before[..i].chars().count() + 1);
                    let line = buf.chars().skip(line_start).take_while(|c| *c != '\n').collect::<String>();
                    crate::squiggles::menu_word(
                        &deckcraft_model::Paragraph::new(&line),
                        &lang,
                        crate::squiggles::Tag::Notes { start: line_start },
                        a - line_start,
                    )
                });
            }
            let mut changed: Option<String> = None;
            if app.squiggles.notes_menu.is_some() {
                r.context_menu(|ui| {
                    let Some(w) = app.squiggles.notes_menu.clone() else { return };
                    let crate::squiggles::Tag::Notes { start: ls } = w.tag else { return };
                    let chars: Vec<char> = buf.chars().collect();
                    let end = ls + w.start + w.word.chars().count();
                    let splice = |from: usize, to: &str| -> String {
                        chars[..from.min(chars.len())]
                            .iter()
                            .chain(to.chars().collect::<Vec<_>>().iter())
                            .chain(chars[end.min(chars.len())..].iter())
                            .collect()
                    };
                    if let Some(from) = w.delete_from {
                        if ui.button(egui::RichText::new("Delete Repeated Word").strong()).clicked() {
                            changed = Some(splice(ls + from, ""));
                            ui.close();
                        }
                    } else if w.suggestions.is_empty() {
                        ui.label(egui::RichText::new("(no spelling suggestions)").weak());
                    }
                    for to in &w.suggestions {
                        if ui.button(egui::RichText::new(to).strong()).clicked() {
                            changed = Some(splice(ls + w.start, to));
                            ui.close();
                        }
                    }
                    if ui.button("Ignore All").clicked() {
                        app.squiggles.ignore(&w.word);
                        ui.close();
                    }
                    if w.delete_from.is_none() && ui.button("Add to Dictionary").clicked() {
                        if let Err(e) = app.session.execute("review.addToDictionary", &json!({"word": w.word})) {
                            app.set_status(e.to_string());
                        }
                        ui.close();
                    }
                });
            }
            if let Some(new) = changed {
                // A correction is typed into the formatted draft, then saved.
                buf = new;
                app.squiggles.notes_menu = None;
                draft.text = buf.clone();
                deckcraft_model::edit::replace_text(&mut draft.body, &buf);
                draft.edited = false;
                let _ = app.run("slide.notes", json!({"body": draft.body, "index": idx}));
            }
            if r.lost_focus() && draft.edited {
                draft.edited = false;
                let _ = app.run("slide.notes", json!({"body": draft.body, "index": idx}));
            }
            app.notes_buf.2 = draft;
        });
}
