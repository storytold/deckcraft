//! The slide master's text styles (`p:txStyles`): the title style, the body style's levels and
//! the other style, which every placeholder of that kind inherits; and the presentation's default
//! text style (`p:defaultTextStyle`), which text boxes and shapes inherit. Editing one restyles
//! all the text that hasn't been given its own formatting, as editing the master does in
//! PowerPoint.

use std::sync::Arc;

use deckcraft_model::text::{ListStyle, ParaProps, Spacing};
use deckcraft_model::{Fill, PhType, Presentation, RunProps, Shape};
use serde_json::{Value, json};

use super::*;
use crate::Target;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(
            "master.textStyle",
            "Edit Text Style",
            ["Slide Master", "Edit Master"],
            None,
            "{style: title|body|other|default (text boxes and shapes), level?: 1..9 (default 1), master?, font?: family|+mj-lt|+mn-lt, size?: pt, bold?, italic?, color?, lineSpacing?: {lines}|{pt}, layouts?: bool — also clear these properties where the layouts and the master's placeholders override them}",
            has_doc,
            text_style
        ),
        cmd!(
            query "master.textStyles",
            "Text Styles",
            [],
            None,
            "{master?} → {title: [level…], body: […], other: […], default: […]}: each level's own run and paragraph properties",
            has_doc,
            text_styles
        ),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Title,
    Body,
    Other,
    /// The presentation's default text style.
    Default,
}

impl Which {
    fn parse(s: &str) -> Option<Which> {
        match s {
            "title" => Some(Which::Title),
            "body" => Some(Which::Body),
            "other" => Some(Which::Other),
            "default" => Some(Which::Default),
            _ => None,
        }
    }
    /// The style a shape's text inherits (as `resolve::master_list_style`).
    fn of(sh: &Shape) -> Which {
        let Some(ph) = sh.ph_type() else { return Which::Default };
        match ph {
            PhType::Title | PhType::CtrTitle => Which::Title,
            PhType::Body
            | PhType::Obj
            | PhType::SubTitle
            | PhType::Table
            | PhType::Chart
            | PhType::Diagram
            | PhType::Media
            | PhType::Picture
            | PhType::ClipArt => Which::Body,
            _ => Which::Other,
        }
    }
}

fn master_of_current(s: &Session) -> Result<usize> {
    let st = s.doc()?;
    Ok(match st.selection.target {
        Target::Master { master } | Target::Layout { master, .. } => master,
        Target::Slides | Target::Handout | Target::NotesMaster => {
            st.doc.slides.get(st.selection.slide).and_then(|x| st.doc.masters.iter().position(|m| m.layout(x.layout).is_some())).unwrap_or(0)
        }
    })
}

/// The properties a `master.textStyle` call sets.
#[derive(Clone, Default)]
struct Change {
    font: Option<String>,
    size: Option<f64>,
    bold: Option<bool>,
    italic: Option<bool>,
    color: Option<deckcraft_model::ColorRef>,
    line_spacing: Option<Spacing>,
}

impl Change {
    fn parse(p: &Value) -> Result<Change> {
        const CMD: &str = "master.textStyle";
        let line_spacing = match p.get("lineSpacing") {
            None | Some(Value::Null) => None,
            Some(v) => match (f64_param(v, "lines"), f64_param(v, "pt")) {
                (Some(l), _) if l > 0.0 => Some(Spacing::Pct(l)),
                (_, Some(pt)) if pt > 0.0 => Some(Spacing::Pts(pt)),
                _ => return Err(bad(CMD, "`lineSpacing` takes {lines} or {pt}, above zero")),
            },
        };
        let color = match p.get("color") {
            None | Some(Value::Null) => None,
            Some(_) => Some(color_param(p, "color").ok_or_else(|| bad(CMD, "invalid `color`"))?),
        };
        let c = Change {
            font: str_param(p, "font").map(str::to_string),
            size: f64_param(p, "size").map(|v| v.clamp(1.0, 4000.0)),
            bold: bool_param(p, "bold"),
            italic: bool_param(p, "italic"),
            color,
            line_spacing,
        };
        if c.font.is_none() && c.size.is_none() && c.bold.is_none() && c.italic.is_none() && c.color.is_none() && c.line_spacing.is_none() {
            return Err(bad(CMD, "give `font`, `size`, `bold`, `italic`, `color` or `lineSpacing`"));
        }
        Ok(c)
    }
    fn set(&self, run: &mut RunProps, para: &mut ParaProps) {
        if let Some(f) = &self.font {
            run.font = Some(f.clone());
        }
        if self.size.is_some() {
            run.size = self.size;
        }
        if self.bold.is_some() {
            run.bold = self.bold;
        }
        if self.italic.is_some() {
            run.italic = self.italic;
        }
        if let Some(c) = &self.color {
            run.fill = Some(Fill::Solid { color: c.clone() });
        }
        if self.line_spacing.is_some() {
            para.line_spacing = self.line_spacing;
        }
    }
    /// Remove an override of the properties this change sets, so the style shows through.
    fn clear(&self, run: &mut RunProps, para: &mut ParaProps) {
        if self.font.is_some() {
            run.font = None;
        }
        if self.size.is_some() {
            run.size = None;
        }
        if self.bold.is_some() {
            run.bold = None;
        }
        if self.italic.is_some() {
            run.italic = None;
        }
        if self.color.is_some() {
            run.fill = None;
        }
        if self.line_spacing.is_some() {
            para.line_spacing = None;
        }
    }
}

/// Clear `change`'s properties at `level` (0-based) of the placeholders in `shapes` that inherit
/// `which`: their list style and the text in them.
fn clear_overrides(shapes: &mut [Shape], which: Which, level: u8, change: &Change) {
    for sh in shapes.iter_mut().filter(|sh| sh.ph.is_some() && Which::of(sh) == which) {
        let Some(body) = sh.text.as_mut() else { continue };
        if let Some(Some(ls)) = body.list_style.levels.get_mut(level as usize) {
            change.clear(&mut ls.run, &mut ls.para);
        }
        for para in body.paragraphs.iter_mut().filter(|p| p.level == level) {
            change.clear(&mut RunProps::default(), &mut para.props);
            for run in &mut para.runs {
                change.clear(&mut run.props, &mut ParaProps::default());
            }
            change.clear(&mut para.end_props, &mut ParaProps::default());
        }
    }
}

fn text_style(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "master.textStyle";
    let which = str_param(p, "style").and_then(Which::parse).ok_or_else(|| bad(CMD, "`style` is title, body or other"))?;
    let level = usize_param(p, "level").unwrap_or(1);
    if !(1..=9).contains(&level) {
        return Err(bad(CMD, "`level` is 1 to 9"));
    }
    let level = (level - 1) as u8;
    let change = Change::parse(p)?;
    let mi = match usize_param(p, "master") {
        Some(m) => m,
        None => master_of_current(s)?,
    };
    let layouts = bool_param(p, "layouts").unwrap_or(false);
    s.edit(|doc, _| {
        if which == Which::Default {
            let mut ls = doc.default_text_style.level(level).cloned().unwrap_or_default();
            change.set(&mut ls.run, &mut ls.para);
            doc.default_text_style.set(level, ls);
            return Ok(());
        }
        let m = Arc::make_mut(doc.masters.get_mut(mi).ok_or_else(|| bad(CMD, "no such master"))?);
        let list = match which {
            Which::Title => &mut m.title_style,
            Which::Body => &mut m.body_style,
            Which::Other | Which::Default => &mut m.other_style,
        };
        let mut ls = list.level(level).cloned().unwrap_or_default();
        change.set(&mut ls.run, &mut ls.para);
        list.set(level, ls);
        if layouts {
            clear_overrides(&mut m.shapes, which, level, &change);
            for l in &mut m.layouts {
                clear_overrides(&mut l.shapes, which, level, &change);
            }
        }
        Ok(())
    })?;
    ok()
}

fn levels(list: &ListStyle) -> Value {
    Value::Array(
        (0..9u8)
            .map(|l| match list.level(l) {
                Some(ls) => json!({"run": ls.run, "para": ls.para}),
                None => Value::Null,
            })
            .collect(),
    )
}

fn text_styles(s: &mut Session, p: &Value) -> Result<Value> {
    let mi = match usize_param(p, "master") {
        Some(m) => m,
        None => master_of_current(s)?,
    };
    let doc: &Presentation = &s.doc()?.doc;
    let m = doc.masters.get(mi).ok_or_else(|| bad("master.textStyles", "no such master"))?;
    Ok(json!({
        "title": levels(&m.title_style),
        "body": levels(&m.body_style),
        "other": levels(&m.other_style),
        "default": levels(&doc.default_text_style),
    }))
}

#[cfg(test)]
#[path = "textstyle_tests.rs"]
mod tests;
