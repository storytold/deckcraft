//! Text styles for the canvas shell: Title, Body, Body 2, Body 3 and Plain text. They are the
//! deck's own inherited styles, so editing one restyles the deck the way PPTX does it:
//!
//! - Title is the slide master's title style, which title placeholders inherit; Body, Body 2 and
//!   Body 3 are the first three levels of its body style, which body and content placeholders
//!   inherit by list level; Plain text is the presentation's default text style, which text
//!   boxes inherit. Each is shown over the default text style.
//! - **Applying** a style to text whose box inherits it clears the text's own formatting
//!   (`format.clear`, and the list level for the body levels), so the text follows the style from
//!   then on. Applied anywhere else (Title on a text box) PPTX has no link to keep, so the style's
//!   values are copied onto the text.
//! - **Editing** a style (`master.textStyle`) changes it for all the text that inherits it. A
//!   theme font (`+mj-lt`, `+mn-lt`) is changed in the theme (`design.fonts`), which every style
//!   and shape in that font follows. Copies made earlier (text whose formatting still matches the
//!   style's old values) are updated to the new values too.

use deckcraft_model::text::Spacing;
use deckcraft_model::{Fill, PhType, Presentation, RunProps, Shape, ShapeId};
use serde_json::{Value, json};

use crate::SlideApp;

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub name: &'static str,
    /// `master.textStyle`'s `style`: `title`, `body` or `default`.
    pub group: &'static str,
    /// List level, 0-based.
    pub level: u8,
    pub run: RunProps,
    pub line_spacing: Option<Spacing>,
}

impl TextStyle {
    pub fn size(&self) -> f64 {
        self.run.size.unwrap_or(18.0)
    }
    pub fn bold(&self) -> bool {
        self.run.bold.unwrap_or(false)
    }
    pub fn italic(&self) -> bool {
        self.run.italic.unwrap_or(false)
    }
    /// The font as shown: a theme reference resolved to the theme's family.
    pub fn family(&self, doc: &Presentation) -> String {
        let f = self.run.font.clone().unwrap_or_default();
        match (theme_font(&f), doc.masters.first()) {
            (Some(true), Some(m)) => m.theme.fonts.major.latin.clone(),
            (Some(false), Some(m)) => m.theme.fonts.minor.latin.clone(),
            _ => f,
        }
    }
    /// The font is the theme's heading (`Some(true)`) or body (`Some(false)`) font.
    pub fn theme_font(&self) -> Option<bool> {
        self.run.font.as_deref().and_then(theme_font)
    }
}

fn theme_font(f: &str) -> Option<bool> {
    match f {
        "+mj-lt" => Some(true),
        "+mn-lt" => Some(false),
        _ => None,
    }
}

/// The deck's text styles, from its first slide master and its default text style.
pub fn styles(doc: &Presentation) -> Vec<TextStyle> {
    let Some(m) = doc.masters.first() else { return vec![] };
    let make = |name: &'static str, group: &'static str, level: u8| {
        let own = match group {
            "title" => m.title_style.level(level),
            "body" => m.body_style.level(level),
            _ => None,
        };
        let mut run = own.map(|s| s.run.clone()).unwrap_or_default();
        let mut para = own.map(|s| s.para.clone()).unwrap_or_default();
        if let Some(d) = doc.default_text_style.level(level) {
            run.inherit(&d.run);
            para.inherit(&d.para);
        }
        TextStyle { name, group, level, run, line_spacing: para.line_spacing }
    };
    vec![make("Title", "title", 0), make("Body", "body", 0), make("Body 2", "body", 1), make("Body 3", "body", 2), make("Plain text", "default", 0)]
}

/// The style group a shape's text inherits (as `resolve::master_list_style`); None for the
/// placeholders the styles here don't cover (date, footer, slide number).
pub fn group_of(sh: &Shape) -> Option<&'static str> {
    let Some(ph) = sh.ph_type() else { return Some("default") };
    match ph {
        PhType::Title | PhType::CtrTitle => Some("title"),
        PhType::Body
        | PhType::Obj
        | PhType::SubTitle
        | PhType::Table
        | PhType::Chart
        | PhType::Diagram
        | PhType::Media
        | PhType::Picture
        | PhType::ClipArt => Some("body"),
        _ => None,
    }
}

/// The commands that copy `style`'s values onto the selected text.
pub fn commands(style: &TextStyle) -> Vec<(&'static str, Value)> {
    let r = &style.run;
    let mut v = vec![];
    if let Some(f) = &r.font {
        v.push(("format.font", json!({"family": f})));
    }
    v.push(("format.size", json!({"size": style.size()})));
    v.push(("format.bold", json!({"on": style.bold()})));
    v.push(("format.italic", json!({"on": style.italic()})));
    if let Some(Fill::Solid { color }) = &r.fill {
        v.push(("format.color", json!({"color": crate::ribbon::cref_param(color)})));
    }
    match style.line_spacing {
        Some(Spacing::Pct(x)) => v.push(("format.lineSpacing", json!({"lines": x}))),
        Some(Spacing::Pts(x)) => v.push(("format.lineSpacing", json!({"pt": x}))),
        None => {}
    }
    v
}

/// The selection's text shape and the list level of its (first selected) paragraph.
fn selected_text(app: &SlideApp) -> Option<(Shape, u8)> {
    let st = app.session.active()?;
    let (id, para) = match &st.selection.text {
        Some(t) if !t.notes => (t.shape, t.caret.0),
        _ => (*st.selection.shapes.first()?, 0),
    };
    let sh = st.shape(id)?.clone();
    let level = sh.text.as_ref().and_then(|b| b.paragraphs.get(para)).map(|p| p.level).unwrap_or(0);
    Some((sh, level))
}

/// Whether the selected text's box inherits `style` (applying it links the text to it).
pub fn links(app: &SlideApp, style: &TextStyle) -> bool {
    selected_text(app).is_some_and(|(sh, _)| group_of(&sh) == Some(style.group))
}

/// Give the selected text `style`, as one undo step: linked when its box inherits the style,
/// otherwise a copy of the style's values.
pub fn apply(app: &mut SlideApp, style: &TextStyle) {
    if !links(app, style) {
        app.run_as_one(&format!("Text Style: {} (copy)", style.name), commands(style));
        return;
    }
    let mut cmds = vec![];
    if style.group == "body"
        && let Some((_, level)) = selected_text(app)
    {
        let (id, n) = if style.level > level { ("format.indent", style.level - level) } else { ("format.outdent", level - style.level) };
        cmds.extend((0..n).map(|_| (id, json!({}))));
    }
    cmds.push(("format.clear", json!({})));
    app.run_as_one(&format!("Text Style: {}", style.name), cmds);
}

/// What the style menu shows for the selection: the style it follows ("Title"), a style its
/// values were copied from ("Title (copy)"), or None ("Custom").
pub fn current(app: &SlideApp, styles: &[TextStyle], state: &Value) -> Option<String> {
    let font = state.get("fontRef").and_then(Value::as_str);
    let size = state.get("size").and_then(Value::as_f64)?;
    let bold = state.get("bold").and_then(Value::as_bool).unwrap_or(false);
    let same_font = |s: &TextStyle| s.run.font.is_none() || s.run.font.as_deref() == font;
    if let Some((sh, level)) = selected_text(app)
        && let Some(group) = group_of(&sh)
        && let Some(s) = styles.iter().find(|s| s.group == group && (group != "body" || s.level == level.min(2)))
        // A layout may enlarge its placeholder (a title slide's title): the font and weight say
        // whether the text still follows the style.
        && same_font(s)
        && s.bold() == bold
    {
        return Some(s.name.to_string());
    }
    styles.iter().find(|s| same_font(s) && (s.size() - size).abs() < 0.01 && s.bold() == bold).map(|s| format!("{} (copy)", s.name))
}

// ----- Editing a style ----------------------------------------------------------------------

/// One property of a style being edited.
#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    Font(String),
    Size(f64),
    Bold(bool),
    Italic(bool),
    Color(deckcraft_model::ColorRef),
    LineSpacing(f64),
}

/// The text (slide, shape) that copies `style`: text the style doesn't reach by inheritance whose
/// every run still has the style's font, size and weight.
pub fn copies(doc: &Presentation, style: &TextStyle) -> Vec<(usize, ShapeId)> {
    let mut out = vec![];
    for (i, slide) in doc.slides.iter().enumerate() {
        for sh in &slide.shapes {
            if group_of(sh) == Some(style.group) {
                continue;
            }
            let Some(body) = &sh.text else { continue };
            let runs: Vec<&RunProps> = body.paragraphs.iter().flat_map(|p| p.runs.iter().filter(|r| !r.text.is_empty()).map(|r| &r.props)).collect();
            let copy = !runs.is_empty()
                && runs.iter().all(|r| {
                    r.font.is_some()
                        && r.font == style.run.font
                        && r.size.is_some_and(|v| (v - style.size()).abs() < 0.01)
                        && r.bold.unwrap_or(false) == style.bold()
                });
            if copy {
                out.push((i, sh.id));
            }
        }
    }
    out
}

/// The commands that make `edit` to `style` everywhere, as one undo step: the style itself (or the
/// theme font it uses), then the copies of it, then back to the slide and selection the user had.
pub fn edit_commands(app: &SlideApp, style: &TextStyle, edit: &Edit) -> Vec<(&'static str, Value)> {
    let Some(st) = app.session.active() else { return vec![] };
    let doc = &st.doc;
    let mut v: Vec<(&'static str, Value)> = vec![];
    let base = json!({"style": style.group, "level": style.level + 1});
    let with = |k: &str, val: Value, layouts: bool| {
        let mut p = base.clone();
        p[k] = val;
        p["layouts"] = json!(layouts);
        p
    };
    match edit {
        Edit::Font(f) => match (style.theme_font(), doc.masters.first()) {
            (Some(major), Some(m)) => {
                let (mj, mn) = if major { (f.clone(), m.theme.fonts.minor.latin.clone()) } else { (m.theme.fonts.major.latin.clone(), f.clone()) };
                v.push(("design.fonts", json!({"major": mj, "minor": mn})));
            }
            _ => v.push(("master.textStyle", with("font", json!(f), true))),
        },
        // A size or line height a layout sets on purpose (a title slide's big title) is kept.
        Edit::Size(s) => v.push(("master.textStyle", with("size", json!(s), false))),
        Edit::Bold(b) => v.push(("master.textStyle", with("bold", json!(b), true))),
        Edit::Italic(b) => v.push(("master.textStyle", with("italic", json!(b), true))),
        Edit::Color(c) => v.push(("master.textStyle", with("color", crate::ribbon::cref_param(c), true))),
        Edit::LineSpacing(l) => v.push(("master.textStyle", with("lineSpacing", json!({"lines": l}), false))),
    }
    // Copies: select each and format it the same way.
    let fmt: (&'static str, Value) = match edit {
        Edit::Font(f) => {
            ("format.font", json!({"family": if style.theme_font().is_some() { style.run.font.clone().unwrap_or_default() } else { f.clone() }}))
        }
        Edit::Size(s) => ("format.size", json!({"size": s})),
        Edit::Bold(b) => ("format.bold", json!({"on": b})),
        Edit::Italic(b) => ("format.italic", json!({"on": b})),
        Edit::Color(c) => ("format.color", json!({"color": crate::ribbon::cref_param(c)})),
        Edit::LineSpacing(l) => ("format.lineSpacing", json!({"lines": l})),
    };
    let copies = copies(doc, style);
    // A theme font copy keeps its reference, which already follows the theme.
    let font_ref_only = matches!(edit, Edit::Font(_)) && style.theme_font().is_some();
    if !copies.is_empty() && !font_ref_only {
        let (slide, shapes) = (st.selection.slide, st.selection.shapes.iter().map(|i| i.0).collect::<Vec<_>>());
        for (i, id) in copies {
            v.push(("slide.go", json!({"index": i})));
            v.push(("selection.select", json!({"ids": [id.0]})));
            v.push(fmt.clone());
        }
        v.push(("slide.go", json!({"index": slide})));
        if shapes.is_empty() {
            v.push(("edit.deselect", json!({})));
        } else {
            v.push(("selection.select", json!({"ids": shapes})));
        }
    }
    v
}

/// Make `edit` to `style` everywhere (see [`edit_commands`]).
pub fn edit_style(app: &mut SlideApp, style: &TextStyle, edit: &Edit) {
    let cmds = edit_commands(app, style, edit);
    app.run_as_one(&format!("Edit Text Style: {}", style.name), cmds);
}

#[cfg(test)]
#[path = "textstyles_tests.rs"]
mod tests;
