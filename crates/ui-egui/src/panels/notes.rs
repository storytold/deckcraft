//! Speaker notes editing (the notes pane and Notes Page view). The notes are edited as text and
//! drawn with their formatting: font, size, bold, italic, underline, strikethrough, highlight and
//! baseline from the runs, indents (wrapped lines included) and bullets or numbers from the
//! paragraphs. Each edit is applied to a formatted draft, so typed text takes the formatting where
//! it lands and the rest keeps its own; the draft is saved with `slide.notes {body}` once the
//! editor loses focus. While the caret is in the notes, Home ▸ Font and the other text commands
//! format that caret the same way they format text on a slide.

use std::sync::Arc;

use deckcraft_model::edit;
use deckcraft_model::style::{ColorBase, ColorRef, Fill};
use deckcraft_model::text::{Bullet, Paragraph, RunProps, Strike, TextBody};
use egui::text::{CCursor, Galley, LayoutJob, TextFormat};
use egui::{Align2, Color32, Stroke, Ui, vec2};

use crate::theme;

/// Size and colour of a notes editor's text.
pub struct Look {
    /// Pixels for 12 pt text, the notes' default size; indents scale with it.
    pub size: f32,
    /// `None` = the widget text colour.
    pub color: Option<Color32>,
}

/// Notes being edited: the text in the editor and the formatted notes it stands for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub text: String,
    pub body: TextBody,
    /// Edited since it was saved.
    pub edited: bool,
}

impl Draft {
    pub fn new(body: &TextBody) -> Self {
        Draft { text: body.text(), body: body.clone(), edited: false }
    }
}

/// The draft as it reads with `text` typed in (one edit since the draft): `None` when that can't
/// be mapped onto it.
fn typed(body: &TextBody, text: &str) -> Option<TextBody> {
    let mut b = body.clone();
    edit::replace_text(&mut b, text);
    (b.text() == text).then_some(b)
}

/// A length in points from the file as pixels, bounded.
fn px(pt: f64, k: f32) -> f32 {
    if pt.is_finite() { pt.clamp(0.0, 400.0) as f32 * k } else { 0.0 }
}

/// Where a paragraph's first line starts and its other lines start (points): a level without its
/// own margin is indented 36 pt per level, as in the default notes master.
fn indents(p: &Paragraph) -> (f64, f64) {
    let margin = p.props.margin_left.unwrap_or(36.0 * f64::from(p.level.min(8)));
    let first = margin + p.props.indent.unwrap_or(0.0);
    (first, margin)
}

/// Each paragraph's bullet or number, numbered as on a slide (per level, restarting after an
/// unnumbered paragraph at the same or a lower level).
fn bullets(body: &TextBody) -> Vec<Option<String>> {
    let mut numbering = [0u32; 9];
    body.paragraphs
        .iter()
        .map(|p| {
            let lvl = usize::from(p.level.min(8));
            if p.is_empty() {
                return None;
            }
            let auto = matches!(p.props.bullet, Some(Bullet::AutoNum { .. }));
            for (i, n) in numbering.iter_mut().enumerate().skip(lvl) {
                if i > lvl || !auto {
                    *n = 0;
                }
            }
            match p.props.bullet.as_ref()? {
                Bullet::None => None,
                Bullet::Char { char } => Some(deckcraft_text::map_symbol_bullet(char, p.props.bullet_font.as_deref())),
                Bullet::AutoNum { scheme, start_at } => {
                    let n = numbering.get_mut(lvl)?;
                    *n = n.saturating_add(1);
                    Some(deckcraft_text::autonum(scheme, start_at.saturating_sub(1).saturating_add(*n)))
                }
                Bullet::Picture { .. } => Some("•".into()),
            }
        })
        .collect()
}

/// A run's solid colour, or `fallback` when the run has no plain RGB fill.
pub(crate) fn run_color(props: &RunProps, fallback: Color32) -> Color32 {
    let Some(Fill::Solid { color }) = props.fill.as_ref() else { return fallback };
    let ColorBase::Rgb { rgb } = &color.base else { return fallback };
    let a = (color.alpha().clamp(0.0, 1.0) * f64::from(u8::MAX)).round();
    let a = if a.is_finite() { a as u8 } else { u8::MAX };
    Color32::from_rgba_unmultiplied(rgb.r, rgb.g, rgb.b, a)
}

fn format(props: &RunProps, size: f32, fallback: Color32, ready: &[String]) -> TextFormat {
    // A slide's own point size has to stay visible: 8 pt through 96 pt, on top of the reading zoom.
    let mut size = match props.size {
        Some(pt) if pt.is_finite() && pt > 0.0 => (size * pt as f32 / 12.0).clamp(size * 0.5, size * 8.0),
        _ => size,
    };
    // Superscript and subscript sit above or below the line and draw smaller, as on a slide.
    let raised = props.baseline.filter(|b| b.is_finite() && b.abs() > 0.01);
    if raised.is_some() {
        size = (size * 0.65).max(6.0);
    }
    let color = run_color(props, fallback);
    let font = note_font(props, size, ready);
    let mut f = TextFormat::simple(font, color);
    f.italics = props.italic == Some(true);
    if let Some(b) = raised {
        f.valign = if b > 0.0 { egui::Align::TOP } else { egui::Align::BOTTOM };
    }
    f.background = highlight_bg(props);
    if props.underline.as_deref().is_some_and(|u| u != "none") {
        f.underline = Stroke::new(1.0, color);
    }
    if matches!(props.strike, Some(Strike::Single | Strike::Double)) {
        f.strikethrough = Stroke::new(1.0, color);
    }
    f
}

/// The egui family key for a notes run's font, once that face has been installed.
pub(crate) fn note_font_key(family: &str, bold: bool) -> String {
    if bold { format!("notes:{family}:bold") } else { format!("notes:{family}") }
}

fn note_font(props: &RunProps, size: f32, ready: &[String]) -> egui::FontId {
    let bold = props.bold == Some(true);
    if let Some(family) = props.font.as_deref().filter(|f| !f.is_empty() && !f.starts_with('+')) {
        let key = note_font_key(family, bold);
        if ready.iter().any(|s| s == &key) {
            return egui::FontId::new(size, egui::FontFamily::Name(key.into()));
        }
    }
    if bold { theme::bold(size) } else { theme::font(size) }
}

fn highlight_bg(props: &RunProps) -> Color32 {
    let Some(ColorRef { base: ColorBase::Rgb { rgb }, .. }) = props.highlight.as_ref() else {
        return Color32::TRANSPARENT;
    };
    Color32::from_rgb(rgb.r, rgb.g, rgb.b)
}

/// Explicit font families in `body` (theme references such as `+mn-lt` wait until a command
/// writes a real family). At most eight, which is all the notes font installer will load.
pub fn used_families(body: &TextBody) -> Vec<String> {
    let mut out = Vec::new();
    let mut push = |font: &Option<String>| {
        let Some(family) = font.as_deref().filter(|f| !f.is_empty() && !f.starts_with('+') && f.len() <= 200) else { return };
        if out.len() < 8 && !out.iter().any(|e| e == family) {
            out.push(family.to_string());
        }
    };
    for p in &body.paragraphs {
        push(&p.end_props.font);
        for r in &p.runs {
            push(&r.props.font);
        }
    }
    out
}

/// Ask the next frame's font install to draw these notes in their own families.
pub fn request_fonts(ui: &Ui, body: &TextBody) {
    let families = used_families(body);
    ui.data_mut(|d| d.insert_temp(egui::Id::unique(crate::uifonts::NOTE_FONT_REQUEST), families));
}

fn ready_fonts(ui: &Ui) -> Vec<String> {
    ui.data(|d| d.get_temp(egui::Id::unique(crate::uifonts::NOTE_FONT_READY))).unwrap_or_default()
}

/// One paragraph to lay out: wrapped lines start at the margin (pixels), the first line's text at
/// `text`, clear of a bullet.
struct ParaLayout {
    job: LayoutJob,
    margin: f32,
    /// x of the first line's text.
    text: f32,
}

fn para_layout(p: &Paragraph, bullet: Option<&str>, size: f32, color: Color32, wrap: f32, ready: &[String]) -> ParaLayout {
    let k = size / 12.0;
    let (first, margin) = indents(p);
    let (first, margin) = (px(first, k), px(margin, k));
    let text = match bullet {
        // The text starts at the margin, after the bullet if it's wider than the hanging indent.
        Some(b) => margin.max(first + size * (0.4 + 0.6 * b.chars().count() as f32)),
        None => first,
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = (wrap - margin).max(size * 4.0);
    let mut lead = (text - margin).max(0.0);
    for r in &p.runs {
        let t = r.content();
        if !t.is_empty() {
            job.append(t, lead, format(&r.props, size, color, ready));
            lead = 0.0;
        }
    }
    if job.text.is_empty() {
        job.append("", 0.0, format(&p.end_props, size, color, ready));
    }
    ParaLayout { job, margin, text }
}

/// The whole notes as one job (the galley's text and formats).
fn job(body: &TextBody, size: f32, color: Color32, wrap: f32, ready: &[String]) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    for (i, (p, bullet)) in body.paragraphs.iter().zip(bullets(body)).enumerate() {
        if i > 0 {
            job.append("\n", 0.0, TextFormat::simple(theme::font(size), color));
        }
        let pl = para_layout(p, bullet.as_deref(), size, color, wrap, ready);
        for s in pl.job.sections {
            if let Some(t) = pl.job.text.get(s.byte_range.start.0..s.byte_range.end.0) {
                job.append(t, s.leading_space, s.format);
            }
        }
    }
    job
}

/// Lay the notes out paragraph by paragraph, each moved to its indent, so wrapped lines line up
/// under the text and not under the bullet.
fn galley(ui: &Ui, body: &TextBody, size: f32, color: Color32, wrap: f32, ready: &[String]) -> Arc<Galley> {
    let whole = job(body, size, color, wrap, ready);
    if body.paragraphs.is_empty() {
        return ui.fonts_mut(|f| f.layout_job(whole));
    }
    let parts: Vec<Arc<Galley>> = ui.fonts_mut(|f| {
        body.paragraphs
            .iter()
            .zip(bullets(body))
            .map(|(p, bullet)| {
                let pl = para_layout(p, bullet.as_deref(), size, color, wrap, ready);
                let mut g = (*f.layout_job(pl.job)).clone();
                for (i, row) in g.rows.iter_mut().enumerate() {
                    // The first line already holds the gap after a bullet; a first line hanging
                    // left of the margin without one starts there.
                    row.pos.x += if i == 0 { pl.margin.min(pl.text) } else { pl.margin };
                }
                Arc::new(g)
            })
            .collect()
    });
    Arc::new(Galley::concat(Arc::new(whole), &parts, ui.ctx().pixels_per_point()))
}

/// A multiline editor for the draft that draws its formatting; `configure` sets the id, hint,
/// size and frame. Edits update the draft's text and formatted body. Returns the field's output
/// (galley and caret too, for spelling marks).
pub fn editor(
    ui: &mut Ui,
    draft: &mut Draft,
    look: &Look,
    configure: impl for<'a> FnOnce(egui::TextEdit<'a>) -> egui::TextEdit<'a>,
) -> egui::text_edit::TextEditOutput {
    request_fonts(ui, &draft.body);
    let ready = ready_fonts(ui);
    let color = look.color.unwrap_or_else(|| ui.visuals().override_text_color.unwrap_or_else(|| ui.visuals().widgets.inactive.text_color()));
    let size = look.size;
    let before = draft.body.clone();
    let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| match typed(&before, text.as_str()) {
        Some(b) => galley(ui, &b, size, color, wrap, &ready),
        None => ui.fonts_mut(|f| f.layout_job(LayoutJob::simple(text.as_str().to_string(), theme::font(size), color, wrap))),
    };
    let out = configure(egui::TextEdit::multiline(&mut draft.text)).layouter(&mut layouter).show(ui);
    if draft.body.text() != draft.text {
        edit::replace_text(&mut draft.body, &draft.text);
        draft.edited = true;
    }
    if draft.body.text() == draft.text {
        paint_bullets(&ui.painter().with_clip_rect(out.text_clip_rect), &draft.body, &out.galley, out.galley_pos, size, color);
    }
    out
}

/// Bullets and numbers at the start of each paragraph's first line of `galley` (laid out from
/// `body`), drawn at `origin`.
fn paint_bullets(painter: &egui::Painter, body: &TextBody, galley: &Galley, origin: egui::Pos2, size: f32, color: Color32) {
    let k = size / 12.0;
    let mut at = 0usize;
    for (p, bullet) in body.paragraphs.iter().zip(bullets(body)) {
        if let Some(b) = bullet {
            let row = galley.pos_from_cursor(CCursor::new(at));
            let pos = origin + vec2(px(indents(p).0, k), row.center().y);
            let font = theme::font(row.height().clamp(size * 0.75, size * 2.0) * 0.8);
            let ink = p.runs.first().map(|r| run_color(&r.props, color)).unwrap_or(color);
            painter.text(pos, Align2::LEFT_CENTER, b, font, ink);
        }
        at += p.char_len() + 1;
    }
}

/// The notes drawn read-only with their formatting, wrapped to the available width, and
/// selectable like a label (the presenter's notes pane).
pub fn view(ui: &mut Ui, body: &TextBody, look: &Look) -> egui::Response {
    request_fonts(ui, body);
    let color = look.color.unwrap_or_else(|| ui.visuals().text_color());
    let g = galley(ui, body, look.size, color, ui.available_width(), &ready_fonts(ui));
    let response = ui.add(egui::Label::new(egui::WidgetText::Galley(g.clone())));
    paint_bullets(ui.painter(), body, &g, response.rect.min, look.size, color);
    response
}

/// A character index in the notes' plain text, as the editor's caret counts it (a paragraph
/// break is one character).
pub(crate) fn char_pos(body: &TextBody, index: usize) -> (usize, usize) {
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

/// Write an edited notes draft into the presentation. One undo step. No-op when nothing is pending.
pub fn commit_draft(app: &mut crate::SlideApp) {
    if !app.notes_buf.2.edited {
        return;
    }
    let uid = app.notes_buf.0;
    let index = app.notes_buf.1;
    if app.session.active().is_none_or(|st| st.uid != uid) {
        app.notes_buf.2.edited = false;
        return;
    }
    let body = app.notes_buf.2.body.clone();
    app.notes_buf.2.edited = false;
    if let Err(e) = app.session.execute("slide.notes", &serde_json::json!({"body": body, "index": index})) {
        app.notes_buf.2.edited = true;
        app.set_status(e.to_string());
    }
}

/// The draft for this document and slide. An edit that belongs to another slide, or a field that
/// no longer has focus, is saved first. A formatting change from a text command (the draft was
/// not edited, the body differs) replaces the draft so the pane shows it.
pub fn prepare_draft(app: &mut crate::SlideApp, uid: u64, index: usize, session_notes: &TextBody, focused: bool) -> Draft {
    let pending = app.notes_buf.2.edited && (!focused || app.notes_buf.0 != uid || app.notes_buf.1 != index);
    let pending_index = app.notes_buf.1;
    let pending_doc = app.notes_buf.0 == uid;
    if pending {
        commit_draft(app);
    }
    // The caller's snapshot is from before that save. Keep what was just written on this slide
    // instead of reloading the copy from before the save.
    let notes = if pending && pending_doc && pending_index == index { app.notes_buf.2.body.clone() } else { session_notes.clone() };
    if app.notes_buf.0 != uid || app.notes_buf.1 != index || (!app.notes_buf.2.edited && app.notes_buf.2.body != notes) {
        app.notes_buf = (uid, index, Draft::new(&notes));
    }
    app.notes_buf.2.clone()
}

/// Point the text commands at these notes. Unchanged carets do not touch the selection, so a
/// frame that only redraws does not bump the document revision.
pub fn publish_selection(app: &mut crate::SlideApp, index: usize, body: &TextBody, anchor: usize, caret: usize) {
    let a = char_pos(body, anchor);
    let c = char_pos(body, caret);
    let Some(st) = app.session.active() else { return };
    if st.selection.slide != index {
        return;
    }
    if let Some(t) = &st.selection.text
        && t.notes
        && t.anchor == a
        && t.caret == c
        && st.selection.shapes.is_empty()
    {
        return;
    }
    let _ = app.session.select(|_, sel| {
        if sel.slide != index {
            return;
        }
        sel.shapes.clear();
        sel.cells = None;
        sel.text = Some(deckcraft_engine::TextSel { notes: true, anchor: a, caret: c, ..Default::default() });
    });
}

/// Leave notes editing so the next text command hits a shape again.
pub fn release_notes(app: &mut crate::SlideApp) {
    let editing = app.session.active().is_some_and(|st| st.selection.text.as_ref().is_some_and(|t| t.notes));
    if editing {
        let _ = app.session.execute("text.exit", &serde_json::json!({}));
    }
}

/// Remember whether the notes field has the keyboard, and keep the engine caret on it.
/// `anchor` and `caret` are character indexes in the notes' plain text.
pub fn sync_focus(app: &mut crate::SlideApp, ui: &Ui, index: usize, body: &TextBody, focused: bool, anchor: Option<usize>, caret: Option<usize>) {
    app.notes_touched = true;
    app.notes_focused = focused;
    if focused {
        if let (Some(a), Some(c)) = (anchor, caret) {
            publish_selection(app, index, body, a, c);
        }
    } else if !ui.ctx().any_popup_open() {
        release_notes(app);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use deckcraft_model::text::{ParaProps, Run};

    use super::*;

    /// Notes as PowerPoint users write them: a bold heading, a plain line, points with a
    /// Wingdings square bullet and a hanging indent (one long enough to wrap), numbered points,
    /// a point with an explicit buNone, closing plain lines; Slovak text throughout.
    fn notes() -> TextBody {
        let props = |bullet: Bullet| ParaProps {
            margin_left: Some(22.5),
            indent: Some(-22.5),
            bullet: Some(bullet),
            bullet_font: Some("Wingdings".into()),
            ..Default::default()
        };
        let point = |runs: Vec<Run>, bullet: Bullet| Paragraph { level: 1, props: props(bullet), runs, ..Default::default() };
        let square = || Bullet::Char { char: "§".into() };
        let num = || Bullet::AutoNum { scheme: "arabicPeriod".into(), start_at: 1 };
        let bold = RunProps { bold: Some(true), ..Default::default() };
        TextBody {
            paragraphs: vec![
                Paragraph { runs: vec![Run::with("Metodický prehľad", bold.clone())], ..Default::default() },
                Paragraph::new("Žiaci pracujú vo dvojiciach."),
                point(vec![Run::with("Cieľ: ", bold), Run::new("vysvetliť cyklus")], square()),
                point(vec![Run::new("Dlhý bod, ktorý sa zalomí na viac riadkov, lebo je naozaj veľmi dlhý a pokračuje ďalej.")], square()),
                point(vec![Run::new("Jeden")], num()),
                point(vec![Run::new("Dva")], num()),
                point(vec![Run::new("Bez odrážky")], Bullet::None),
                Paragraph::new("Pomôcky: počítač"),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn bullets_follow_the_paragraphs() {
        let b = bullets(&notes());
        assert_eq!(b, vec![None, None, Some("▪".into()), Some("▪".into()), Some("1.".into()), Some("2.".into()), None, None]);
    }

    #[test]
    fn notes_show_bold_runs_indents_and_wrap_under_the_text() {
        let body = notes();
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let mut laid = None;
        let mut out = ctx.run_ui(Default::default(), |ui| laid = Some(galley(ui, &body, 12.0, Color32::BLACK, 300.0, &[])));
        out.textures_delta.clear();
        let g = laid.unwrap();
        assert_eq!(g.job.text, body.text(), "the galley reads the notes");
        assert_eq!(g.rows.iter().map(|r| r.char_count_including_newline().0).sum::<usize>(), body.text().chars().count());
        // Bold where the runs are bold, regular elsewhere.
        let font_of = |s: &str| {
            let b = g.job.text.find(s).unwrap();
            g.job.sections.iter().find(|x| (x.byte_range.start.0..x.byte_range.end.0).contains(&b)).unwrap().format.font_id.clone()
        };
        assert_eq!(font_of("Metodický"), theme::bold(12.0));
        assert_eq!(font_of("Cieľ"), theme::bold(12.0));
        assert_eq!(font_of("vysvetliť"), theme::font(12.0));
        assert_eq!(font_of("Pomôcky"), theme::font(12.0));
        // Rows: x where each paragraph's text starts.
        let row_x = |s: &str| {
            let at = body.text().find(s).map(|b| body.text()[..b].chars().count()).unwrap();
            g.pos_from_cursor(CCursor::new(at)).min.x
        };
        assert!(row_x("Metodický") < 1.0 && row_x("Žiaci") < 1.0 && row_x("Pomôcky") < 1.0, "plain paragraphs aren't indented");
        assert!((row_x("Cieľ") - 22.5).abs() < 1.0, "bulleted text at the 22.5 pt margin: {}", row_x("Cieľ"));
        assert!((row_x("Bez odrážky") - 0.0).abs() < 1.0, "buNone: no bullet, the first line hangs to 0");
        // The long point wraps, and its wrapped lines start at the margin, not under the bullet.
        let long = body.text().find("Dlhý").map(|b| body.text()[..b].chars().count()).unwrap();
        let first_row = g.pos_from_cursor(CCursor::new(long));
        let wrapped: Vec<_> =
            g.rows.iter().filter(|r| r.pos.y > first_row.min.y + 1.0 && r.pos.y < g.pos_from_cursor(CCursor::new(long + 80)).max.y).collect();
        assert!(!wrapped.is_empty(), "the long point wraps");
        assert!(wrapped.iter().all(|r| (r.pos.x - 22.5).abs() < 1.0), "wrapped lines align with the text");
    }

    #[test]
    fn typing_keeps_the_formatting_around_it() {
        let mut d = Draft::new(&notes());
        // Typing into the bold heading continues it in bold.
        let t = d.text.replacen("prehľad", "prehľad hodiny", 1);
        d.body = typed(&d.body, &t).unwrap();
        assert_eq!(d.body.paragraphs[0].runs.len(), 1);
        assert_eq!(d.body.paragraphs[0].runs[0].props.bold, Some(true));
        // Enter at the end of a bulleted point starts another one with the same bullet and indent.
        let t = d.body.text().replacen("cyklus", "cyklus\nnový bod", 1);
        d.body = typed(&d.body, &t).unwrap();
        let new = &d.body.paragraphs[3];
        assert_eq!(new.text(), "nový bod");
        assert_eq!((new.level, &new.props), (1, &notes().paragraphs[2].props));
        assert_eq!(d.body.paragraphs[2].runs[0].props.bold, Some(true), "the bold label stays bold");
        // Hostile sizes and indents stay bounded.
        let mut wild = notes();
        wild.paragraphs[2].props.margin_left = Some(f64::NAN);
        wild.paragraphs[0].runs[0].props.size = Some(f64::INFINITY);
        let j = job(&wild, 12.0, Color32::BLACK, 300.0, &[]);
        assert!(j.sections.iter().all(|s| s.leading_space.is_finite() && s.format.font_id.size <= 24.0));
    }

    #[test]
    fn a_notes_run_draws_in_its_own_family_and_point_size() {
        let mut body = TextBody::from_text("Serif");
        let run = body.paragraphs.first_mut().and_then(|p| p.runs.first_mut()).unwrap();
        run.props.font = Some("Liberation Serif".into());
        run.props.size = Some(24.0);
        let ctx = egui::Context::default();
        theme::install_fonts(&ctx);
        let ready = vec![note_font_key("Liberation Serif", false)];
        let mut laid = None;
        let mut out = ctx.run_ui(Default::default(), |ui| laid = Some(galley(ui, &body, 12.0, Color32::BLACK, 300.0, &ready)));
        out.textures_delta.clear();
        let g = laid.unwrap();
        let font = g.job.sections.first().unwrap().format.font_id.clone();
        assert_eq!(font.family, egui::FontFamily::Name(note_font_key("Liberation Serif", false).into()));
        assert!((font.size - 24.0).abs() < 0.1, "{}", font.size);
    }
}
