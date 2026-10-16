//! Speaker notes editing (the notes pane and Notes Page view). The notes are edited as text and
//! drawn with their formatting: bold, italic, underline, strikethrough and size from the runs,
//! indents (wrapped lines included) and bullets or numbers from the paragraphs. Each edit is
//! applied to a formatted draft, so typed text takes the formatting where it lands and the rest
//! keeps its own; the draft is saved with `slide.notes {body}` once the editor loses focus.

use std::sync::Arc;

use deckcraft_model::edit;
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

fn format(props: &RunProps, size: f32, color: Color32) -> TextFormat {
    let size = match props.size {
        Some(pt) if pt.is_finite() && pt > 0.0 => (size * pt as f32 / 12.0).clamp(size * 0.75, size * 2.0),
        _ => size,
    };
    let font = if props.bold == Some(true) { theme::bold(size) } else { theme::font(size) };
    let mut f = TextFormat::simple(font, color);
    f.italics = props.italic == Some(true);
    if props.underline.as_deref().is_some_and(|u| u != "none") {
        f.underline = Stroke::new(1.0, color);
    }
    if matches!(props.strike, Some(Strike::Single | Strike::Double)) {
        f.strikethrough = Stroke::new(1.0, color);
    }
    f
}

/// One paragraph to lay out: wrapped lines start at the margin (pixels), the first line's text at
/// `text`, clear of a bullet.
struct ParaLayout {
    job: LayoutJob,
    margin: f32,
    /// x of the first line's text.
    text: f32,
}

fn para_layout(p: &Paragraph, bullet: Option<&str>, size: f32, color: Color32, wrap: f32) -> ParaLayout {
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
            job.append(t, lead, format(&r.props, size, color));
            lead = 0.0;
        }
    }
    if job.text.is_empty() {
        job.append("", 0.0, format(&p.end_props, size, color));
    }
    ParaLayout { job, margin, text }
}

/// The whole notes as one job (the galley's text and formats).
fn job(body: &TextBody, size: f32, color: Color32, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    for (i, (p, bullet)) in body.paragraphs.iter().zip(bullets(body)).enumerate() {
        if i > 0 {
            job.append("\n", 0.0, TextFormat::simple(theme::font(size), color));
        }
        let pl = para_layout(p, bullet.as_deref(), size, color, wrap);
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
fn galley(ui: &Ui, body: &TextBody, size: f32, color: Color32, wrap: f32) -> Arc<Galley> {
    let whole = job(body, size, color, wrap);
    if body.paragraphs.is_empty() {
        return ui.fonts_mut(|f| f.layout_job(whole));
    }
    let parts: Vec<Arc<Galley>> = ui.fonts_mut(|f| {
        body.paragraphs
            .iter()
            .zip(bullets(body))
            .map(|(p, bullet)| {
                let pl = para_layout(p, bullet.as_deref(), size, color, wrap);
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
    let color = look.color.unwrap_or_else(|| ui.visuals().override_text_color.unwrap_or_else(|| ui.visuals().widgets.inactive.text_color()));
    let size = look.size;
    let before = draft.body.clone();
    let mut layouter = |ui: &Ui, text: &dyn egui::TextBuffer, wrap: f32| match typed(&before, text.as_str()) {
        Some(b) => galley(ui, &b, size, color, wrap),
        None => ui.fonts_mut(|f| f.layout_job(LayoutJob::simple(text.as_str().to_string(), theme::font(size), color, wrap))),
    };
    let out = configure(egui::TextEdit::multiline(&mut draft.text)).layouter(&mut layouter).show(ui);
    if draft.body.text() != draft.text {
        edit::replace_text(&mut draft.body, &draft.text);
        draft.edited = true;
    }
    // Bullets and numbers sit at the start of each paragraph's first line.
    if draft.body.text() == draft.text {
        let painter = ui.painter().with_clip_rect(out.text_clip_rect);
        let k = size / 12.0;
        let mut at = 0usize;
        for (p, bullet) in draft.body.paragraphs.iter().zip(bullets(&draft.body)) {
            if let Some(b) = bullet {
                let row = out.galley.pos_from_cursor(CCursor::new(at));
                let pos = out.galley_pos + vec2(px(indents(p).0, k), row.center().y);
                let font = theme::font(row.height().clamp(size * 0.75, size * 2.0) * 0.8);
                painter.text(pos, Align2::LEFT_CENTER, b, font, color);
            }
            at += p.char_len() + 1;
        }
    }
    out
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
    fn korean_notes_round_trip_and_draw_without_installed_fonts() {
        let text = "한국어 발표자 노트입니다\n한글과 UTF-8 문자를 보존합니다";
        let mut session = deckcraft_engine::Session::with_new();
        session.execute("slide.notes", &serde_json::json!({"text": text})).unwrap();
        let bytes = deckcraft_engine::cmd::file::save_bytes(&session.doc().unwrap().doc, "pptx").unwrap();
        let imported = deckcraft_engine::cmd::file::open_presentation("korean-notes.pptx", &bytes).unwrap();
        assert_eq!(imported.slides[0].notes_text(), text, "PPTX preserves Korean UTF-8 notes");

        if deckcraft_fonts::CRAFT_FONTS.is_empty() {
            return;
        }
        let db = deckcraft_fonts::FontDb::with_font_dirs(vec![]);
        db.set_system_fallback(false);
        let faces = crate::uifonts::faces_for_text(text, &[], &mut std::collections::HashSet::new(), |_| false, |c| db.fallback_for(c, u32::MAX));
        for c in text.chars().filter(|c| !c.is_ascii() && !c.is_whitespace()) {
            assert!(faces.iter().any(|f| f.covers(c)), "embedded fonts lack {c:?}");
        }
        let ctx = egui::Context::default();
        theme::install_fonts_with(&ctx, &faces);
        let glyphs: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let mut laid = None;
        let mut output = ctx.run_ui(Default::default(), |ui| {
            for font in [theme::font(12.0), theme::bold(12.0)] {
                assert!(ui.fonts_mut(|f| f.has_glyphs(&font, &glyphs)), "notes require real Korean glyphs");
            }
            laid = Some(galley(ui, &imported.slides[0].notes, 12.0, Color32::BLACK, 300.0));
        });
        output.textures_delta.clear();
        let laid = laid.unwrap();
        assert_eq!(laid.job.text, text);
        assert!(
            laid.rows.iter().flat_map(|r| &r.glyphs).filter(|g| !g.chr.is_whitespace()).all(|g| g.uv_rect.size.x > 0.0 && g.uv_rect.size.y > 0.0)
        );
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
        let mut out = ctx.run_ui(Default::default(), |ui| laid = Some(galley(ui, &body, 12.0, Color32::BLACK, 300.0)));
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
        let j = job(&wild, 12.0, Color32::BLACK, 300.0);
        assert!(j.sections.iter().all(|s| s.leading_space.is_finite() && s.format.font_id.size <= 24.0));
    }
}
