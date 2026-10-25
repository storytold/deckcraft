//! Status bar (slide counter, language, notes/comments toggles, view buttons, zoom slider) and the
//! notes pane under the slide.

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, Ui, pos2, vec2};
use serde_json::json;

use crate::icons::{self, Icon};
use crate::theme::{self, Tokens};
use crate::{SlideApp, ViewMode};

/// The status bar: slide counter, proofing language and status message on the left; notes,
/// comments, views and zoom on the right.
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
        // Left to right: slide counter, language, status message. Each starts at its usual column, or
        // after the one before it when that runs long ("Slide 100 of 120", "… (not checked)").
        let sr = p.text(pos2(rect.min.x + 14.0, rect.center().y), Align2::LEFT_CENTER, &left, theme::font(12.0), t.text_dim);
        shown_text(ui, sr, "status_slide", &left);
        // Where the controls on the right begin; nothing on the left paints or clicks past it.
        let right_x = rect.max.x - 560.0;
        // The selection's proofing language; a click opens Review › Language.
        let (lang, no_proof) = crate::dialogs::selected_language(app);
        let name = crate::i18n::tr(&crate::squiggles::display_lang_name(ui.ctx(), &lang)).to_string()
            + if no_proof { crate::i18n::tr(" (not checked)") } else { "" };
        let lx = (rect.min.x + 120.0).max(sr.max.x + 16.0);
        let lclip = Rect::from_min_max(pos2(lx - 3.0, rect.min.y), pos2(right_x.max(lx - 3.0), rect.max.y));
        let lr = p.with_clip_rect(lclip).text(pos2(lx, rect.center().y), Align2::LEFT_CENTER, &name, theme::font(12.0), t.text_dim);
        let hit = lr.expand(3.0).intersect(lclip);
        if hit.is_positive() {
            let resp = ui.interact(hit, ui.scope_id().with("status_lang"), Sense::click()).on_hover_text(crate::i18n::tr("Language"));
            resp.widget_info(|| named_with_value(crate::i18n::tr("Language"), &name));
            if resp.clicked() {
                app.dialog = Some(crate::dialogs::Dialog::new("language"));
            }
        }
        // Transient status message, clipped before the controls on the right.
        if let Some(msg) = app.fresh_status() {
            let mx = (rect.min.x + 290.0).max(lr.max.x + 16.0);
            let clip = Rect::from_min_max(pos2(mx, rect.min.y), pos2(right_x.max(mx), rect.max.y));
            let mr = p.with_clip_rect(clip).text(pos2(mx, rect.center().y), Align2::LEFT_CENTER, &msg, theme::font(12.0), t.accent);
            shown_text(ui, mr.intersect(clip), "status_message", &msg);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        let mut right = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(pos2(right_x, rect.min.y), pos2(rect.max.x - 8.0, rect.max.y)))
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        right.spacing_mut().item_spacing.x = 4.0;
        if small_icon(&mut right, Icon::FitSmall, "Fit slide to current window", false).clicked() {
            app.ui.zoom = None;
        }
        let pct = (app.canvas_scale * 100.0).round();
        let lbl = right.add(egui::Label::new(egui::RichText::new(format!("{pct}%")).size(12.0).color(t.text_dim)).sense(Sense::click()));
        lbl.widget_info(|| named_with_value("Zoom level", &format!("{pct}%")));
        if lbl.clicked() {
            let _ = app.run("view.zoomDialog", json!({}));
        }
        if small_icon(&mut right, Icon::ZoomIn, "Zoom In", false).clicked() {
            let _ = app.run("view.zoomIn", json!({}));
        }
        // Slider 10%–400% (log scale).
        let mut z = app.ui.zoom.unwrap_or(app.canvas_scale).clamp(0.1, 4.0);
        let mut zp = z * 100.0;
        // egui 0.37 gives a slider its own width and the theme's control height; ask for exactly
        // those so the status row stays consistent with every other control without hard-coding
        // a size the slider would override anyway.
        let slider_size = vec2(right.spacing().slider_width, right.spacing().interact_size.y);
        let s = right.add_sized(slider_size, egui::Slider::new(&mut zp, 10.0..=400.0).logarithmic(true).show_value(false));
        // The status bar has no room for a visible "Zoom" label; name the slider for assistive
        // technology only, leaving its geometry unchanged. Its value is the percentage shown.
        let shown = format!("{}%", zp.round());
        let enabled = s.enabled();
        s.widget_info(|| egui::WidgetInfo { current_text_value: Some(shown.clone()), ..egui::WidgetInfo::slider(enabled, f64::from(zp), "Zoom") });
        if s.changed() {
            z = zp / 100.0;
            app.ui.zoom = Some(z);
        }
        if small_icon(&mut right, Icon::ZoomOut, "Zoom Out", false).clicked() {
            let _ = app.run("view.zoomOut", json!({}));
        }
        right.add_space(8.0);
        let view = app.ui.view;
        if view_icon(&mut right, Icon::ViewShowSmall, "Slide Show", false).clicked() {
            let i = app.session.active().map(|d| d.selection.slide).unwrap_or(0);
            app.start_show(i, false);
        }
        if view_icon(&mut right, Icon::ViewReadingSmall, "Reading View", view == ViewMode::Reading).clicked() {
            let _ = app.run("view.reading", json!({}));
        }
        if view_icon(&mut right, Icon::ViewSorterSmall, "Slide Sorter", view == ViewMode::Sorter).clicked() {
            app.ui.view = ViewMode::Sorter;
        }
        if view_icon(&mut right, Icon::ViewNormalSmall, "Normal", view == ViewMode::Normal).clicked() {
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

/// Draw a small status icon with a tooltip and return its interaction response.
fn small_icon(ui: &mut Ui, icon: Icon, tip: &str, on: bool) -> egui::Response {
    let resp = icon_button(ui, icon, on);
    let tip = crate::i18n::tr(tip);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip)
}

/// A view button: screen readers hear its name and whether its view is the one shown.
fn view_icon(ui: &mut Ui, icon: Icon, tip: &str, on: bool) -> egui::Response {
    let resp = icon_button(ui, icon, on);
    let tip = crate::i18n::tr(tip);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, tip));
    resp.on_hover_text(tip)
}

fn icon_button(ui: &mut Ui, icon: Icon, on: bool) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let (r, resp) = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::click());
    if on || resp.hovered() {
        ui.painter().rect_filled(r, CornerRadius::same(4), if on { t.selected } else { t.hover });
    }
    icons::paint(ui.painter(), Rect::from_center_size(r.center(), vec2(16.0, 16.0)), icon, t.text_dim, false);
    resp
}

/// A clickable status-bar text: screen readers hear `label` as its name and `text` as its value.
fn named_with_value(label: &str, text: &str) -> egui::WidgetInfo {
    egui::WidgetInfo { current_text_value: Some(text.to_string()), ..egui::WidgetInfo::labeled(egui::Role::Button, true, label) }
}

/// Painted status-bar text: screen readers read `text` from a label over `rect`.
fn shown_text(ui: &Ui, rect: Rect, id: &str, text: &str) {
    if rect.is_positive() {
        let resp = ui.interact(rect, ui.scope_id().with(id), Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    }
}

/// Draw a labelled status icon, painting the text directly and exposing the toggle state as its
/// accessible selected state.
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
    // The visible label is painted directly, so name the interactive control explicitly and
    // expose the toggle state as selected: a screen reader hears "Notes, pressed" not "Notes".
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), on, crate::i18n::tr(label)));
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
    let id = egui::Id::unique("notes_edit");
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use deckcraft_engine::Session;
    use egui::accesskit::{Role, Toggled};
    use egui::{Event, Modifiers, PointerButton};
    use egui_kittest::{
        Harness,
        kittest::{NodeT, Queryable},
    };
    use serde_json::json;

    use crate::{Services, SlideApp, ViewMode};

    /// A long proofing language in a narrow window must not take clicks meant for the controls on
    /// the right: Notes toggles the notes pane and the Language dialog stays closed.
    #[test]
    fn a_long_language_name_leaves_the_notes_button_clickable() {
        let mut app = SlideApp::new(Session::with_new(), Services::default());
        app.session.execute("shape.insert", &json!({"preset": "rect", "rect": [80, 120, 400, 100], "text": "hello"})).unwrap();
        app.session.execute("format.language", &json!({"lang": "en-US-x-a-very-long-private-use-tag"})).unwrap();
        app.ui.notes = false;
        let size = egui::vec2(700.0, 800.0);
        let mut h = Harness::builder().with_size(size).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            app,
        );
        h.run_steps(3);
        assert!(!h.state().ui.notes);
        {
            let mut notes = h.query_all_by_role_and_label(Role::Button, "Notes");
            let button = notes.next().expect("the Notes button is there");
            assert!(notes.next().is_none(), "exactly one Notes button, not a duplicated label target");
            assert_eq!(button.accesskit_node().toggled(), Some(Toggled::False), "the Notes button reports its off state to screen readers");
        }
        // The controls on the right start 560 points from the bar's right edge, which is 8 points
        // inside the window (x 132); the Notes button is the leftmost of them. Its position depends
        // on the interface font's widths, so the clicks use the position it was laid out at.
        let right_x = size.x - 8.0 - 560.0;
        let notes = h.get_by_label("Notes").rect();
        assert!(notes.left() > right_x + 2.0, "the Notes button ({notes:?}) starts after x {right_x}");
        let click = |h: &mut Harness<'_, SlideApp>, at: egui::Pos2| {
            h.hover_at(at);
            h.run_steps(1);
            for pressed in [true, false] {
                h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE });
            }
            h.run_steps(3);
        };
        // The gap before the controls is outside the language name's clickable area.
        click(&mut h, egui::pos2((right_x + notes.left()) / 2.0, notes.center().y));
        assert!(h.state().dialog.is_none(), "the Language dialog stayed closed");
        assert!(!h.state().ui.notes, "the click in the gap did not reach the Notes button");
        click(&mut h, notes.center());
        assert!(h.state().ui.notes, "the click reached the Notes button");
        assert_eq!(
            h.get_by_role_and_label(egui::Role::Button, "Notes").accesskit_node().toggled(),
            Some(Toggled::True),
            "the open notes pane is exposed as a pressed Notes button"
        );
        assert!(h.state().dialog.is_none(), "the Language dialog stayed closed");
    }

    /// Screen readers find each status-bar control by name, hear the language and zoom it shows,
    /// hear which view button is on, and read the slide counter and the status message.
    #[test]
    fn status_bar_controls_have_names() {
        let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
            |ui, app: &mut SlideApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            SlideApp::new(Session::with_new(), Services::default()),
        );
        h.run_steps(3);
        let status_y = h.get_by_role_and_label(Role::Button, "Fit slide to current window").rect().center().y;
        for name in ["Fit slide to current window", "Zoom In", "Zoom Out", "Slide Show", "Reading View"] {
            // The ribbon also has a "Slide Show" control. Require exactly one named button
            // in the Fit button's laid-out status row, allowing the app's UI zoom.
            let buttons = h.get_all_by_role_and_label(Role::Button, name).filter(|n| n.rect().y_range().contains(status_y)).count();
            assert_eq!(buttons, 1, "one {name} button in the status bar");
        }
        let pct = format!("{}%", (h.state().canvas_scale * 100.0).round());
        assert_eq!(h.get_by_role_and_label(Role::Button, "Zoom level").value().as_deref(), Some(pct.as_str()));
        let zoom = h.get_by_role_and_label(Role::Slider, "Zoom");
        assert_eq!(zoom.value().as_deref(), Some(pct.as_str()));
        // Its number and range are in percent, as the value text is.
        let node = zoom.accesskit_node();
        assert_eq!(node.numeric_value().map(f64::round), Some((h.state().canvas_scale * 100.0).round().into()));
        assert_eq!((node.min_numeric_value(), node.max_numeric_value()), (Some(10.0), Some(400.0)));
        assert_eq!(h.get_by_role_and_label(Role::Button, "Language").value().as_deref(), Some("English (United States)"));
        let toggled = |h: &Harness<'_, SlideApp>, name: &str| h.get_by_role_and_label(Role::Button, name).accesskit_node().toggled();
        assert_eq!(toggled(&h, "Normal"), Some(Toggled::True));
        assert_eq!(toggled(&h, "Slide Sorter"), Some(Toggled::False));
        h.get_by_role_and_label(Role::Button, "Slide Sorter").click();
        h.run_steps(3);
        assert_eq!(h.state().ui.view, ViewMode::Sorter, "the click by name reached the Slide Sorter button");
        assert_eq!(toggled(&h, "Slide Sorter"), Some(Toggled::True));
        assert_eq!(toggled(&h, "Normal"), Some(Toggled::False));
        assert_eq!(toggled(&h, "Reading View"), Some(Toggled::False));
        h.state_mut().ui.view = ViewMode::Reading;
        h.run_steps(3);
        assert_eq!(toggled(&h, "Reading View"), Some(Toggled::True));
        assert_eq!(toggled(&h, "Slide Sorter"), Some(Toggled::False));
        assert_eq!(h.get_by_value("Slide 1 of 1").accesskit_node().role(), Role::Label);
        h.state_mut().set_status("Saved a copy");
        h.run_steps(3);
        assert_eq!(h.get_by_value("Saved a copy").accesskit_node().role(), Role::Label);
    }
}
