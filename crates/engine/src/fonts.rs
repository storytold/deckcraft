//! Fonts a presentation uses that this machine doesn't have, so opening it can say so (its text
//! draws in a stand-in; see `FontDb::document_face`).

use deckcraft_fonts::FontDb;
use deckcraft_model::resolve::{self, Ctx};
use deckcraft_model::text::{Bullet, TextBody};
use deckcraft_model::{Presentation, Shape, ShapeKind};

/// Warm the document-scoped font cache. Embedded faces take precedence only inside this
/// presentation; they never replace the global installed/bundled family or another document's.
pub fn install_embedded_fonts(p: &Presentation) {
    for font in &p.embedded_fonts {
        for id in font.faces().into_iter().flatten() {
            let Some(item) = p.media(id) else { continue };
            let _ = deckcraft_fonts::embedded_face(item.data.clone(), &font.typeface, "Regular");
        }
    }
}

/// The font families the slides' text is drawn in (theme fonts resolved, bullet fonts included)
/// that aren't installed or bundled, sorted, without duplicates (case-insensitive). Names that
/// start with a dot are macOS hidden system fonts (`.SF NS`); they never appear in a font list
/// and can't be installed, so they aren't reported.
pub fn missing_fonts(p: &Presentation) -> Vec<String> {
    let db = FontDb::global();
    let mut out: Vec<String> = Vec::new();
    for family in used_fonts(p) {
        let embedded = p.embedded_fonts.iter().filter(|f| f.typeface.eq_ignore_ascii_case(&family)).any(|f| {
            f.faces()
                .into_iter()
                .flatten()
                .filter_map(|id| p.media(id))
                .any(|m| deckcraft_fonts::embedded_face(m.data.clone(), &family, "Regular").is_some())
        });
        if !family.starts_with('.') && !embedded && !db.has_family(&family) && !out.iter().any(|f| f.eq_ignore_ascii_case(&family)) {
            out.push(family);
        }
    }
    out.sort_by_key(|f| f.to_lowercase());
    out
}

/// The families the slides' text runs and bullets are drawn in.
pub fn used_fonts(p: &Presentation) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |f: String| {
        let f = f.trim().to_string();
        if !f.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(&f)) {
            out.push(f);
        }
    };
    for slide in &p.slides {
        let Some(ctx) = Ctx::for_slide(p, slide) else { continue };
        deckcraft_model::walk(&slide.shapes, &mut |s, _| {
            if let Some(body) = &s.text {
                body_fonts(&ctx, s, body, &mut add);
            }
            if let ShapeKind::Table(t) = &s.kind {
                for cell in t.rows.iter().flat_map(|r| &r.cells) {
                    body_fonts(&ctx, s, &cell.text, &mut add);
                }
            }
            if let ShapeKind::Chart(c) = &s.kind {
                for (_, body, _) in chart_samples(c) {
                    let shape = Shape { text: Some(body.clone()), ..Default::default() };
                    body_fonts(&ctx, &shape, &body, &mut add);
                }
            }
        });
    }
    out
}

/// Read-only diagnostics from the renderer's font selection. Locations are source character
/// ranges, never document text or local paths, so reports can be shared without either.
pub fn inspect_fonts(p: &Presentation) -> serde_json::Value {
    use serde_json::json;
    let mut out = Vec::new();
    for (slide_index, slide) in p.slides.iter().enumerate() {
        let Some(ctx) = Ctx::for_slide(p, slide) else { continue };
        deckcraft_model::walk(&slide.shapes, &mut |shape, _| {
            let mut inspect = |body: &TextBody, cell: Option<(usize, usize)>, role: Option<deckcraft_model::chart::ChartTextRole>, sampled: bool| {
                let label_shape = Shape { text: Some(body.clone()), ..Default::default() };
                for f in deckcraft_text::font_usage(&ctx, if role.is_some() { &label_shape } else { shape }, body) {
                    out.push(json!({ "slide": slide_index + 1, "shape": shape.id.0, "cell": cell,
                        "component": role, "sampled": sampled,
                        "paragraph": f.paragraph, "run": f.run, "start": f.start, "end": f.end,
                        "requested": f.requested, "actual": f.actual.family, "style": f.actual.style,
                        "faceIndex": f.actual.index(), "fontSha256": f.actual.sha256(), "variationCoordinates": f.actual.coords,
                        "embedded": f.actual.embedded, "reason": f.reason, "missingGlyph": f.missing_glyph,
                        "syntheticBold": f.synthetic_bold, "syntheticItalic": f.synthetic_italic }));
                }
            };
            if let Some(body) = &shape.text {
                inspect(body, None, None, false);
            }
            if let ShapeKind::Table(table) = &shape.kind {
                for (row, r) in table.rows.iter().enumerate() {
                    for (column, c) in r.cells.iter().enumerate() {
                        inspect(&c.text, Some((row, column)), None, false);
                    }
                }
            }
            if let ShapeKind::Chart(c) = &shape.kind {
                for (role, body, sampled) in chart_samples(c) {
                    inspect(&body, None, Some(role), sampled);
                }
            }
        });
    }
    json!(out)
}

/// Actual title/legend/category strings plus representative numeric axis labels. Tick values
/// depend on the frame size, so those rows are explicitly marked as samples, not source ranges.
fn chart_samples(c: &deckcraft_model::Chart) -> Vec<(deckcraft_model::chart::ChartTextRole, TextBody, bool)> {
    use deckcraft_model::chart::{ChartTextRole as Role, ChartType};
    let mut samples = Vec::new();
    let mut add = |role, text: &str, sampled| {
        let mut body = TextBody::from_text(text);
        for para in &mut body.paragraphs {
            para.props.bullet = Some(Bullet::None);
            for run in &mut para.runs {
                run.props = c.text_props(role);
                if run.props.bold.is_none() {
                    run.props.bold = Some(matches!(role, Role::CategoryTitle | Role::ValueTitle));
                }
            }
        }
        samples.push((role, body, sampled));
    };
    if let Some(title) = &c.title {
        add(Role::Title, title, false);
    }
    for (role, title) in [(Role::CategoryTitle, &c.axis_titles.0), (Role::ValueTitle, &c.axis_titles.1)] {
        if let Some(title) = title {
            add(role, title, false);
        }
    }
    let pie = matches!(c.kind, ChartType::Pie | ChartType::Doughnut);
    for category in &c.categories {
        if !pie {
            add(Role::CategoryAxis, category, false);
        }
        if pie && c.legend.is_some() {
            add(Role::Legend, category, false);
        }
    }
    if !pie && c.legend.is_some() {
        for series in &c.series {
            add(Role::Legend, &series.name, false);
        }
    }
    if !pie {
        for (secondary, role, hidden) in [(false, Role::ValueAxis, c.y_hidden), (true, Role::SecondaryValueAxis, c.y2_hidden)] {
            if !hidden && let Some((min, max, _)) = c.axis_scale(secondary, 6) {
                for value in [min, max] {
                    add(role, &c.axis_label(secondary, value), true);
                }
            }
        }
    }
    samples
}

fn body_fonts(ctx: &Ctx, s: &Shape, body: &TextBody, add: &mut dyn FnMut(String)) {
    for family in deckcraft_text::requested_fonts(ctx, s, body) {
        add(family);
    }
    for para in &body.paragraphs {
        let has_text = para.runs.iter().any(|r| !r.text.trim().is_empty());
        if !has_text {
            continue;
        }
        let pp = resolve::para(ctx, s, para);
        if matches!(pp.bullet, Some(Bullet::Char { .. }))
            && let Some(f) = pp.bullet_font.as_deref()
        {
            add(ctx.master.theme.font(f));
        }
    }
}

/// Replace the font family `from` with `to` everywhere it is named explicitly: text runs (and
/// their east Asian, complex script and symbol faces), bullet fonts, paragraph end marks, list
/// styles of shapes, layouts and masters, the master text styles, the deck default style, speaker
/// notes, tables, chart text roles and the theme fonts. Names compare case-insensitively; theme references
/// (`+mn-lt`) are left alone. Returns how many names changed.
pub fn replace_font(p: &mut Presentation, from: &str, to: &str) -> usize {
    use std::sync::Arc;
    let from = from.trim();
    let to = to.trim();
    if from.is_empty() || to.is_empty() || from.starts_with('+') || from == to {
        return 0;
    }
    let mut n = 0usize;
    let mut fix = |f: &mut Option<String>| {
        if f.as_deref().is_some_and(|x| x.trim().eq_ignore_ascii_case(from)) {
            *f = Some(to.to_string());
            n = n.saturating_add(1);
        }
    };
    replace_list(&mut p.default_text_style, &mut fix);
    for slide in &mut p.slides {
        let sl = Arc::make_mut(slide);
        replace_shapes(&mut sl.shapes, &mut fix);
        replace_body(&mut sl.notes, &mut fix);
    }
    let mut theme_hits = 0usize;
    let masters = p.masters.iter_mut().chain(p.notes_master.iter_mut()).chain(p.handout_master.iter_mut());
    for m in masters {
        let m = Arc::make_mut(m);
        replace_shapes(&mut m.shapes, &mut fix);
        for l in [&mut m.title_style, &mut m.body_style, &mut m.other_style] {
            replace_list(l, &mut fix);
        }
        for lay in &mut m.layouts {
            replace_shapes(&mut lay.shapes, &mut fix);
        }
        for set in [&mut m.theme.fonts.major, &mut m.theme.fonts.minor] {
            for face in [&mut set.latin, &mut set.ea, &mut set.cs].into_iter().chain(set.scripts.iter_mut().map(|s| &mut s.typeface)) {
                if face.trim().eq_ignore_ascii_case(from) {
                    *face = to.to_string();
                    theme_hits = theme_hits.saturating_add(1);
                }
            }
        }
    }
    n.saturating_add(theme_hits)
}

fn replace_shapes(shapes: &mut [Shape], fix: &mut dyn FnMut(&mut Option<String>)) {
    for s in shapes {
        if let Some(body) = &mut s.text {
            replace_body(body, fix);
        }
        if let ShapeKind::Table(t) = &mut s.kind {
            for cell in t.rows.iter_mut().flat_map(|r| &mut r.cells) {
                replace_body(&mut cell.text, fix);
            }
        }
        if let ShapeKind::Chart(c) = &mut s.kind {
            for props in c.text_styles.values_mut() {
                replace_run(props, fix);
            }
        }
        if let Some(children) = s.children_mut() {
            replace_shapes(children, fix);
        }
    }
}

fn replace_body(body: &mut TextBody, fix: &mut dyn FnMut(&mut Option<String>)) {
    replace_list(&mut body.list_style, fix);
    for para in &mut body.paragraphs {
        fix(&mut para.props.bullet_font);
        replace_run(&mut para.end_props, fix);
        for run in &mut para.runs {
            replace_run(&mut run.props, fix);
        }
    }
}

fn replace_list(l: &mut deckcraft_model::text::ListStyle, fix: &mut dyn FnMut(&mut Option<String>)) {
    for lvl in l.levels.iter_mut().flatten() {
        fix(&mut lvl.para.bullet_font);
        replace_run(&mut lvl.run, fix);
    }
}

fn replace_run(r: &mut deckcraft_model::text::RunProps, fix: &mut dyn FnMut(&mut Option<String>)) {
    for f in [&mut r.font, &mut r.font_ea, &mut r.font_cs, &mut r.font_sym] {
        fix(f);
    }
}
