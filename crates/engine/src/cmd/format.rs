//! Home ▸ Font and Paragraph, Format menu: character, paragraph and text box formatting, and
//! the Format Painter.

use deckcraft_model::edit as ed;
use deckcraft_model::resolve;
use deckcraft_model::text::{Align, Anchor, AutoFit, Bullet, Caps, ParaProps, RunProps, Spacing, Strike, TabStop, TextDir};
use deckcraft_model::{ColorRef, ColorTransform, Effects, Fill, Line, SchemeSlot, Shape, ShapeKind};
use serde_json::{Value, json};
use std::sync::Arc;

use super::text::{format_body, format_para, format_run};
use super::{
    CommandSpec, always, bad, base64_decode, bool_or, bool_param, cmd, color_param, edit_shapes, f64_or, f64_param, has_doc, has_text,
    has_text_or_shapes, ids_param, ok, str_param,
};
use crate::{Result, Session};
use deckcraft_model::ShapeId;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(noundo "font.add", "Add Font", [], None, "{data: base64 TTF/OTF/TTC} → {faces} added for this session", always, font_add),
        cmd!(noundo "font.rescan", "Rescan Installed Fonts", ["Home", "Font"], None, "{} → {faces, families} (fonts installed since start-up)", always, font_rescan),
        cmd!("format.bold", "Bold", ["Home", "Font"], Some("Cmd+B"), "{on?: bool}", has_text_or_shapes, bold),
        cmd!("format.italic", "Italic", ["Home", "Font"], Some("Cmd+I"), "{on?: bool}", has_text_or_shapes, italic),
        cmd!(
            "format.underline",
            "Underline",
            ["Home", "Font"],
            Some("Cmd+U"),
            "{on?: bool, style?: sng|dbl|heavy|dotted|dash|wavy}",
            has_text_or_shapes,
            underline
        ),
        cmd!(
            "format.strikethrough",
            "Strikethrough",
            ["Home", "Font"],
            Some("Cmd+Shift+X"),
            "{on?: bool, double?: bool}",
            has_text_or_shapes,
            strike
        ),
        cmd!("format.superscript", "Superscript", ["Home", "Font"], Some("Cmd+Shift+="), "{on?: bool}", has_text_or_shapes, superscript),
        cmd!("format.subscript", "Subscript", ["Home", "Font"], Some("Cmd+="), "{on?: bool}", has_text_or_shapes, subscript),
        cmd!("format.font", "Font", ["Home", "Font"], None, "{family}", has_text_or_shapes, font),
        cmd!("format.size", "Font Size", ["Home", "Font"], None, "{size: pt}", has_text_or_shapes, size),
        cmd!("format.grow", "Increase Font Size", ["Home", "Font"], Some("Cmd+Shift+."), "{}", has_text_or_shapes, grow),
        cmd!("format.shrink", "Decrease Font Size", ["Home", "Font"], Some("Cmd+Shift+,"), "{}", has_text_or_shapes, shrink),
        cmd!("format.color", "Font Color", ["Home", "Font"], None, "{color: #RRGGBB|accent1..6|tx1…|{scheme, lumMod…}}", has_text_or_shapes, color),
        cmd!(
            "format.textTransparency",
            "Text Transparency",
            ["Shape Format", "WordArt Styles"],
            None,
            "{value: 0..1} — 0 opaque, 1 invisible; keeps the text's colour (inherited colours included)",
            has_text_or_shapes,
            text_transparency
        ),
        cmd!(
            "format.language",
            "Language",
            ["Review", "Language"],
            None,
            "{lang?, noProof?: bool} (proofing language of the selected text, and Do Not Check Spelling)",
            has_text_or_shapes,
            language
        ),
        cmd!(
            "format.languageAll",
            "Language for All Text",
            ["Review", "Language"],
            None,
            "{lang?, noProof?: bool} (proofing language of every text in the presentation: slides, tables, notes, masters and layouts, and the default for new text; one undo step)",
            has_doc,
            language_all
        ),
        cmd!("format.highlight", "Text Highlight Color", ["Home", "Font"], None, "{color? (absent = none)}", has_text_or_shapes, highlight),
        cmd!(
            "format.case",
            "Change Case",
            ["Home", "Font"],
            Some("Shift+F3"),
            "{mode?: cycle|sentence|lower|upper|title|toggle} — cycle (Shift+F3, the default) goes lowercase → Capitalize Each Word → UPPERCASE; a caret in a word changes that word",
            has_text_or_shapes,
            case
        ),
        cmd!("format.caps", "Caps", [], None, "{caps: none|small|all}", has_text_or_shapes, caps),
        cmd!(
            "format.spacing",
            "Character Spacing",
            ["Home", "Font"],
            None,
            "{pt} (veryTight -3, tight -1.5, normal 0, loose 3, veryLoose 6)",
            has_text_or_shapes,
            spacing
        ),
        cmd!("format.clear", "Clear All Formatting", ["Home", "Font"], Some("Cmd+Space"), "{}", has_text_or_shapes, clear),
        cmd!(
            "format.fontDialog",
            "Font…",
            ["Home", "Font"],
            Some("Cmd+T"),
            "{family?, size?: pt, bold?: bool, italic?: bool, underline?: none|sng|dbl|heavy|dotted|dash|wavy, color?, strike?: none|sng|dbl, baseline?: none|super|sub|<fraction>, caps?: none|small|all, spacing?: pt} — with none of these, opens the Font dialog",
            has_text_or_shapes,
            font_dialog
        ),
        cmd!("format.textShadow", "Text Shadow", ["Home", "Font"], None, "{on?: bool}", has_text_or_shapes, text_shadow),
        cmd!(
            "format.textOutline",
            "Text Outline",
            ["Shape Format", "WordArt Styles"],
            None,
            "{color?, width?: pt} (no color = none)",
            has_text_or_shapes,
            text_outline
        ),
        cmd!(
            "format.textWarp",
            "Text Transform",
            ["Shape Format", "WordArt Styles"],
            None,
            "{preset: a prstTxWarp name or none, adj?: [finite raw guide values, at most 8]} — none clears the transform; omitted adj uses the preset's defaults",
            has_text_or_shapes,
            text_warp
        ),
        cmd!(
            "format.align",
            "Align",
            ["Home", "Paragraph"],
            None,
            "{align: left|center|right|justify|justifyLow|distributed}",
            has_text_or_shapes,
            align
        ),
        cmd!(
            "format.rtl",
            "Right-to-Left Text Direction",
            ["Home", "Paragraph"],
            None,
            "{on?: bool} (off = left-to-right; switching mirrors left/right alignment)",
            has_text_or_shapes,
            rtl
        ),
        cmd!("format.alignLeft", "Align Left", ["Home", "Paragraph"], Some("Cmd+L"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Left)),
        cmd!("format.alignCenter", "Center", ["Home", "Paragraph"], Some("Cmd+E"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Center)),
        cmd!("format.alignRight", "Align Right", ["Home", "Paragraph"], Some("Cmd+R"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Right)),
        cmd!("format.justify", "Justify", ["Home", "Paragraph"], Some("Cmd+J"), "{}", has_text_or_shapes, |s, _| align_to(s, Align::Justify)),
        cmd!(
            "format.bullets",
            "Bullets",
            ["Home", "Paragraph"],
            None,
            "{on?: bool, char?: •|○|■|□|◆|➢|✓|–, color?, size?: % of text}",
            has_text_or_shapes,
            bullets
        ),
        cmd!(
            "format.numbering",
            "Numbering",
            ["Home", "Paragraph"],
            None,
            "{on?: bool, scheme?: arabicPeriod|arabicParenR|romanUcPeriod|romanLcPeriod|alphaUcPeriod|alphaLcParenR|alphaLcPeriod, start?: n, color?, size?: % of text}",
            has_text_or_shapes,
            numbering
        ),
        cmd!("format.indent", "Increase List Level", ["Home", "Paragraph"], Some("Tab"), "{}", has_text_or_shapes, |s, _| level(s, 1)),
        cmd!("format.outdent", "Decrease List Level", ["Home", "Paragraph"], Some("Shift+Tab"), "{}", has_text_or_shapes, |s, _| level(s, -1)),
        cmd!(
            "format.lineSpacing",
            "Line Spacing",
            ["Home", "Paragraph"],
            None,
            "{lines?: 1.0|1.5|2.0…, pt?: exactly}",
            has_text_or_shapes,
            line_spacing
        ),
        cmd!(
            "format.paragraph",
            "Paragraph…",
            ["Format"],
            None,
            "{align?, indentLeft?: pt, indentFirst?: pt (negative = hanging), spaceBefore?: pt, spaceAfter?: pt, lineSpacing?: lines}",
            has_text_or_shapes,
            paragraph
        ),
        cmd!(query "format.ruler", "Ruler", [], None, "{} → {x, y, width, rotated, marginLeft, indent, tabs: [{pos, align}], defaultTab} for the paragraph with the caret (x, y: the text area's top-left in slide pt)", has_text, ruler),
        cmd!(
            "format.paragraphDialog",
            "Paragraph…",
            ["Home", "Paragraph"],
            None,
            "{align?: left|center|right|justify|justLow|distributed, indentLeft?: pt, indentRight?: pt, special?: none|first|hanging, specialBy?: pt, indentFirst?: pt, spaceBefore?: pt, spaceAfter?: pt, spaceBeforeKind?: pts|pct, spaceAfterKind?: pts|pct, lineSpacing?: lines, lineSpacingPt?: pt} — with none of these, opens the Paragraph dialog",
            has_text_or_shapes,
            paragraph_dialog
        ),
        cmd!(
            "format.tabs",
            "Tabs…",
            ["Home", "Paragraph"],
            None,
            "{set?: [{pos, align?}] (replaces all), add?: {pos, align?: l|ctr|r|dec}, move?: {from, to}, remove?: pos, clear?: bool, defaultTab?: pt} on the selected paragraphs; with none of them, opens the Tabs dialog (pos in pt from the text area's left)",
            has_text_or_shapes,
            tabs
        ),
        cmd!("format.anchor", "Align Text", ["Home", "Paragraph"], None, "{anchor: top|middle|bottom}", has_text_or_shapes, anchor),
        cmd!(
            "format.direction",
            "Text Direction",
            ["Home", "Paragraph"],
            None,
            "{dir: horizontal|rotate90|rotate270|stacked}",
            has_text_or_shapes,
            direction
        ),
        cmd!(
            "format.columns",
            "Columns",
            ["Home", "Paragraph"],
            None,
            "{count?: 1-16, spacing?: pt between columns}; a key left out keeps the shape's value, and with neither the Columns dialog opens",
            has_text_or_shapes,
            columns
        ),
        cmd!("format.autofit", "Autofit", ["Format Shape", "Text Box"], None, "{mode: none|shrink|resize}", has_text_or_shapes, autofit),
        cmd!("format.wrap", "Wrap Text in Shape", ["Format Shape", "Text Box"], None, "{on: bool}", has_text_or_shapes, wrap),
        cmd!(
            "format.margins",
            "Text Box Margins",
            ["Format Shape", "Text Box"],
            None,
            "{left?: pt, top?: pt, right?: pt, bottom?: pt}",
            has_text_or_shapes,
            margins
        ),
        cmd!(noundo "format.painter", "Format Painter", ["Home", "Clipboard"], Some("Cmd+Shift+C"), "{sticky?: bool}", has_text_or_shapes, painter_pick),
        cmd!("format.painterApply", "Paste Formatting", [], Some("Cmd+Shift+V"), "{ids?}", has_text_or_shapes, painter_apply),
        cmd!(query "format.state", "Formatting State", [], None, "{} → the effective formatting at the selection (font, size, bold, …)", has_doc, state),
    ]
}

fn on_param(p: &Value, current: Option<bool>) -> bool {
    bool_param(p, "on").unwrap_or(!current.unwrap_or(false))
}

/// The effective run props at the selection (first selected run).
fn current_run(s: &Session) -> RunProps {
    let Ok(st) = s.doc() else { return RunProps::default() };
    let ctx = super::text::ctx_for(&st.doc, &st.selection);
    let (shape, para, run) = match &st.selection.text {
        Some(t) => {
            let Some(body) = super::text::body_of(st, t) else { return RunProps::default() };
            let a = t.ordered().0;
            let rp = if t.is_range() { ed::props_at(body, (a.0, a.1 + 1)) } else { ed::props_at(body, a) };
            let para = body.paragraphs.get(a.0).cloned().unwrap_or_default();
            (st.shape(t.shape).cloned().unwrap_or_default(), para, rp)
        }
        None => {
            let Some(sh) = st.selected_shapes().first().map(|x| (*x).clone()) else { return RunProps::default() };
            let para = sh.text.as_ref().and_then(|t| t.paragraphs.first().cloned()).unwrap_or_default();
            let rp = para.runs.first().map(|r| r.props.clone()).unwrap_or_default();
            (sh, para, rp)
        }
    };
    match ctx {
        Some(c) => resolve::run(&c, &shape, &para, &run),
        None => run,
    }
}

fn current_para(s: &Session) -> ParaProps {
    let Ok(st) = s.doc() else { return ParaProps::default() };
    let ctx = super::text::ctx_for(&st.doc, &st.selection);
    let (shape, para) = match &st.selection.text {
        Some(t) => {
            let para = super::text::body_of(st, t).and_then(|b| b.paragraphs.get(t.caret.0).cloned()).unwrap_or_default();
            (st.shape(t.shape).cloned().unwrap_or_default(), para)
        }
        None => {
            let Some(sh) = st.selected_shapes().first().map(|x| (*x).clone()) else { return ParaProps::default() };
            let para = sh.text.as_ref().and_then(|t| t.paragraphs.first().cloned()).unwrap_or_default();
            (sh, para)
        }
    };
    match ctx {
        Some(c) => resolve::para(&c, &shape, &para),
        None => para.props,
    }
}

fn bold(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, current_run(s).bold);
    format_run(s, &move |r| r.bold = Some(v))
}
fn italic(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, current_run(s).italic);
    format_run(s, &move |r| r.italic = Some(v))
}
fn underline(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = current_run(s).underline.is_some_and(|u| u != "none");
    let v = on_param(p, Some(cur));
    let style = str_param(p, "style").unwrap_or("sng").to_string();
    format_run(s, &move |r| r.underline = Some(if v { style.clone() } else { "none".into() }))
}
fn strike(s: &mut Session, p: &Value) -> Result<Value> {
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
fn superscript(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, Some(current_run(s).baseline.unwrap_or(0.0) > 0.0));
    format_run(s, &move |r| r.baseline = Some(if v { 0.3 } else { 0.0 }))
}
fn subscript(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, Some(current_run(s).baseline.unwrap_or(0.0) < 0.0));
    format_run(s, &move |r| r.baseline = Some(if v { -0.25 } else { 0.0 }))
}
fn font(s: &mut Session, p: &Value) -> Result<Value> {
    let f = str_param(p, "family").ok_or_else(|| bad("format.font", "missing `family`"))?.to_string();
    format_run(s, &move |r| r.font = Some(f.clone()))
}
fn size(s: &mut Session, p: &Value) -> Result<Value> {
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
fn grow(s: &mut Session, _p: &Value) -> Result<Value> {
    grow_shrink(s, true)
}
fn shrink(s: &mut Session, _p: &Value) -> Result<Value> {
    grow_shrink(s, false)
}
fn color(s: &mut Session, p: &Value) -> Result<Value> {
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
fn text_transparency(s: &mut Session, p: &Value) -> Result<Value> {
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
fn language(s: &mut Session, p: &Value) -> Result<Value> {
    let f = language_edit("format.language", p)?;
    format_run(s, &f)
}
/// Every run in the deck, and the text styles new text inherits from, in one language.
fn language_all(s: &mut Session, p: &Value) -> Result<Value> {
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
fn highlight(s: &mut Session, p: &Value) -> Result<Value> {
    let c = color_param(p, "color");
    format_run(s, &move |r| r.highlight = c.clone())
}
fn caps(s: &mut Session, p: &Value) -> Result<Value> {
    let c = match str_param(p, "caps").unwrap_or("none") {
        "small" => Caps::Small,
        "all" => Caps::All,
        _ => Caps::None,
    };
    format_run(s, &move |r| r.caps = Some(c))
}
fn spacing(s: &mut Session, p: &Value) -> Result<Value> {
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
    let family = s.doc().ok().and_then(|st| super::text::ctx_for(&st.doc, &st.selection).map(|c| resolve::font_family(&c, &r))).unwrap_or_default();
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

fn font_dialog(s: &mut Session, p: &Value) -> Result<Value> {
    let edits = font_edits(p)?;
    if edits.is_empty() {
        s.ui_requests.push(crate::UiRequest::Dialog { id: "font".into(), params: font_dialog_params(s) });
        return ok();
    }
    format_run(s, &|r| edits.write(r))
}
fn clear(s: &mut Session, _p: &Value) -> Result<Value> {
    format_run(s, &|r| *r = RunProps { lang: r.lang.clone(), link: r.link.clone(), ..Default::default() })
}
fn text_shadow(s: &mut Session, p: &Value) -> Result<Value> {
    let v = on_param(p, Some(current_run(s).shadow.is_some()));
    let sh = deckcraft_model::style::Shadow {
        color: deckcraft_model::ColorRef::rgb(deckcraft_color::Rgba::BLACK).with(deckcraft_color::ColorTransform::Alpha(43000)),
        blur: 3.0,
        dist: 2.0,
        dir: 45.0,
        inner: false,
        sx: 1.0,
        sy: 1.0,
        kx: 0.0,
        ky: 0.0,
        align: String::new(),
        rotate_with_shape: false,
    };
    format_run(s, &move |r| r.shadow = if v { Some(sh.clone()) } else { None })
}
fn text_outline(s: &mut Session, p: &Value) -> Result<Value> {
    let c = color_param(p, "color");
    let w = f64_or(p, "width", 0.75);
    format_run(s, &move |r| r.outline = c.clone().map(|c| Line::solid(c, w)))
}

fn text_warp(s: &mut Session, p: &Value) -> Result<Value> {
    let preset = str_param(p, "preset").ok_or_else(|| bad("format.textWarp", "missing `preset`"))?;
    let clear = preset == "none" || preset == "textNoShape";
    if !clear && !deckcraft_geom::warp::is_supported(preset) {
        return Err(bad("format.textWarp", format!("unknown text transform `{preset}`")));
    }
    let adj = match p.get("adj") {
        None => Vec::new(),
        Some(Value::Array(arr)) => {
            if arr.len() > 8 {
                return Err(bad("format.textWarp", "at most 8 adjust values"));
            }
            let mut v = Vec::with_capacity(arr.len());
            for a in arr {
                let Some(n) = a.as_f64().filter(|n| n.is_finite()) else {
                    return Err(bad("format.textWarp", "adjust values must be finite numbers"));
                };
                v.push(n.clamp(-2.0e9, 2.0e9));
            }
            v
        }
        Some(_) => return Err(bad("format.textWarp", "`adj` must be an array of numbers")),
    };
    let stored = if clear { None } else { Some(preset.to_string()) };
    format_body(s, &|b| {
        b.warp.clone_from(&stored);
        b.warp_adj.clone_from(&adj);
    })
}

fn case(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("cycle").to_string();
    let st = s.doc()?;
    if let Some(t) = st.selection.text.clone() {
        let (mut a, mut b) = t.ordered();
        let body = super::text::body_of(st, &t).cloned().unwrap_or_default();
        let collapsed = a == b;
        if collapsed {
            // Shift+F3 with a bare caret changes the word it is in and puts the caret at its start.
            let (wa, wb) = ed::word_at(&body, a);
            let is_word = ed::text_range(&body, wa, wb).chars().any(char::is_alphanumeric);
            if !(is_word && wa.1 <= a.1 && a.1 < wb.1) {
                return ok();
            }
            (a, b) = (wa, wb);
        }
        let m = if mode == "cycle" { ed::next_case(&ed::text_range(&body, a, b)).to_string() } else { mode.clone() };
        super::text::edit_text(s, move |body, t| {
            let last = body.paragraphs.len().saturating_sub(1);
            for pi in a.0..=b.0.min(last) {
                let Some(para) = body.paragraphs.get_mut(pi) else { continue };
                let n = para.char_len();
                let from = if pi == a.0 { a.1 } else { 0 };
                let to = if pi == b.0 { b.1 } else { n };
                let mut pos = 0;
                let mut pieces = Vec::new();
                for (ri, r) in para.runs.iter().enumerate() {
                    // Breaks and fields are read for context (a break ends a word) but not rewritten.
                    let len = r.char_len();
                    let rs = from.max(pos).saturating_sub(pos);
                    let re = to.min(pos + len).saturating_sub(pos);
                    if re > rs {
                        pieces.push((ri, rs, re));
                    }
                    pos += len;
                }
                let mids: Vec<String> = pieces
                    .iter()
                    .map(|&(ri, rs, re)| para.runs.get(ri).map(|r| r.content().chars().skip(rs).take(re - rs).collect()).unwrap_or_default())
                    .collect();
                let changed = ed::change_case_parts(&mids.iter().map(String::as_str).collect::<Vec<_>>(), &m);
                for (&(ri, rs, re), new) in pieces.iter().zip(changed) {
                    let Some(r) = para.runs.get_mut(ri).filter(|r| r.kind == deckcraft_model::text::RunKind::Text) else { continue };
                    let pre: String = r.text.chars().take(rs).collect();
                    let post: String = r.text.chars().skip(re).collect();
                    r.text = format!("{pre}{new}{post}");
                }
            }
            if collapsed {
                t.anchor = a;
                t.caret = a;
            }
        })?;
        return ok();
    }
    let mode = if mode == "cycle" {
        let all: String =
            st.selection.shapes.iter().filter_map(|id| st.shape(*id)).filter_map(|sh| sh.text.as_ref()).map(|t| t.text()).collect::<Vec<_>>().join(
                "
",
            );
        ed::next_case(&all).to_string()
    } else {
        mode
    };
    edit_shapes(s, &json!({}), "format.case", |sh| {
        if let Some(t) = sh.text.as_mut() {
            for para in &mut t.paragraphs {
                // Breaks and fields are read for context (a break ends a word) but not rewritten.
                let texts: Vec<&str> = para.runs.iter().map(|r| r.content()).collect();
                let changed = ed::change_case_parts(&texts, &mode);
                for (r, new) in para.runs.iter_mut().zip(changed) {
                    if r.kind == deckcraft_model::text::RunKind::Text {
                        r.text = new;
                    }
                }
            }
        }
        Ok(())
    })
}

fn align_to(s: &mut Session, a: Align) -> Result<Value> {
    format_para(s, &move |p| p.props.align = Some(a))
}
fn align(s: &mut Session, p: &Value) -> Result<Value> {
    let a = match str_param(p, "align").unwrap_or("left") {
        "center" | "centre" | "ctr" => Align::Center,
        "right" | "r" => Align::Right,
        "justify" | "just" => Align::Justify,
        "justifyLow" | "justLow" => Align::JustLow,
        "distributed" | "dist" => Align::Distributed,
        _ => Align::Left,
    };
    align_to(s, a)
}

/// Paragraph reading direction. Switching it mirrors left and right alignment, so text that sat
/// at the start edge stays there.
fn rtl(s: &mut Session, p: &Value) -> Result<Value> {
    let on = on_param(p, current_para(s).rtl);
    format_para(s, &move |para| {
        if para.props.rtl.unwrap_or(false) != on {
            para.props.align = match para.props.align {
                None | Some(Align::Left) => Some(Align::Right),
                Some(Align::Right) => Some(Align::Left),
                other => other,
            };
        }
        para.props.rtl = Some(on);
    })
}

fn bullets(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = matches!(current_para(s).bullet, Some(Bullet::Char { .. }));
    let on = on_param(p, Some(cur));
    let ch = str_param(p, "char").unwrap_or("•").to_string();
    let col = color_param(p, "color");
    let size = f64_param(p, "size").map(|v| (v / 100.0).clamp(0.25, 4.0));
    let explicit_char = p.get("char").is_some();
    format_para(s, &move |para| {
        if on {
            para.props.bullet = Some(Bullet::Char { char: ch.clone() });
            para.props.bullet_font = if explicit_char { Some("Arial".into()) } else { para.props.bullet_font.clone() };
            if col.is_some() {
                para.props.bullet_color = col.clone();
            }
            if size.is_some() {
                para.props.bullet_size = size;
            }
            if para.props.indent.is_none_or(|i| i >= 0.0) && para.props.margin_left.is_none_or(|m| m == 0.0) {
                para.props.margin_left = Some(18.0 + 36.0 * para.level as f64);
                para.props.indent = Some(-18.0);
            }
        } else {
            para.props.bullet = Some(Bullet::None);
            para.props.indent = Some(0.0);
            para.props.margin_left = Some(36.0 * para.level as f64);
        }
    })
}

fn numbering(s: &mut Session, p: &Value) -> Result<Value> {
    let cur = matches!(current_para(s).bullet, Some(Bullet::AutoNum { .. }));
    let on = on_param(p, Some(cur));
    let scheme = str_param(p, "scheme").unwrap_or("arabicPeriod").to_string();
    let start = p.get("start").and_then(Value::as_u64).map(|v| v.clamp(1, 32767) as u32).unwrap_or(1);
    let col = color_param(p, "color");
    let size = f64_param(p, "size").map(|v| (v / 100.0).clamp(0.25, 4.0));
    format_para(s, &move |para| {
        if on {
            para.props.bullet = Some(Bullet::AutoNum { scheme: scheme.clone(), start_at: start });
            if col.is_some() {
                para.props.bullet_color = col.clone();
            }
            if size.is_some() {
                para.props.bullet_size = size;
            }
            para.props.margin_left = Some(27.0 + 36.0 * para.level as f64);
            para.props.indent = Some(-27.0);
        } else {
            para.props.bullet = Some(Bullet::None);
            para.props.indent = Some(0.0);
            para.props.margin_left = Some(36.0 * para.level as f64);
        }
    })
}

fn level(s: &mut Session, d: i32) -> Result<Value> {
    format_para(s, &move |p| {
        let l = (p.level as i32 + d).clamp(0, 8) as u8;
        // Explicit indents shift with the level.
        if l != p.level {
            if let Some(m) = p.props.margin_left.as_mut() {
                *m = (*m + 36.0 * d as f64).max(0.0);
            }
            p.level = l;
        }
    })
}

fn line_spacing(s: &mut Session, p: &Value) -> Result<Value> {
    let sp =
        if let Some(pt) = f64_param(p, "pt") { Spacing::Pts(pt.clamp(0.0, 1584.0)) } else { Spacing::Pct(f64_or(p, "lines", 1.0).clamp(0.0, 9.99)) };
    format_para(s, &move |para| para.props.line_spacing = Some(sp))
}

fn paragraph(s: &mut Session, p: &Value) -> Result<Value> {
    let a = str_param(p, "align").map(|a| match a {
        "center" => Align::Center,
        "right" => Align::Right,
        "justify" => Align::Justify,
        "distributed" => Align::Distributed,
        _ => Align::Left,
    });
    let ml = f64_param(p, "indentLeft");
    let ind = f64_param(p, "indentFirst");
    let sb = f64_param(p, "spaceBefore");
    let sa = f64_param(p, "spaceAfter");
    let ls = f64_param(p, "lineSpacing");
    let lsp = f64_param(p, "lineSpacingPt");
    format_para(s, &move |para| {
        if a.is_some() {
            para.props.align = a;
        }
        if ml.is_some() {
            para.props.margin_left = ml.map(|v| v.max(0.0));
        }
        if ind.is_some() {
            para.props.indent = ind;
        }
        if let Some(v) = sb {
            para.props.space_before = Some(Spacing::Pts(v.max(0.0)));
        }
        if let Some(v) = sa {
            para.props.space_after = Some(Spacing::Pts(v.max(0.0)));
        }
        if let Some(v) = ls {
            para.props.line_spacing = Some(Spacing::Pct(v.clamp(0.0, 9.99)));
        }
        if let Some(v) = lsp {
            para.props.line_spacing = Some(Spacing::Pts(v.max(0.0)));
        }
    })
}

const PARA_DIALOG_KEYS: &[&str] =
    &["align", "indentLeft", "indentRight", "indentFirst", "special", "specialBy", "spaceBefore", "spaceAfter", "lineSpacing", "lineSpacingPt"];

fn para_dialog_has_fields(p: &Value) -> bool {
    PARA_DIALOG_KEYS.iter().any(|k| p.get(*k).is_some())
}

fn finite_key(p: &Value, cmd: &str, key: &str) -> Result<Option<f64>> {
    match p.get(key) {
        None => Ok(None),
        Some(v) => {
            let n = v.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad(cmd, format!("`{key}` must be a finite number")))?;
            Ok(Some(n))
        }
    }
}

fn clamp_pt(v: f64) -> f64 {
    v.clamp(0.0, 1584.0)
}

fn parse_align(cmd: &str, s: &str) -> Result<Align> {
    Ok(match s {
        "left" | "l" => Align::Left,
        "center" | "ctr" => Align::Center,
        "right" | "r" => Align::Right,
        "justify" | "just" => Align::Justify,
        "justLow" => Align::JustLow,
        "distributed" | "dist" => Align::Distributed,
        _ => return Err(bad(cmd, "`align` must be left, center, right, justify, justLow or distributed")),
    })
}

fn align_name(a: Align) -> &'static str {
    match a {
        Align::Left => "left",
        Align::Center => "center",
        Align::Right => "right",
        Align::Justify => "justify",
        Align::JustLow => "justLow",
        Align::Distributed => "distributed",
    }
}

fn spacing_parts(s: Option<Spacing>) -> (&'static str, f64) {
    match s {
        Some(Spacing::Pct(v)) => ("pct", v),
        Some(Spacing::Pts(v)) => ("pts", v),
        None => ("pts", 0.0),
    }
}

fn paragraph_dialog_params(s: &Session) -> Value {
    let pp = current_para(s);
    let indent = pp.indent.unwrap_or(0.0);
    let (special, by) = if indent > 0.05 {
        ("first", indent)
    } else if indent < -0.05 {
        ("hanging", -indent)
    } else {
        ("none", 0.0)
    };
    let (line_kind, line_value) = match pp.line_spacing {
        Some(Spacing::Pts(v)) => ("exactly", v),
        Some(Spacing::Pct(v)) => ("multiple", v),
        None => ("multiple", 1.0),
    };
    let (before_kind, before) = spacing_parts(pp.space_before);
    let (after_kind, after) = spacing_parts(pp.space_after);
    let tabs: Vec<Value> = pp.tabs.unwrap_or_default().iter().map(|t| json!({"pos": t.pos, "align": t.align})).collect();
    json!({
        "align": align_name(pp.align.unwrap_or(Align::Left)),
        "indentLeft": pp.margin_left.unwrap_or(0.0),
        "indentRight": pp.margin_right.unwrap_or(0.0),
        "special": special,
        "specialBy": by,
        "spaceBefore": before,
        "spaceBeforeKind": before_kind,
        "spaceAfter": after,
        "spaceAfterKind": after_kind,
        "lineKind": line_kind,
        "lineValue": line_value,
        "tabs": tabs,
        "defaultTab": pp.default_tab.unwrap_or(72.0),
    })
}

fn write_spacing(kind: Option<&str>, value: f64) -> Spacing {
    if kind == Some("pct") { Spacing::Pct(value.clamp(0.0, 10.0)) } else { Spacing::Pts(clamp_pt(value)) }
}

fn paragraph_dialog(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "format.paragraphDialog";
    if !para_dialog_has_fields(p) {
        s.ui_requests.push(crate::UiRequest::Dialog { id: "paragraph".into(), params: paragraph_dialog_params(s) });
        return ok();
    }
    let align = match p.get("align") {
        None => None,
        Some(Value::String(a)) => Some(parse_align(CMD, a)?),
        Some(_) => return Err(bad(CMD, "`align` must be a string")),
    };
    let indent_left = finite_key(p, CMD, "indentLeft")?.map(clamp_pt);
    let indent_right = finite_key(p, CMD, "indentRight")?.map(clamp_pt);
    let special_by = finite_key(p, CMD, "specialBy")?.map(|v| clamp_pt(v.max(0.0)));
    let indent = match p.get("special") {
        None => finite_key(p, CMD, "indentFirst")?.map(|v| v.clamp(-1584.0, 1584.0)),
        Some(Value::String(kind)) => {
            let by = special_by.unwrap_or(0.0);
            Some(match kind.as_str() {
                "none" => 0.0,
                "first" | "firstLine" => by,
                "hanging" | "hang" => -by,
                _ => return Err(bad(CMD, "`special` must be none, first or hanging")),
            })
        }
        Some(_) => return Err(bad(CMD, "`special` must be none, first or hanging")),
    };
    let space_before = finite_key(p, CMD, "spaceBefore")?;
    let space_after = finite_key(p, CMD, "spaceAfter")?;
    let before_kind = str_param(p, "spaceBeforeKind");
    let after_kind = str_param(p, "spaceAfterKind");
    for kind in [before_kind, after_kind].into_iter().flatten() {
        if !matches!(kind, "pts" | "pct") {
            return Err(bad(CMD, "spacing kind must be pts or pct"));
        }
    }
    let line_mult = finite_key(p, CMD, "lineSpacing")?.map(|v| v.clamp(0.0, 9.99));
    let line_pt = finite_key(p, CMD, "lineSpacingPt")?.map(clamp_pt);
    format_para(s, &move |para| {
        if align.is_some() {
            para.props.align = align;
        }
        if indent_left.is_some() {
            para.props.margin_left = indent_left;
        }
        if indent_right.is_some() {
            para.props.margin_right = indent_right;
        }
        if indent.is_some() {
            para.props.indent = indent;
        }
        if let Some(v) = space_before {
            para.props.space_before = Some(write_spacing(before_kind, v));
        }
        if let Some(v) = space_after {
            para.props.space_after = Some(write_spacing(after_kind, v));
        }
        if let Some(v) = line_mult {
            para.props.line_spacing = Some(Spacing::Pct(v));
        }
        if let Some(v) = line_pt {
            para.props.line_spacing = Some(Spacing::Pts(v));
        }
    })
}

fn tab_align(s: &str) -> Option<&'static str> {
    Some(match s {
        "l" | "left" => "l",
        "ctr" | "center" => "ctr",
        "r" | "right" => "r",
        "dec" | "decimal" => "dec",
        _ => return None,
    })
}

fn tabs_params(s: &Session) -> Value {
    let pp = current_para(s);
    let stops: Vec<Value> = pp.tabs.unwrap_or_default().iter().map(|t| json!({"pos": t.pos, "align": t.align})).collect();
    json!({"tabs": stops, "defaultTab": pp.default_tab.unwrap_or(72.0)})
}

fn anchor(s: &mut Session, p: &Value) -> Result<Value> {
    let a = match str_param(p, "anchor").unwrap_or("top") {
        "middle" | "center" | "ctr" => Anchor::Middle,
        "bottom" | "b" => Anchor::Bottom,
        _ => Anchor::Top,
    };
    format_body(s, &move |b| b.anchor = Some(a))
}
fn direction(s: &mut Session, p: &Value) -> Result<Value> {
    let d = match str_param(p, "dir").unwrap_or("horizontal") {
        "rotate90" | "vert" => TextDir::Vertical,
        "rotate270" | "vert270" => TextDir::Vertical270,
        "stacked" => TextDir::Stacked,
        _ => TextDir::Horizontal,
    };
    format_body(s, &move |b| b.vert = Some(d))
}
/// The edited or first selected shape's columns as drawn: the count and the spacing in points.
fn current_columns(s: &Session) -> (u32, f64) {
    let Ok(st) = s.doc() else { return (1, 0.0) };
    let id = match &st.selection.text {
        Some(t) if !t.notes => Some(t.shape),
        _ => st.selection.shapes.first().copied(),
    };
    let Some(sh) = id.and_then(|id| st.shape(id)) else { return (1, 0.0) };
    let body = match super::text::ctx_for(&st.doc, &st.selection) {
        Some(c) => resolve::body(&c, sh),
        None => sh.text.as_ref().map(|b| b.body.clone()).unwrap_or_default(),
    };
    (body.columns.unwrap_or(1).clamp(1, 16), body.col_spacing.unwrap_or(0.0).max(0.0))
}
fn columns(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "format.columns";
    let count = match p.get("count") {
        None => None,
        Some(v) => {
            let n = v
                .as_f64()
                .filter(|n| n.fract() == 0.0 && (1.0..=16.0).contains(n))
                .ok_or_else(|| bad(CMD, "`count` must be a whole number from 1 to 16"))?;
            Some(n as u32)
        }
    };
    let spacing = finite_key(p, CMD, "spacing")?.map(clamp_pt);
    if count.is_none() && spacing.is_none() {
        let (count, spacing) = current_columns(s);
        s.ui_requests.push(crate::UiRequest::Dialog { id: "columns".into(), params: json!({"count": count, "spacing": spacing}) });
        return ok();
    }
    format_body(s, &move |b| {
        if let Some(n) = count {
            b.columns = Some(n);
        }
        if let Some(sp) = spacing {
            b.col_spacing = Some(sp);
        }
    })
}
fn autofit(s: &mut Session, p: &Value) -> Result<Value> {
    let m = match str_param(p, "mode").unwrap_or("none") {
        "shrink" => AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 },
        "resize" | "shape" => AutoFit::Shape,
        _ => AutoFit::None,
    };
    format_body(s, &move |b| b.autofit = Some(m))
}
fn wrap(s: &mut Session, p: &Value) -> Result<Value> {
    let on = bool_or(p, "on", true);
    format_body(s, &move |b| b.wrap = Some(on))
}
fn margins(s: &mut Session, p: &Value) -> Result<Value> {
    let (l, t, r, b) = (f64_param(p, "left"), f64_param(p, "top"), f64_param(p, "right"), f64_param(p, "bottom"));
    format_body(s, &move |bp| {
        if l.is_some() {
            bp.inset_l = l.map(|v| v.max(0.0));
        }
        if t.is_some() {
            bp.inset_t = t.map(|v| v.max(0.0));
        }
        if r.is_some() {
            bp.inset_r = r.map(|v| v.max(0.0));
        }
        if b.is_some() {
            bp.inset_b = b.map(|v| v.max(0.0));
        }
    })
}

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

fn painter_pick(s: &mut Session, p: &Value) -> Result<Value> {
    let st = s.doc()?;
    let mut painted = Painted::default();
    if let Some(t) = &st.selection.text {
        if let Some(b) = super::text::body_of(st, t) {
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
    let Some(body) = super::text::body_of(st, t) else { return vec![] };
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

fn painter_apply(s: &mut Session, p: &Value) -> Result<Value> {
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
            super::text::edit_text(s, |body, _| {
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

fn state(s: &mut Session, _p: &Value) -> Result<Value> {
    let r = current_run(s);
    let pp = current_para(s);
    let st = s.doc()?;
    let family = super::text::ctx_for(&st.doc, &st.selection).map(|c| resolve::font_family(&c, &r)).unwrap_or_default();
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
fn font_add(_s: &mut Session, p: &Value) -> Result<Value> {
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
fn font_rescan(_s: &mut Session, _p: &Value) -> Result<Value> {
    let db = deckcraft_fonts::FontDb::global();
    #[cfg(not(target_arch = "wasm32"))]
    let faces = db.load_system_fonts();
    #[cfg(target_arch = "wasm32")]
    let faces = 0usize;
    Ok(json!({"faces": faces, "families": db.families().len()}))
}

/// What the horizontal ruler shows while editing text: the paragraph's indents and tab stops,
/// and where its text area sits on the slide.
fn ruler(s: &mut Session, _p: &Value) -> Result<Value> {
    let props = current_para(s);
    let st = s.doc()?;
    let t = st.selection.text.clone().ok_or_else(|| bad("format.ruler", "not editing text"))?;
    let (x, y, w, rotated) = match (t.notes, st.shape(t.shape)) {
        (false, Some(sh)) => {
            let ctx = super::text::ctx_for(&st.doc, &st.selection);
            let x = match &ctx {
                Some(c) => resolve::xfrm(c, sh),
                None => sh.xfrm.unwrap_or_default(),
            };
            let body = match &ctx {
                Some(c) => resolve::body(c, sh),
                None => sh.text.as_ref().map(|b| b.body.clone()).unwrap_or_default(),
            };
            let (l, r, tp) = (body.inset_l.unwrap_or(7.2), body.inset_r.unwrap_or(7.2), body.inset_t.unwrap_or(3.6));
            (x.x + l, x.y + tp, (x.w - l - r).max(0.0), x.rot != 0.0 || body.vert.is_some_and(|v| v != TextDir::Horizontal))
        }
        _ => (0.0, 0.0, 0.0, true),
    };
    let tabs: Vec<Value> = props.tabs.clone().unwrap_or_default().iter().map(|t| json!({"pos": t.pos, "align": t.align})).collect();
    Ok(json!({
        "x": x, "y": y, "width": w, "rotated": rotated,
        "marginLeft": props.margin_left.unwrap_or(0.0),
        "indent": props.indent.unwrap_or(0.0),
        "tabs": tabs,
        "defaultTab": props.default_tab.unwrap_or(72.0),
    }))
}

const MAX_TABS: usize = 32;

fn tabs(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "format.tabs";
    let fin = |v: Option<f64>| v.filter(|x| x.is_finite()).map(|x| x.clamp(0.0, 4000.0));
    let align_of = |v: &Value| match v.get("align").and_then(Value::as_str).unwrap_or("l") {
        "ctr" | "center" => "ctr",
        "r" | "right" => "r",
        "dec" | "decimal" => "dec",
        _ => "l",
    };
    let add = p.get("add").and_then(|a| fin(a.get("pos").and_then(Value::as_f64)).map(|pos| (pos, align_of(a).to_string())));
    let mv = p.get("move").and_then(|m| Some((fin(m.get("from").and_then(Value::as_f64))?, fin(m.get("to").and_then(Value::as_f64))?)));
    let remove = fin(p.get("remove").and_then(Value::as_f64));
    // `set` replaces every stop: entries without a usable position are dropped, an unknown
    // alignment or a non-object entry is an error; stops within 0.05 pt collapse (the last wins).
    let set = match p.get("set") {
        None => None,
        Some(Value::Array(items)) => {
            if items.len() > 64 {
                return Err(bad(CMD, "`set` has more than 64 tab stops"));
            }
            let mut out: Vec<TabStop> = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let Some(obj) = item.as_object() else {
                    return Err(bad(CMD, format!("tab stop {i} must be an object")));
                };
                let Some(pos) = obj.get("pos").and_then(Value::as_f64).filter(|n| n.is_finite()).map(clamp_pt) else {
                    continue;
                };
                let align = match obj.get("align").and_then(Value::as_str) {
                    None => "l",
                    Some(a) => tab_align(a).ok_or_else(|| bad(CMD, format!("tab stop {i} align must be l, ctr, r or dec")))?,
                };
                if let Some(prev) = out.iter_mut().find(|t| (t.pos - pos).abs() < 0.05) {
                    *prev = TabStop { pos, align: align.to_string() };
                } else {
                    out.push(TabStop { pos, align: align.to_string() });
                }
            }
            Some(out)
        }
        Some(_) => return Err(bad(CMD, "`set` must be an array")),
    };
    let clear = bool_or(p, "clear", false);
    let default_tab = finite_key(p, CMD, "defaultTab")?.map(|v| v.clamp(1.0, 1584.0));
    let edits_stops = set.is_some() || add.is_some() || mv.is_some() || remove.is_some() || clear;
    if !edits_stops && default_tab.is_none() {
        // No fields: open the Tabs dialog on the caret's paragraph.
        s.ui_requests.push(crate::UiRequest::Dialog { id: "tabs".into(), params: tabs_params(s) });
        return ok();
    }
    // The stops as the caret's paragraph shows them (inherited ones included), so editing a
    // placeholder's tab that came from its layout works like editing one set on the paragraph.
    let base = current_para(s).tabs.unwrap_or_default();
    let near = |a: f64, b: f64| (a - b).abs() < 0.5;
    let mut stops = if clear { vec![] } else { set.unwrap_or(base) };
    if let Some(at) = remove {
        stops.retain(|t| !near(t.pos, at));
    }
    if let Some((from, to)) = mv
        && let Some(t) = stops.iter_mut().find(|t| near(t.pos, from))
    {
        t.pos = to;
    }
    if let Some((pos, align)) = add {
        stops.retain(|t| !near(t.pos, pos));
        stops.push(TabStop { pos, align });
    }
    stops.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    stops.dedup_by(|a, b| near(a.pos, b.pos));
    stops.truncate(MAX_TABS);
    format_para(s, &move |para| {
        if edits_stops {
            para.props.tabs = Some(stops.clone());
        }
        if let Some(d) = default_tab {
            para.props.default_tab = Some(d);
        }
    })
}

#[cfg(test)]
#[path = "ruler_tests.rs"]
mod ruler_tests;

#[cfg(test)]
mod tests {
    use deckcraft_model::text::{Align, Caps, ParaProps, Spacing, Strike};
    use deckcraft_model::{ColorBase, Fill, SchemeSlot};
    use serde_json::json;

    use super::*;
    use crate::{Session, UiRequest};

    fn shape() -> Session {
        let mut s = Session::with_new();
        s.execute("shape.insert", &json!({"preset": "rect", "rect": [40.0, 40.0, 240.0, 80.0], "text": "Hello"})).unwrap();
        s
    }

    fn props(s: &Session) -> RunProps {
        s.doc()
            .unwrap()
            .selected_shapes()
            .first()
            .and_then(|sh| sh.text.as_ref())
            .and_then(|t| t.paragraphs.first())
            .and_then(|p| p.runs.first())
            .unwrap()
            .props
            .clone()
    }

    #[test]
    fn font_dialog_applies_character_formatting_in_one_undo() {
        let mut s = shape();
        let before = s.doc().unwrap().history.undo.len();
        s.execute(
            "format.fontDialog",
            &json!({
                "family": "Liberation Serif",
                "size": 28,
                "bold": true,
                "italic": false,
                "underline": "wavy",
                "color": "#112233",
                "strike": "dbl",
                "baseline": "super",
                "caps": "small",
                "spacing": 3
            }),
        )
        .unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Font…");
        let r = props(&s);
        assert_eq!(r.font.as_deref(), Some("Liberation Serif"));
        assert_eq!(r.size, Some(28.0));
        assert_eq!(r.bold, Some(true));
        assert_eq!(r.italic, Some(false));
        assert_eq!(r.underline.as_deref(), Some("wavy"));
        assert!(matches!(r.fill, Some(Fill::Solid { color }) if matches!(color.base, ColorBase::Rgb { .. })));
        assert_eq!(r.strike, Some(Strike::Double));
        assert_eq!(r.baseline, Some(0.3));
        assert_eq!(r.caps, Some(Caps::Small));
        assert_eq!(r.spacing, Some(3.0));
        s.execute("edit.undo", &json!({})).unwrap();
        let r = props(&s);
        assert!(r.font.is_none() && r.size.is_none() && r.bold.is_none() && r.underline.is_none() && r.fill.is_none());
        assert_eq!(r.strike, None);
        assert_eq!(r.caps, None);
    }

    #[test]
    fn font_dialog_changes_only_the_fields_it_is_given() {
        let mut s = shape();
        s.execute("format.bold", &json!({"on": true})).unwrap();
        s.execute("format.fontDialog", &json!({"size": 22, "color": {"scheme": "accent2", "lumMod": 50000}})).unwrap();
        let r = props(&s);
        assert_eq!(r.bold, Some(true));
        assert_eq!(r.size, Some(22.0));
        assert!(r.font.is_none() && r.italic.is_none());
        let Some(Fill::Solid { color }) = r.fill else { panic!("solid fill") };
        assert!(matches!(color.base, ColorBase::Scheme { slot: SchemeSlot::Accent2 }));
        assert!(color.mods.iter().any(|m| m.xml_name() == "lumMod"));
        s.execute("edit.undo", &json!({})).unwrap();
        let r = props(&s);
        assert_eq!(r.bold, Some(true));
        assert!(r.size.is_none() && r.fill.is_none());
    }

    #[test]
    fn font_dialog_with_no_fields_asks_the_ui_to_open() {
        let mut s = shape();
        s.execute("format.italic", &json!({"on": true})).unwrap();
        let rev = s.doc().unwrap().revision;
        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.fontDialog", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "font");
        assert_eq!(params["italic"], true);
        assert_eq!(params["bold"], false);
        assert!(params["family"].as_str().is_some_and(|f| !f.is_empty()));
        assert!(params["size"].as_f64().is_some_and(|n| n > 0.0));
    }

    #[test]
    fn font_dialog_rejects_bad_fields_without_editing() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        let err = s.execute("format.fontDialog", &json!({"underline": "sparkle"})).unwrap_err();
        assert!(err.to_string().contains("underline"), "{err}");
        assert!(s.execute("format.fontDialog", &json!({"size": "large"})).is_err());
        assert!(s.execute("format.fontDialog", &json!({"bold": 1})).is_err());
        assert!(s.execute("format.fontDialog", &json!({"family": "  "})).is_err());
        assert!(s.execute("format.fontDialog", &json!({"color": "not-a-color"})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        assert!(props(&s).underline.is_none());
        s.execute("edit.deselect", &json!({})).unwrap();
        assert!(s.execute("format.fontDialog", &json!({"bold": true})).is_err());
    }

    fn para(s: &Session) -> ParaProps {
        s.doc().unwrap().selected_shapes().first().and_then(|sh| sh.text.as_ref()).and_then(|t| t.paragraphs.first()).unwrap().props.clone()
    }

    #[test]
    fn paragraph_dialog_applies_alignment_indent_and_spacing_in_one_undo() {
        let mut s = shape();
        let before = s.doc().unwrap().history.undo.len();
        s.execute(
            "format.paragraphDialog",
            &json!({
                "align": "justify",
                "indentLeft": 36.0,
                "indentRight": 18.0,
                "special": "hanging",
                "specialBy": 18.0,
                "spaceBefore": 6.0,
                "spaceAfter": 8.0,
                "lineSpacing": 1.5
            }),
        )
        .unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Paragraph…");
        let p = para(&s);
        assert_eq!(p.align, Some(Align::Justify));
        assert_eq!(p.margin_left, Some(36.0));
        assert_eq!(p.margin_right, Some(18.0));
        assert_eq!(p.indent, Some(-18.0));
        assert_eq!(p.space_before, Some(Spacing::Pts(6.0)));
        assert_eq!(p.space_after, Some(Spacing::Pts(8.0)));
        assert_eq!(p.line_spacing, Some(Spacing::Pct(1.5)));
        s.execute("edit.undo", &json!({})).unwrap();
        let p = para(&s);
        assert_eq!(p.align, Some(Align::Center), "undo restores the alignment shape.insert wrote");
        assert!(p.margin_left.is_none() && p.indent.is_none() && p.space_before.is_none() && p.line_spacing.is_none());
    }

    #[test]
    fn paragraph_dialog_first_line_and_exact_spacing_replace_only_those_fields() {
        let mut s = shape();
        s.execute("format.paragraphDialog", &json!({"align": "right"})).unwrap();
        s.execute("format.paragraphDialog", &json!({"special": "first", "specialBy": 27.0, "lineSpacingPt": 20.0})).unwrap();
        let p = para(&s);
        assert_eq!(p.align, Some(Align::Right));
        assert_eq!(p.indent, Some(27.0));
        assert_eq!(p.line_spacing, Some(Spacing::Pts(20.0)));
        assert!(p.margin_left.is_none());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(para(&s).align, Some(Align::Right));
        assert!(para(&s).indent.is_none());
    }

    #[test]
    fn paragraph_dialog_with_no_fields_asks_the_ui_to_open() {
        let mut s = shape();
        let rev = s.doc().unwrap().revision;
        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.paragraphDialog", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "paragraph");
        assert_eq!(params["align"], "center", "a new shape's text is centred");
        assert_eq!(params["special"], "none");
        assert_eq!(params["lineKind"], "multiple");
        assert!(params["lineValue"].as_f64().is_some_and(|v| v > 0.0));
    }

    #[test]
    fn paragraph_dialog_rejects_bad_fields_without_editing() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        assert!(s.execute("format.paragraphDialog", &json!({"align": "sparkle"})).unwrap_err().to_string().contains("align"));
        assert!(s.execute("format.paragraphDialog", &json!({"special": "middle"})).is_err());
        assert!(s.execute("format.paragraphDialog", &json!({"indentLeft": "wide"})).is_err());
        assert!(s.execute("format.paragraphDialog", &json!({"lineSpacing": "double"})).is_err());
        assert!(s.execute("format.paragraphDialog", &json!({"spaceBeforeKind": "lines", "spaceBefore": 4.0})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        assert_eq!(para(&s).align, Some(Align::Center));
        assert!(para(&s).indent.is_none());
    }

    fn body(s: &Session) -> deckcraft_model::text::BodyProps {
        s.doc().unwrap().selected_shapes().first().and_then(|sh| sh.text.as_ref()).map(|t| t.body.clone()).unwrap_or_default()
    }

    #[test]
    fn columns_set_the_number_and_spacing_and_a_left_out_key_keeps_its_value() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        s.execute("format.columns", &json!({"count": 4, "spacing": 36.0})).unwrap();
        assert_eq!((body(&s).columns, body(&s).col_spacing), (Some(4), Some(36.0)));
        assert_eq!(s.doc().unwrap().history.undo.len(), n + 1, "one undo step");
        s.execute("format.columns", &json!({"count": 2})).unwrap();
        assert_eq!((body(&s).columns, body(&s).col_spacing), (Some(2), Some(36.0)), "the spacing stays");
        s.execute("format.columns", &json!({"spacing": 9.0})).unwrap();
        assert_eq!((body(&s).columns, body(&s).col_spacing), (Some(2), Some(9.0)), "the number stays");
    }

    #[test]
    fn columns_with_no_fields_open_the_dialog_with_the_shapes_values() {
        let mut s = shape();
        s.execute("format.columns", &json!({"count": 3, "spacing": 27.0})).unwrap();
        let rev = s.doc().unwrap().revision;
        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.columns", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().revision, rev);
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "columns");
        assert_eq!(params["count"], 3);
        assert_eq!(params["spacing"], 27.0);
    }

    #[test]
    fn columns_reject_a_bad_number_or_spacing_without_editing() {
        let mut s = shape();
        let n = s.doc().unwrap().history.undo.len();
        for p in [json!({"count": 0}), json!({"count": 17}), json!({"count": 2.5}), json!({"count": "two"}), json!({"spacing": "wide"})] {
            let err = s.execute("format.columns", &p).unwrap_err().to_string();
            assert!(err.contains("format.columns"), "{p}: {err}");
        }
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        assert_eq!(body(&s).columns, None);
    }

    #[test]
    fn tabs_dialog_sets_stops_in_one_undo_and_opens_with_no_fields() {
        let mut s = shape();
        let before = s.doc().unwrap().history.undo.len();
        s.execute(
            "format.tabs",
            &json!({"set": [{"pos": 144.0, "align": "center"}, {"pos": 72.0, "align": "dec"}, {"pos": 72.02, "align": "right"}], "defaultTab": 36.0}),
        )
        .unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), before + 1);
        assert_eq!(s.doc().unwrap().history.undo.last().unwrap().label, "Tabs…");
        let p = para(&s);
        let tabs = p.tabs.unwrap();
        assert_eq!(tabs.len(), 2, "stops within 0.05 pt collapse");
        assert_eq!(tabs[0].pos, 72.02);
        assert_eq!(tabs[0].align, "r");
        assert_eq!(tabs[1].align, "ctr");
        assert_eq!(p.default_tab, Some(36.0));
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(para(&s).tabs.is_none() && para(&s).default_tab.is_none());

        let n = s.doc().unwrap().history.undo.len();
        s.ui_requests.clear();
        s.execute("format.tabs", &json!({})).unwrap();
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
        let Some(UiRequest::Dialog { id, params }) = s.ui_requests.last() else { panic!("dialog request") };
        assert_eq!(id, "tabs");
        assert!(params["tabs"].as_array().is_some_and(|a| a.is_empty()));
        assert!(s.execute("format.tabs", &json!({"set": [{"pos": 10.0, "align": "sparkle"}]})).is_err());
        assert!(s.execute("format.tabs", &json!({"set": "nope"})).is_err());
        assert_eq!(s.doc().unwrap().history.undo.len(), n);
    }
}
