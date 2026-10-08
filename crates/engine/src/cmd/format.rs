//! Home ▸ Font and Paragraph, Format menu: character, paragraph and text box formatting, and
//! the Format Painter.

use deckcraft_model::edit as ed;
use deckcraft_model::resolve;
use deckcraft_model::text::{Align, Anchor, AutoFit, Bullet, Caps, ParaProps, RunProps, Spacing, Strike, TextDir};
use deckcraft_model::{Effects, Fill, Line, Shape};
use serde_json::{Value, json};

use super::text::{format_body, format_para, format_run};
use super::*;
use crate::{Result, Session};

pub fn specs() -> Vec<CommandSpec> {
    vec![
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
        cmd!(
            "format.color",
            "Font Color",
            ["Home", "Font"],
            None,
            "{color: #RRGGBB | accent1..6 | tx1… | {scheme, lumMod…}}",
            has_text_or_shapes,
            color
        ),
        cmd!("format.highlight", "Text Highlight Color", ["Home", "Font"], None, "{color? (absent = none)}", has_text_or_shapes, highlight),
        cmd!("format.case", "Change Case", ["Home", "Font"], Some("Shift+F3"), "{mode: sentence|lower|upper|title|toggle}", has_text_or_shapes, case),
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
            "{on?: bool, scheme?: arabicPeriod|arabicParenR|romanUcPeriod|romanLcPeriod|alphaUcPeriod|alphaLcParenR|alphaLcPeriod, start?: n}",
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
        cmd!("format.columns", "Columns", ["Home", "Paragraph"], None, "{count, spacing?: pt}", has_text_or_shapes, columns),
        cmd!("format.autofit", "Autofit", ["Format Shape", "Text Box"], None, "{mode: none|shrink|resize}", has_text_or_shapes, autofit),
        cmd!("format.wrap", "Wrap Text in Shape", ["Format Shape", "Text Box"], None, "{on: bool}", has_text_or_shapes, wrap),
        cmd!(
            "format.margins",
            "Text Box Margins",
            ["Format Shape", "Text Box"],
            None,
            "{left?, top?, right?, bottom?: pt}",
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
    format_run(s, &move |r| r.fill = Some(Fill::solid(c.clone())))
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

fn case(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = str_param(p, "mode").unwrap_or("sentence").to_string();
    let st = s.doc()?;
    if let Some(t) = st.selection.text.clone() {
        let (a, b) = t.ordered();
        if a == b {
            return ok();
        }
        let m = mode.clone();
        super::text::edit_text(s, move |body, _| {
            for pi in a.0..=b.0 {
                let Some(para) = body.paragraphs.get_mut(pi) else { continue };
                let n = para.char_len();
                let from = if pi == a.0 { a.1 } else { 0 };
                let to = if pi == b.0 { b.1 } else { n };
                let mut pos = 0;
                for r in &mut para.runs {
                    let len = r.char_len();
                    if r.kind == deckcraft_model::text::RunKind::Text {
                        let rs = from.max(pos).saturating_sub(pos);
                        let re = to.min(pos + len).saturating_sub(pos);
                        if re > rs {
                            let chars: Vec<char> = r.text.chars().collect();
                            let mid: String = chars.get(rs..re).map(|c| c.iter().collect()).unwrap_or_default();
                            let pre: String = chars.get(..rs).map(|c| c.iter().collect()).unwrap_or_default();
                            let post: String = chars.get(re..).map(|c| c.iter().collect()).unwrap_or_default();
                            r.text = format!("{pre}{}{post}", ed::change_case(&mid, &m));
                        }
                    }
                    pos += len;
                }
            }
        })?;
        return ok();
    }
    edit_shapes(s, &json!({}), "format.case", |sh| {
        if let Some(t) = sh.text.as_mut() {
            for para in &mut t.paragraphs {
                for r in &mut para.runs {
                    if r.kind == deckcraft_model::text::RunKind::Text {
                        r.text = ed::change_case(&r.text, &mode);
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
    let size = f64_param(p, "size").map(|v| v / 100.0);
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
    format_para(s, &move |para| {
        if on {
            para.props.bullet = Some(Bullet::AutoNum { scheme: scheme.clone(), start_at: start });
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
fn columns(s: &mut Session, p: &Value) -> Result<Value> {
    let n = p.get("count").and_then(Value::as_u64).unwrap_or(1).clamp(1, 16) as u32;
    let sp = f64_or(p, "spacing", 0.0).max(0.0);
    format_body(s, &move |b| {
        b.columns = Some(n);
        b.col_spacing = Some(sp);
    })
}
fn autofit(s: &mut Session, p: &Value) -> Result<Value> {
    let m = match str_param(p, "mode").unwrap_or("none") {
        "shrink" => AutoFit::Shrink { font_scale: 1.0, line_reduction: 0.0 },
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

fn painter_apply(s: &mut Session, p: &Value) -> Result<Value> {
    let Some((painted, sticky)) = s.painter.clone() else { return Err(bad("format.painterApply", "pick formatting with the Format Painter first")) };
    if !sticky {
        s.painter = None;
    }
    if s.doc()?.selection.text.as_ref().is_some_and(|t| t.is_range()) && ids_param(p, "ids").is_none() {
        if let Some(r) = painted.run.clone() {
            format_run(s, &move |rp| *rp = r.clone())?;
        }
        if let Some(pp) = painted.para.clone() {
            format_para(s, &move |para| para.props = pp.clone())?;
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
        "lineSpacing": serde_json::to_value(pp.line_spacing).unwrap_or_default(),
        "painter": s.painter.is_some(),
    }))
}
