//! Presenter notes for one slide: locked while the show keys are in use, and a small
//! editor (F2) whose size, colour and emphasis are stored on that slide alone.

use deckcraft_color::Rgba;
use deckcraft_model::Presentation;
use deckcraft_model::edit;
use deckcraft_model::style::{ColorRef, Fill};
use deckcraft_model::text::{Paragraph, RunProps, Strike, TextBody};
use egui::{Color32, Ui, vec2};

use super::Show;
use super::presenter::NOTES_STEP;
use crate::notes::{Draft, Look};

const SIZE_MIN: f64 = 8.0;
const SIZE_MAX: f64 = 96.0;
const SIZE_STEP: f64 = 2.0;

/// One formatting change. With no selection it covers the whole slide.
#[derive(Clone, Copy)]
pub(super) enum Style {
    Bold,
    Italic,
    Underline,
    Strike,
    /// Added to each run's point size.
    Size(f64),
    /// The point size, as Home ▸ Font Size sets it.
    Point(f64),
    /// `None` clears the colour so the notes use the presenter grey.
    Color(Option<Rgba>),
}

/// Apply `style` to the selected characters, or to every run when nothing is selected.
pub(super) fn restyle(body: &mut TextBody, selection: Option<(usize, usize)>, style: Style) {
    if body.paragraphs.is_empty() {
        body.paragraphs.push(Paragraph::default());
    }
    let anchor = anchor_props(body, selection);
    let range = selection.filter(|(a, b)| a != b).map(|(a, b)| {
        let (from, to) = if a < b { (a, b) } else { (b, a) };
        (char_pos(body, from), char_pos(body, to))
    });
    let apply = |body: &mut TextBody, f: &dyn Fn(&mut RunProps)| {
        if let Some((a, b)) = range {
            edit::format(body, a, b, f);
        } else {
            edit::format_all(body, f);
        }
    };
    match style {
        Style::Bold => {
            let on = anchor.bold != Some(true);
            apply(body, &|p| p.bold = Some(on));
        }
        Style::Italic => {
            let on = anchor.italic != Some(true);
            apply(body, &|p| p.italic = Some(on));
        }
        Style::Underline => {
            let on = underline_on(&anchor);
            apply(body, &|p| p.underline = Some(if on { "none".into() } else { "sng".into() }));
        }
        Style::Strike => {
            let on = matches!(anchor.strike, Some(Strike::Single | Strike::Double));
            apply(body, &|p| p.strike = Some(if on { Strike::None } else { Strike::Single }));
        }
        Style::Size(delta) => apply(body, &|p| {
            let cur = point_size(p);
            p.size = Some((cur + delta).clamp(SIZE_MIN, SIZE_MAX));
        }),
        Style::Point(pt) => {
            let pt = if pt.is_finite() { pt.clamp(1.0, 4000.0) } else { 12.0 };
            apply(body, &|p| p.size = Some(pt));
        }
        Style::Color(None) => apply(body, &|p| p.fill = None),
        Style::Color(Some(rgb)) => apply(body, &|p| p.fill = Some(Fill::solid(ColorRef::rgb(rgb)))),
    }
}

/// Set the font family on the selection, or on every run when nothing is selected.
pub(super) fn restyle_font(body: &mut TextBody, selection: Option<(usize, usize)>, family: &str) {
    let family = family.to_string();
    apply_runs(body, selection, &|p| p.font = Some(family.clone()));
}

fn apply_runs(body: &mut TextBody, selection: Option<(usize, usize)>, f: &dyn Fn(&mut RunProps)) {
    if body.paragraphs.is_empty() {
        body.paragraphs.push(Paragraph::default());
    }
    let range = selection.filter(|(a, b)| a != b).map(|(a, b)| {
        let (from, to) = if a < b { (a, b) } else { (b, a) };
        (crate::notes::char_pos(body, from), crate::notes::char_pos(body, to))
    });
    if let Some((a, b)) = range {
        edit::format(body, a, b, f);
    } else {
        edit::format_all(body, f);
    }
}

/// The notes column under the next-slide preview.
///
/// Done and F2 Edit take effect on the next frame, so the click doesn't swap the
/// whole column out from under the pointer.
pub(super) fn pane(ui: &mut Ui, show: &mut Show, slide: usize, notes: &TextBody, doc: &Presentation) {
    prepare(show, slide, notes);
    if show.notes_editing {
        let id = editor_id(slide);
        if edit_bar(ui, show, id, doc) {
            show.notes_editing = false;
        }
        edit_body(ui, show, id);
        return;
    }
    let fresh = take_draft(show, slide);
    read_bar(ui, show);
    show_read(ui, show, notes, fresh.as_ref());
}

fn show_read(ui: &mut Ui, show: &Show, notes: &TextBody, fresh: Option<&TextBody>) {
    match fresh {
        Some(body) => read_notes(ui, show, body),
        None => read_notes(ui, show, notes),
    }
}

/// Keep a draft for the slide on screen, and remember the previous slide's notes when it changes.
fn prepare(show: &mut Show, slide: usize, notes: &TextBody) {
    if !show.notes_editing {
        return;
    }
    let switched = show.notes_draft.as_ref().is_some_and(|(at, _)| *at != slide);
    if switched
        && let Some((prev, draft)) = show.notes_draft.take()
        && draft.edited
    {
        show.notes_save = Some((prev, draft.body));
    }
    if show.notes_draft.is_none() {
        show.notes_draft = Some((slide, Draft::new(notes)));
    }
}

/// Move a finished draft onto the save queue. The show writes it after drawing.
pub(super) fn flush_draft(show: &mut Show) {
    if let Some((index, draft)) = show.notes_draft.take()
        && draft.edited
    {
        show.notes_save = Some((index, draft.body));
    }
}

fn take_draft(show: &mut Show, slide: usize) -> Option<TextBody> {
    let (prev, draft) = show.notes_draft.take()?;
    if draft.edited {
        show.notes_save = Some((prev, draft.body.clone()));
    }
    (prev == slide).then_some(draft.body)
}

fn read_bar(ui: &mut Ui, show: &mut Show) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new("Notes").size(13.0).color(Color32::from_gray(170)));
        let edit = bar_button(ui, "F2  Edit", false).on_hover_text("Type and format this slide's notes. Other slides stay as they are.");
        if edit.clicked() {
            show.notes_editing = true;
            show.notes_focus = true;
        }
        ui.add_space(16.0);
        ui.label(egui::RichText::new("Zoom").size(13.0).color(Color32::from_gray(170)))
            .on_hover_text("Reading size for every slide. [ and ] change it too.");
        if bar_button(ui, "A-", false).on_hover_text("Smaller reading size").clicked() {
            show.nudge_notes(-NOTES_STEP);
        }
        ui.label(egui::RichText::new(format!("{}", show.notes_px.round() as i32)).size(13.0).color(Color32::from_gray(200)));
        if bar_button(ui, "A+", false).on_hover_text("Larger reading size").clicked() {
            show.nudge_notes(NOTES_STEP);
        }
    });
}

fn read_notes(ui: &mut Ui, show: &Show, notes: &TextBody) {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        super::presenter::presenter_notes(ui, notes, show.notes_px);
    });
}

/// Draw the formatting bar. True when Done was pressed.
fn edit_bar(ui: &mut Ui, show: &mut Show, id: egui::Id, doc: &Presentation) -> bool {
    let selection = text_selection(ui, id);
    if let Some((style, modifiers, key)) = command_style(ui)
        && let Some((_, draft)) = show.notes_draft.as_mut()
    {
        touch(draft, selection, style);
        ui.input_mut(|i| i.consume_key(modifiers, key));
    }
    let props = show.notes_draft.as_ref().map(|(_, draft)| anchor_props(&draft.body, selection)).unwrap_or_default();
    let mut close = false;
    ui.horizontal_wrapped(|ui| {
        let done = bar_button(ui, "Done", false).on_hover_text("Stop editing (Esc or F2). This slide keeps what you changed.");
        if done.clicked() {
            close = true;
        }
        let bold = bar_button(ui, "B", props.bold == Some(true)).on_hover_text("Bold (Ctrl+B)");
        let italic = bar_button(ui, "I", props.italic == Some(true)).on_hover_text("Italic (Ctrl+I)");
        let underline = bar_button(ui, "U", underline_on(&props)).on_hover_text("Underline (Ctrl+U)");
        let strike = bar_button(ui, "S", matches!(props.strike, Some(Strike::Single | Strike::Double))).on_hover_text("Strikethrough");
        if bold.clicked() {
            touch_show(show, selection, Style::Bold);
        }
        if italic.clicked() {
            touch_show(show, selection, Style::Italic);
        }
        if underline.clicked() {
            touch_show(show, selection, Style::Underline);
        }
        if strike.clicked() {
            touch_show(show, selection, Style::Strike);
        }
        ui.add_space(8.0);
        let family = props.font.clone().unwrap_or_default();
        let font_label = if family.is_empty() { "Font".to_string() } else { short_label(&family) };
        let font_btn = bar_button(ui, &font_label, false).on_hover_text("Font, the same list as a slide");
        egui::Popup::menu(&font_btn).show(|ui| {
            egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                for (k, f) in note_font_names(doc).iter().enumerate() {
                    let text = if k == 0 {
                        format!("{f} (Headings)")
                    } else if k == 1 {
                        format!("{f} (Body)")
                    } else {
                        f.clone()
                    };
                    if ui.button(text).clicked() {
                        restyle_font_show(show, selection, f);
                        ui.close();
                    }
                    if k == 1 {
                        ui.separator();
                    }
                }
            });
        });
        ui.add_space(8.0);
        if bar_button(ui, "−", false).on_hover_text("Smaller text on this slide").clicked() {
            touch_show(show, selection, Style::Size(-SIZE_STEP));
        }
        let size_btn = bar_button(ui, &format!("{}", point_size(&props) as i32), false).on_hover_text("Font size");
        egui::Popup::menu(&size_btn).show(|ui| {
            for s in deckcraft_engine::cmd::format::SIZES.iter().chain([72.0, 80.0, 88.0, 96.0].iter()) {
                if ui.button(widgets_num(*s)).clicked() {
                    touch_show(show, selection, Style::Point(*s));
                    ui.close();
                }
            }
        });
        if bar_button(ui, "+", false).on_hover_text("Larger text on this slide").clicked() {
            touch_show(show, selection, Style::Size(SIZE_STEP));
        }
        ui.add_space(8.0);
        for (name, rgb) in swatches() {
            let on = current_rgb(&props) == rgb;
            let chip = rgb.map(|c| Color32::from_rgb(c.r, c.g, c.b)).unwrap_or(Color32::from_gray(230));
            if swatch(ui, name, chip, on).clicked() {
                touch_show(show, selection, Style::Color(rgb));
            }
        }
        ui.label(egui::RichText::new("This slide only").size(12.0).color(Color32::from_gray(140)));
    });
    close
}

fn edit_body(ui: &mut Ui, show: &mut Show, id: egui::Id) {
    let focus = show.notes_focus;
    show.notes_focus = false;
    let px = show.notes_px;
    style_editor(ui);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let Some((_, draft)) = show.notes_draft.as_mut() else { return };
        let out = crate::notes::editor(ui, draft, &Look { size: px, color: Some(Color32::from_gray(230)) }, |edit| {
            edit.id(id)
                .hint_text("Type notes for this slide")
                .desired_width(f32::INFINITY)
                .frame(egui::Frame::new().fill(Color32::from_gray(32)).inner_margin(egui::Margin::same(8)))
        });
        if focus {
            out.response.request_focus();
        }
    });
}

fn style_editor(ui: &mut Ui) {
    let visuals = &mut ui.style_mut().visuals;
    visuals.override_text_color = Some(Color32::from_gray(230));
    visuals.weak_text_color = Some(Color32::from_gray(150));
    visuals.text_cursor.stroke.color = Color32::WHITE;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(70, 130, 210, 160);
}

fn restyle_font_show(show: &mut Show, selection: Option<(usize, usize)>, family: &str) {
    if let Some((_, draft)) = show.notes_draft.as_mut() {
        let before = draft.body.clone();
        restyle_font(&mut draft.body, selection, family);
        if draft.body != before {
            draft.edited = true;
        }
    }
}

fn note_font_names(doc: &Presentation) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(m) = doc.masters.first() {
        names.push(m.theme.fonts.major.latin.clone());
        names.push(m.theme.fonts.minor.latin.clone());
    }
    names.extend(deckcraft_fonts::FontDb::global().families());
    names.dedup();
    names
}

fn short_label(text: &str) -> String {
    let mut out: String = text.chars().take(18).collect();
    if text.chars().count() > 18 {
        out.push('…');
    }
    out
}

fn widgets_num(n: f64) -> String {
    if (n - n.round()).abs() < 0.01 { format!("{}", n.round() as i32) } else { format!("{n}") }
}

fn touch_show(show: &mut Show, selection: Option<(usize, usize)>, style: Style) {
    if let Some((_, draft)) = show.notes_draft.as_mut() {
        touch(draft, selection, style);
    }
}

fn touch(draft: &mut Draft, selection: Option<(usize, usize)>, style: Style) {
    let before = draft.body.clone();
    restyle(&mut draft.body, selection, style);
    if draft.body != before {
        draft.edited = true;
    }
}

fn command_style(ui: &Ui) -> Option<(Style, egui::Modifiers, egui::Key)> {
    ui.input(|i| {
        if !i.modifiers.command || i.modifiers.shift || i.modifiers.alt {
            return None;
        }
        let key = [(egui::Key::B, Style::Bold), (egui::Key::I, Style::Italic), (egui::Key::U, Style::Underline)]
            .into_iter()
            .find(|(key, _)| i.key_pressed(*key))?;
        Some((key.1, i.modifiers, key.0))
    })
}

fn text_selection(ui: &Ui, id: egui::Id) -> Option<(usize, usize)> {
    let state = egui::text_edit::TextEditState::load(ui.ctx(), id)?;
    let range = state.cursor.char_range()?;
    let a = range.primary.index.0;
    let b = range.secondary.index.0;
    (a != b).then_some((a.min(b), b.max(a)))
}

fn editor_id(slide: usize) -> egui::Id {
    egui::Id::unique(("deckcraft-presenter-notes", slide))
}

fn bar_button(ui: &mut Ui, label: &str, on: bool) -> egui::Response {
    let fill = if on { Color32::from_rgb(56, 104, 168) } else { Color32::from_gray(58) };
    ui.add(egui::Button::new(egui::RichText::new(label).size(14.0).color(Color32::WHITE)).fill(fill).min_size(vec2(0.0, 28.0)))
}

fn swatch(ui: &mut Ui, name: &str, color: Color32, on: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::click());
    let painter = ui.painter();
    painter.circle_filled(rect.center(), 7.0, color);
    let ring = if on || response.hovered() { Color32::WHITE } else { Color32::from_gray(90) };
    painter.circle_stroke(rect.center(), 8.0, egui::Stroke::new(if on { 2.0 } else { 1.0 }, ring));
    response.on_hover_text(name)
}

fn swatches() -> [(&'static str, Option<Rgba>); 7] {
    [
        ("Default", None),
        ("White", Some(Rgba::rgb(255, 255, 255))),
        ("Gold", Some(Rgba::rgb(255, 214, 90))),
        ("Orange", Some(Rgba::rgb(255, 149, 64))),
        ("Green", Some(Rgba::rgb(126, 214, 146))),
        ("Sky", Some(Rgba::rgb(126, 196, 255))),
        ("Rose", Some(Rgba::rgb(255, 154, 176))),
    ]
}

fn anchor_props(body: &TextBody, selection: Option<(usize, usize)>) -> RunProps {
    let at = selection.map(|(a, _)| char_pos(body, a)).unwrap_or((0, 0));
    edit::props_at(body, at)
}

fn point_size(props: &RunProps) -> f64 {
    props.size.filter(|s| s.is_finite() && *s > 0.0).unwrap_or(12.0)
}

fn underline_on(props: &RunProps) -> bool {
    props.underline.as_deref().is_some_and(|u| u != "none")
}

fn current_rgb(props: &RunProps) -> Option<Rgba> {
    let Fill::Solid { color } = props.fill.as_ref()? else { return None };
    let deckcraft_model::style::ColorBase::Rgb { rgb } = &color.base else { return None };
    Some(*rgb)
}

/// A character index in the notes' plain text, as the editor's caret counts it.
fn char_pos(body: &TextBody, index: usize) -> edit::Pos {
    let mut left = index;
    for (pi, paragraph) in body.paragraphs.iter().enumerate() {
        let n = paragraph.char_len();
        if left <= n {
            return (pi, left);
        }
        left = left.saturating_sub(n + 1);
    }
    let pi = body.paragraphs.len().saturating_sub(1);
    (pi, body.paragraphs.get(pi).map(Paragraph::char_len).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatting_one_slide_leaves_the_other_alone() {
        let mut busy = TextBody::from_text("A long note that needs to be smaller");
        let other = TextBody::from_text("Short");
        restyle(&mut busy, None, Style::Size(-4.0));
        restyle(&mut busy, None, Style::Color(Some(Rgba::rgb(255, 214, 90))));
        let sized = busy.paragraphs.first().and_then(|p| p.runs.first()).and_then(|r| r.props.size);
        let colored = busy.paragraphs.first().and_then(|p| p.runs.first()).is_some_and(|r| r.props.fill.is_some());
        let other_sized = other.paragraphs.first().and_then(|p| p.runs.first()).and_then(|r| r.props.size);
        let other_colored = other.paragraphs.first().and_then(|p| p.runs.first()).is_some_and(|r| r.props.fill.is_some());
        assert_eq!(sized, Some(8.0));
        assert!(colored);
        assert_eq!(other_sized, None);
        assert!(!other_colored);
    }

    #[test]
    fn a_selection_changes_only_the_selected_words() {
        let mut body = TextBody::from_text("hello");
        restyle(&mut body, Some((0, 2)), Style::Bold);
        let runs: Vec<(String, Option<bool>)> =
            body.paragraphs.first().map(|p| p.runs.iter().map(|r| (r.text.clone(), r.props.bold)).collect()).unwrap_or_default();
        assert_eq!(runs, vec![("he".into(), Some(true)), ("llo".into(), None)]);
    }

    #[test]
    fn a_solid_fill_is_the_colour_the_notes_draw() {
        let mut body = TextBody::from_text("gold");
        restyle(&mut body, None, Style::Color(Some(Rgba::rgb(255, 214, 90))));
        let props = body.paragraphs.first().and_then(|p| p.runs.first()).map(|r| r.props.clone()).unwrap_or_default();
        assert_eq!(crate::notes::run_color(&props, Color32::from_gray(230)), Color32::from_rgb(255, 214, 90));
        assert_eq!(crate::notes::run_color(&RunProps::default(), Color32::from_gray(230)), Color32::from_gray(230));
    }

    #[test]
    fn a_font_and_a_point_size_change_only_the_selected_words() {
        let mut body = TextBody::from_text("hello");
        restyle_font(&mut body, Some((0, 2)), "Liberation Serif");
        restyle(&mut body, Some((0, 2)), Style::Point(28.0));
        let runs: Vec<(String, Option<String>, Option<f64>)> = body
            .paragraphs
            .first()
            .map(|p| p.runs.iter().map(|r| (r.text.clone(), r.props.font.clone(), r.props.size)).collect())
            .unwrap_or_default();
        assert_eq!(runs, vec![("he".into(), Some("Liberation Serif".into()), Some(28.0)), ("llo".into(), None, None)]);
    }
}
