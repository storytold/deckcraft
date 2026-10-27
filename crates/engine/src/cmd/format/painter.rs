use super::*;

/// Formatting picked up by the Format Painter.
#[derive(Clone, Debug, Default)]
pub struct Painted {
    pub run: Option<RunProps>,
    pub para: Option<ParaProps>,
    pub fill: Option<Fill>,
    pub line: Option<Line>,
    pub effects: Option<Effects>,
    pub style: Option<deckcraft_model::ShapeStyle>,
}

pub(super) fn painter_pick(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mut painted = Painted::default();
    if let Some(t) = &st.selection.text {
        if let Some(b) = crate::cmd::text::body_of(st, t) {
            let a = t.ordered().0;
            painted.run = Some(ed::props_at(b, (a.0, a.1 + 1)));
            painted.para = b.paragraphs.get(a.0).map(|x| x.props.clone());
        }
    } else if let Some(sh) = st.selected_shapes().first() {
        painted.fill = sh.fill.clone();
        painted.line = sh.line.clone();
        painted.effects = sh.effects.clone();
        painted.style = sh.style.clone();
        painted.run = sh.text.as_ref().and_then(|t| t.paragraphs.first()).and_then(|p| p.runs.first()).map(|r| r.props.clone());
    }
    s.painter = Some((painted, bool_or(p, "sticky", false)));
    Ok(json!({"armed": true}))
}

pub(crate) fn apply_painted(sh: &mut Shape, p: &Painted) {
    if p.fill.is_some() || p.style.is_some() {
        sh.fill.clone_from(&p.fill);
        sh.line.clone_from(&p.line);
        sh.effects.clone_from(&p.effects);
        sh.style.clone_from(&p.style);
    }
    if let (Some(r), Some(t)) = (&p.run, sh.text.as_mut()) {
        ed::format_all(t, &|rp| *rp = r.clone());
    }
}

/// The paragraphs the text selection covers whole: it starts at or before their start and
/// reaches their end. A paragraph the selection only touches the start of (it ends at offset
/// 0, as after a triple-click) is not covered.
fn covered_paragraphs(s: &Session) -> Vec<usize> {
    let Ok(st) = s.doc() else { return vec![] };
    let Some(t) = &st.selection.text else { return vec![] };
    let Some(body) = crate::cmd::text::body_of(st, t) else { return vec![] };
    let (a, b) = t.ordered();
    // A hostile selection can point past the body: walk only the paragraphs it has.
    (a.0..=b.0.min(body.paragraphs.len().saturating_sub(1)))
        .filter(|&pi| {
            let Some(para) = body.paragraphs.get(pi) else { return false };
            let starts = pi > a.0 || a.1 == 0;
            let ends = if pi < b.0 {
                true
            } else if b.0 > a.0 && b.1 == 0 {
                false
            } else {
                b.1 >= para.char_len()
            };
            starts && ends
        })
        .collect()
}

pub(super) fn painter_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let Some((painted, sticky)) = s.painter.clone() else { return Err(bad("format.painterApply", "pick formatting with the Format Painter first")) };
    if !sticky {
        s.painter = None;
    }
    if s.doc()?.selection.text.as_ref().is_some_and(|t| t.is_range()) && ids_param(p, "ids").is_none() {
        if let Some(r) = painted.run.clone() {
            format_run(s, &move |rp| *rp = r.clone())?;
        }
        // Paragraph formatting (bullets, alignment, spacing) goes only onto paragraphs the
        // selection covers whole; painting part of a bulleted line changes just its text.
        let covered = covered_paragraphs(s);
        if let (Some(pp), false) = (painted.para.clone(), covered.is_empty()) {
            crate::cmd::text::edit_text(s, |body, _| {
                for &pi in &covered {
                    if let Some(para) = body.paragraphs.get_mut(pi) {
                        para.props = pp.clone();
                    }
                }
            })?;
        }
        return ok();
    }
    edit_shapes(s, p, "format.painterApply", |sh| {
        apply_painted(sh, &painted);
        Ok(())
    })
}

pub(super) fn state(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = current_run(s);
    let pp = current_para(s);
    let st = s.doc()?;
    let family = crate::cmd::text::ctx_for(&st.doc, &st.selection).map(|c| resolve::font_family(&c, &r)).unwrap_or_default();
    Ok(json!({
        "font": family,
        "fontRef": r.font,
        "size": r.size,
        "bold": r.bold.unwrap_or(false),
        "italic": r.italic.unwrap_or(false),
        "underline": r.underline.as_deref().is_some_and(|u| u != "none"),
        "strike": r.strike.is_some_and(|x| x != Strike::None),
        "superscript": r.baseline.unwrap_or(0.0) > 0.0,
        "subscript": r.baseline.unwrap_or(0.0) < 0.0,
        "align": pp.align.map(|a| a.xml()),
        "bullets": matches!(pp.bullet, Some(Bullet::Char { .. })),
        "numbering": matches!(pp.bullet, Some(Bullet::AutoNum { .. })),
        "lang": r.lang,
        "lineSpacing": serde_json::to_value(pp.line_spacing).unwrap_or_default(),
        "painter": s.painter.is_some(),
    }))
}

/// Add a font from its bytes for this session (it then shows in the font menu and draws).
pub(super) fn font_add(_s: &mut Session, p: &Value) -> Result<Value> {
    let data = str_param(p, "data").and_then(base64_decode).ok_or_else(|| bad("font.add", "missing or invalid base64 `data`"))?;
    if data.len() > 64 << 20 {
        return Err(bad("font.add", "font file larger than 64 MB"));
    }
    let faces = deckcraft_fonts::FontDb::global().add_font(data);
    if faces == 0 {
        return Err(bad("font.add", "not a font DeckCraft can read (or already added)"));
    }
    Ok(json!({"faces": faces}))
}

/// Catalog the installed fonts again, so fonts installed while DeckCraft runs appear.
pub(super) fn font_rescan(_s: &mut Session, _p: &Value) -> Result<Value> {
    let db = deckcraft_fonts::FontDb::global();
    #[cfg(not(target_arch = "wasm32"))]
    let faces = db.load_system_fonts();
    #[cfg(target_arch = "wasm32")]
    let faces = 0usize;
    Ok(json!({"faces": faces, "families": db.families().len()}))
}
