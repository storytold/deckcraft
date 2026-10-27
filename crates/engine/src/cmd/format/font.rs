use super::*;

pub(super) fn bold(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, current_run(s).bold);
    format_run(s, &move |r| r.bold = Some(v))
}

pub(super) fn italic(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, current_run(s).italic);
    format_run(s, &move |r| r.italic = Some(v))
}

pub(super) fn underline(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = current_run(s).underline.is_some_and(|u| u != "none");
    let v = on_param(p, Some(cur));
    let style = str_param(p, "style").unwrap_or("sng").to_string();
    format_run(s, &move |r| r.underline = Some(if v { style.clone() } else { "none".into() }))
}

pub(super) fn strike(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = current_run(s).strike.is_some_and(|x| x != Strike::None);
    let v = on_param(p, Some(cur));
    let d = bool_or(p, "double", false);
    format_run(s, &move |r| {
        r.strike = Some(if !v {
            Strike::None
        } else if d {
            Strike::Double
        } else {
            Strike::Single
        })
    })
}

pub(super) fn superscript(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, Some(current_run(s).baseline.unwrap_or(0.0) > 0.0));
    format_run(s, &move |r| r.baseline = Some(if v { 0.3 } else { 0.0 }))
}

pub(super) fn subscript(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, Some(current_run(s).baseline.unwrap_or(0.0) < 0.0));
    format_run(s, &move |r| r.baseline = Some(if v { -0.25 } else { 0.0 }))
}

pub(super) fn font(s: &mut Session, p: &Value) -> Result<Value> {
    let f = str_param(p, "family").ok_or_else(|| bad("format.font", "missing `family`"))?.to_string();
    format_run(s, &move |r| r.font = Some(f.clone()))
}

pub(super) fn size(s: &mut Session, p: &Value) -> Result<Value> {
    let v = f64_param(p, "size").ok_or_else(|| bad("format.size", "missing `size`"))?.clamp(1.0, 4000.0);
    format_run(s, &move |r| r.size = Some(v))
}

/// PowerPoint's font size list (grow/shrink steps through it).
pub const SIZES: [f64; 20] = [8.0, 9.0, 10.0, 10.5, 11.0, 12.0, 14.0, 16.0, 18.0, 20.0, 24.0, 28.0, 32.0, 36.0, 40.0, 44.0, 48.0, 54.0, 60.0, 66.0];

fn step(cur: f64, up: bool) -> f64 {
    if up {
        SIZES.iter().copied().find(|v| *v > cur + 0.01).unwrap_or((cur + 8.0).min(4000.0))
    } else {
        SIZES.iter().rev().copied().find(|v| *v < cur - 0.01).unwrap_or((cur - 1.0).max(1.0))
    }
}

fn grow_shrink(s: &mut Session, up: bool) -> Result<Value> {
    // Each run steps from its own size, like PowerPoint.
    let st = s.doc()?;
    let ctx_sizes = current_run(s).size.unwrap_or(18.0);
    let _ = st;
    let base = ctx_sizes;
    format_run(s, &move |r| {
        let cur = r.size.unwrap_or(base);
        r.size = Some(step(cur, up));
    })
}

pub(super) fn grow(s: &mut Session, _p: &Value) -> Result<Value> {
    grow_shrink(s, true)
}

pub(super) fn shrink(s: &mut Session, _p: &Value) -> Result<Value> {
    grow_shrink(s, false)
}

pub(super) fn color(s: &mut Session, p: &Value) -> Result<Value> {
    let c = color_param(p, "color").ok_or_else(|| bad("format.color", "missing or invalid `color`"))?;
    // A selected chart has no runs: Font Color colours its text (title, axes, legend, labels),
    // which used to answer and change nothing.
    let st = s.doc()?;
    if st.selection.text.is_none() {
        let charts: Vec<ShapeId> =
            st.selection.shapes.iter().copied().filter(|id| matches!(st.shape(*id).map(|sh| &sh.kind), Some(ShapeKind::Chart(_)))).collect();
        let only_charts = charts.len() == st.selection.shapes.len();
        if !charts.is_empty() {
            let tc = c.clone();
            edit_shapes(s, &json!({"ids": charts}), "format.color", |sh| {
                if let ShapeKind::Chart(ch) = &mut sh.kind {
                    ch.text_color = Some(tc.clone());
                    ch.raw = None;
                }
                Ok(())
            })?;
            if only_charts {
                return ok();
            }
        }
    }
    format_run(s, &move |r| r.fill = Some(Fill::solid(c.clone())))
}

/// `a:alpha` for `transparency` (0–1) on `c`, replacing any alpha it had; fully opaque drops it.
fn with_transparency(c: &mut ColorRef, transparency: f64) {
    c.mods.retain(|m| !matches!(m, ColorTransform::Alpha(_)));
    if transparency > 0.0 {
        c.mods.push(ColorTransform::Alpha(((1.0 - transparency) * 100_000.0).round() as i32));
    }
}

/// Text Fill › Transparency: the text keeps its colour and gets an alpha. A run without its own
/// fill takes the colour it shows now (from the placeholder, master or theme) so only its
/// opacity changes.
pub(super) fn text_transparency(s: &mut Session, p: &Value) -> Result<Value> {
    let v = f64_param(p, "value").ok_or_else(|| bad("format.textTransparency", "missing `value` (0–1)"))?;
    if !v.is_finite() {
        return Err(bad("format.textTransparency", "`value` must be a number from 0 to 1"));
    }
    let v = v.clamp(0.0, 1.0);
    let shown = match current_run(s).fill {
        Some(f @ (Fill::Solid { .. } | Fill::Gradient(_))) => f,
        _ => Fill::solid(ColorRef::scheme(SchemeSlot::Tx1)),
    };
    format_run(s, &move |r| {
        let mut f = match &r.fill {
            Some(f @ (Fill::Solid { .. } | Fill::Gradient(_))) => f.clone(),
            Some(Fill::None) => return,
            _ => shown.clone(),
        };
        match &mut f {
            Fill::Solid { color } => with_transparency(color, v),
            Fill::Gradient(g) => g.stops.iter_mut().for_each(|st| with_transparency(&mut st.color, v)),
            _ => {}
        }
        r.fill = Some(f);
    })
}

/// The `lang` and `noProof` params of the language commands, checked: a run-props setter.
fn language_edit(id: &str, p: &Value) -> Result<impl Fn(&mut RunProps) + Send + Sync + 'static> {
    let lang = str_param(p, "lang").map(str::to_string);
    let no_proof = bool_param(p, "noProof");
    if lang.is_none() && no_proof.is_none() {
        return Err(bad(id, "give `lang` or `noProof`"));
    }
    if lang.as_deref().is_some_and(|l| l.is_empty() || l.len() > 35 || !l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')) {
        return Err(bad(id, "`lang` is a language tag such as en-US"));
    }
    Ok(move |r: &mut RunProps| {
        if let Some(l) = &lang {
            r.lang = Some(l.clone());
        }
        if let Some(n) = no_proof {
            r.no_proof = n.then_some(true);
        }
    })
}

/// Review › Language on the selected text (or the selected shapes' text).
pub(super) fn language(s: &mut Session, p: &Value) -> Result<Value> {
    let f = language_edit("format.language", p)?;
    format_run(s, &f)
}

/// Every run in the deck, and the text styles new text inherits from, in one language.
pub(super) fn language_all(s: &mut Session, p: &Value) -> Result<Value> {
    let f = language_edit("format.languageAll", p)?;
    /// Runs, end marks and the body's own list style levels (never creating one).
    fn body(t: &mut deckcraft_model::text::TextBody, f: &dyn Fn(&mut RunProps)) {
        ed::format_all(t, f);
        t.list_style.levels.iter_mut().flatten().for_each(|lv| f(&mut lv.run));
    }
    /// Every text body in `list`: shapes, table cells and groups (nested at most 64 deep).
    fn shapes(list: &mut [Shape], f: &dyn Fn(&mut RunProps), depth: usize) {
        if depth > 64 {
            return;
        }
        for sh in list {
            if let Some(t) = &mut sh.text {
                body(t, f);
            }
            match &mut sh.kind {
                deckcraft_model::ShapeKind::Table(t) => t.rows.iter_mut().flat_map(|r| &mut r.cells).for_each(|c| body(&mut c.text, f)),
                deckcraft_model::ShapeKind::Group { children, .. } => shapes(children, f, depth + 1),
                _ => {}
            }
        }
    }
    /// A master or presentation text style's levels: what new text inherits.
    fn styles(l: &mut deckcraft_model::text::ListStyle, f: &dyn Fn(&mut RunProps)) {
        // Level 1 always, so a deck without a default text style gets one.
        if l.level(0).is_none() {
            l.set(0, Default::default());
        }
        l.levels.iter_mut().flatten().for_each(|lv| f(&mut lv.run));
    }
    s.edit(|doc, _| {
        for sl in &mut doc.slides {
            let sl = Arc::make_mut(sl);
            shapes(&mut sl.shapes, &f, 0);
            body(&mut sl.notes, &f);
        }
        let extra = [doc.notes_master.as_mut(), doc.handout_master.as_mut()];
        for m in doc.masters.iter_mut().chain(extra.into_iter().flatten()) {
            let m = Arc::make_mut(m);
            shapes(&mut m.shapes, &f, 0);
            m.layouts.iter_mut().for_each(|l| shapes(&mut l.shapes, &f, 0));
            [&mut m.title_style, &mut m.body_style, &mut m.other_style].into_iter().for_each(|l| styles(l, &f));
        }
        styles(&mut doc.default_text_style, &f);
        Ok(())
    })?;
    ok()
}

pub(super) fn highlight(s: &mut Session, p: &Value) -> Result<Value> {
    let c = color_param(p, "color");
    format_run(s, &move |r| r.highlight = c.clone())
}

pub(super) fn caps(s: &mut Session, p: &Value) -> Result<Value> {
    let c = match str_param(p, "caps").unwrap_or("none") {
        "small" => Caps::Small,
        "all" => Caps::All,
        _ => Caps::None,
    };
    format_run(s, &move |r| r.caps = Some(c))
}

pub(super) fn spacing(s: &mut Session, p: &Value) -> Result<Value> {
    let v = f64_param(p, "pt").unwrap_or(0.0).clamp(-100.0, 400.0);
    format_run(s, &move |r| r.spacing = Some(v))
}

/// Character formatting applied together, so the Font dialog is one undo step.
struct FontEdits {
    family: Option<String>,
    size: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<String>,
    color: Option<deckcraft_model::ColorRef>,
    strike: Option<Strike>,
    baseline: Option<f64>,
    caps: Option<Caps>,
    spacing: Option<f64>,
}

impl FontEdits {
    fn is_empty(&self) -> bool {
        self.family.is_none()
            && self.size.is_none()
            && self.bold.is_none()
            && self.italic.is_none()
            && self.underline.is_none()
            && self.color.is_none()
            && self.strike.is_none()
            && self.baseline.is_none()
            && self.caps.is_none()
            && self.spacing.is_none()
    }

    fn write(&self, r: &mut RunProps) {
        if let Some(f) = &self.family {
            r.font = Some(f.clone());
        }
        if let Some(v) = self.size {
            r.size = Some(v);
        }
        if let Some(v) = self.bold {
            r.bold = Some(v);
        }
        if let Some(v) = self.italic {
            r.italic = Some(v);
        }
        if let Some(u) = &self.underline {
            r.underline = Some(u.clone());
        }
        if let Some(c) = &self.color {
            r.fill = Some(Fill::solid(c.clone()));
        }
        if let Some(v) = self.strike {
            r.strike = Some(v);
        }
        if let Some(v) = self.baseline {
            r.baseline = Some(v);
        }
        if let Some(v) = self.caps {
            r.caps = Some(v);
        }
        if let Some(v) = self.spacing {
            r.spacing = Some(v);
        }
    }
}

fn font_bool(p: &Value, key: &str) -> Result<Option<bool>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(_) => Err(bad("format.fontDialog", format!("`{key}` must be a bool"))),
    }
}

fn font_str<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(bad("format.fontDialog", format!("`{key}` must be a string"))),
    }
}

fn font_edits(p: &Value) -> Result<FontEdits> {
    let family = match font_str(p, "family")? {
        None => None,
        Some(s) => {
            let t = s.trim();
            if t.is_empty() || t.chars().count() > 200 {
                return Err(bad("format.fontDialog", "`family` must be 1–200 characters"));
            }
            Some(t.to_string())
        }
    };
    let size = match p.get("size") {
        None => None,
        Some(v) => {
            let n = v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad("format.fontDialog", "`size` must be a finite number"))?;
            Some(n.clamp(1.0, 4000.0))
        }
    };
    let underline = match font_str(p, "underline")? {
        None => None,
        Some(s) => {
            if !matches!(s, "none" | "sng" | "dbl" | "heavy" | "dotted" | "dash" | "wavy") {
                return Err(bad("format.fontDialog", "`underline` must be none, sng, dbl, heavy, dotted, dash or wavy"));
            }
            Some(s.to_string())
        }
    };
    let color =
        if p.get("color").is_some() { Some(color_param(p, "color").ok_or_else(|| bad("format.fontDialog", "invalid `color`"))?) } else { None };
    let strike = match font_str(p, "strike")? {
        None => None,
        Some("none") => Some(Strike::None),
        Some("sng" | "single") => Some(Strike::Single),
        Some("dbl" | "double") => Some(Strike::Double),
        Some(_) => return Err(bad("format.fontDialog", "`strike` must be none, sng or dbl")),
    };
    let baseline = match p.get("baseline") {
        None => None,
        Some(Value::String(s)) => match s.as_str() {
            "none" => Some(0.0),
            "super" | "superscript" => Some(0.3),
            "sub" | "subscript" => Some(-0.25),
            _ => return Err(bad("format.fontDialog", "`baseline` must be none, super, sub or a fraction")),
        },
        Some(Value::Number(n)) => {
            let f = n.as_f64().filter(|v| v.is_finite()).ok_or_else(|| bad("format.fontDialog", "`baseline` must be finite"))?;
            Some(f.clamp(-1.0, 1.0))
        }
        Some(_) => return Err(bad("format.fontDialog", "`baseline` must be none, super, sub or a fraction")),
    };
    let caps = match font_str(p, "caps")? {
        None => None,
        Some("none") => Some(Caps::None),
        Some("small") => Some(Caps::Small),
        Some("all") => Some(Caps::All),
        Some(_) => return Err(bad("format.fontDialog", "`caps` must be none, small or all")),
    };
    let spacing = match p.get("spacing") {
        None => None,
        Some(v) => {
            let n = v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad("format.fontDialog", "`spacing` must be a finite number"))?;
            Some(n.clamp(-100.0, 400.0))
        }
    };
    Ok(FontEdits { family, size, bold: font_bool(p, "bold")?, italic: font_bool(p, "italic")?, underline, color, strike, baseline, caps, spacing })
}

/// A colour as a `format.fontDialog` / `format.color` parameter, including transforms.
fn color_command_value(c: &deckcraft_model::ColorRef) -> Value {
    use deckcraft_model::ColorBase;
    let mut o = serde_json::Map::new();
    for m in &c.mods {
        if let Some(v) = m.value() {
            o.insert(m.xml_name().to_string(), json!(v));
        }
    }
    match &c.base {
        ColorBase::Rgb { rgb } if o.is_empty() => json!(format!("#{}", rgb.hex())),
        ColorBase::Rgb { rgb } => {
            o.insert("rgb".into(), json!(format!("#{}", rgb.hex())));
            Value::Object(o)
        }
        ColorBase::Scheme { slot } => {
            o.insert("scheme".into(), json!(slot.xml_name()));
            Value::Object(o)
        }
        ColorBase::Preset { name } => json!(name),
        ColorBase::System { last, .. } => json!(format!("#{}", last.hex())),
    }
}

fn font_dialog_params(s: &Session) -> Value {
    let r = current_run(s);
    let family =
        s.doc().ok().and_then(|st| crate::cmd::text::ctx_for(&st.doc, &st.selection).map(|c| resolve::font_family(&c, &r))).unwrap_or_default();
    let baseline = r.baseline.unwrap_or(0.0);
    let mut v = json!({
        "family": family,
        "size": r.size.unwrap_or(18.0),
        "bold": r.bold.unwrap_or(false),
        "italic": r.italic.unwrap_or(false),
        "underline": r.underline.as_deref().filter(|u| *u != "none").unwrap_or("none"),
        "strike": match r.strike {
            Some(Strike::Double) => "dbl",
            Some(Strike::Single) => "sng",
            _ => "none",
        },
        "baseline": if baseline > 0.0 { "super" } else if baseline < 0.0 { "sub" } else { "none" },
        "caps": match r.caps {
            Some(Caps::Small) => "small",
            Some(Caps::All) => "all",
            _ => "none",
        },
        "spacing": r.spacing.unwrap_or(0.0),
    });
    if let Some(Fill::Solid { color }) = &r.fill {
        v["color"] = color_command_value(color);
    }
    v
}

pub(super) fn font_dialog(s: &mut Session, p: &Value) -> Result<Value> {
    let edits = font_edits(p)?;
    if edits.is_empty() {
        s.ui_requests.push(crate::UiRequest::Dialog { id: "font".into(), params: font_dialog_params(s) });
        return ok();
    }
    format_run(s, &|r| edits.write(r))
}

pub(super) fn clear(s: &mut Session, _p: &Value) -> Result<Value> {
    format_run(s, &|r| *r = RunProps { lang: r.lang.clone(), link: r.link.clone(), ..Default::default() })
}
