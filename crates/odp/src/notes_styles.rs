//! Bounded standard style chains for notes-master defaults and local overrides.
use std::collections::BTreeMap;

use deckcraft_model::{
    Paragraph, Shape, TextBody,
    text::{BodyProps, ParaProps, RunProps},
};

use crate::{
    OdpError, Result,
    xml::{self, El, Node},
};

pub(crate) fn document(fragment: &str) -> Result<El> {
    let source = format!("{}{fragment}</office:document-content>", xml::header("office:document-content"));
    xml::parse(source.as_bytes())
}

fn serialize(el: &El, out: &mut String) {
    out.push('<');
    out.push_str(&el.name);
    for (key, value) in &el.attrs {
        out.push_str(&format!(" {key}=\"{}\"", xml::esc(value)));
    }
    if el.children.is_empty() && !el.name.starts_with("draw:") {
        out.push_str("/>");
        return;
    }
    out.push('>');
    for child in &el.children {
        match child {
            Node::El(child) => serialize(child, out),
            Node::Text(value) => out.push_str(&xml::esc(value)),
        }
    }
    out.push_str(&format!("</{}>", el.name));
}
pub(crate) fn fragment(el: &El) -> String {
    let mut out = String::new();
    serialize(el, &mut out);
    out
}
/// Conservative escaped size, computed before serializing untrusted style trees.
pub(crate) fn serialized_size(el: &El) -> usize {
    el.attrs
        .iter()
        .fold(el.name.len().saturating_mul(2).saturating_add(5), |size, (key, value)| {
            size.saturating_add(key.len()).saturating_add(value.len().saturating_mul(6)).saturating_add(4)
        })
        .saturating_add(el.children.iter().fold(0usize, |size, node| {
            size.saturating_add(match node {
                Node::El(child) => serialized_size(child),
                Node::Text(value) => value.len().saturating_mul(6),
            })
        }))
}
fn contents(el: &El) -> String {
    let mut out = String::new();
    for child in el.elements() {
        serialize(child, &mut out);
    }
    out
}

fn index(root: &El) -> BTreeMap<String, usize> {
    root.children
        .iter()
        .enumerate()
        .filter_map(|(i, node)| match node {
            Node::El(el) => el.attr("style:name").map(|name| (name.to_string(), i)),
            _ => None,
        })
        .collect()
}
fn style<'a>(root: &'a El, index: &BTreeMap<String, usize>, name: &str) -> Result<&'a El> {
    match index.get(name).and_then(|i| root.children.get(*i)) {
        Some(Node::El(el)) => Ok(el),
        _ => Err(OdpError("missing generated notes style".into())),
    }
}
fn style_mut<'a>(root: &'a mut El, index: &BTreeMap<String, usize>, name: &str) -> Result<&'a mut El> {
    match index.get(name).and_then(|i| root.children.get_mut(*i)) {
        Some(Node::El(el)) => Ok(el),
        _ => Err(OdpError("missing generated notes style".into())),
    }
}
fn find_mut<'a>(root: &'a mut El, identity: &str) -> Option<&'a mut El> {
    if root.attr("xml:id") == Some(identity) {
        return Some(root);
    }
    for node in &mut root.children {
        if let Node::El(child) = node
            && let Some(found) = find_mut(child, identity)
        {
            return Some(found);
        }
    }
    None
}
fn graphic_key(key: &str, source: &Shape, body: &BodyProps) -> bool {
    match key {
        "fo:padding-left" => body.inset_l.is_some(),
        "fo:padding-right" => body.inset_r.is_some(),
        "fo:padding-top" => body.inset_t.is_some(),
        "fo:padding-bottom" => body.inset_b.is_some(),
        "fo:wrap-option" => body.wrap.is_some(),
        "draw:textarea-vertical-align" => body.anchor.is_some(),
        "draw:textarea-horizontal-align" => body.anchor_ctr.is_some(),
        "draw:auto-grow-height" | "style:shrink-to-fit" => body.autofit.is_some(),
        "style:writing-mode" => body.vert.is_some() || body.rtl_col.is_some(),
        key if key.starts_with("draw:fill") || key == "draw:opacity" => source.fill.is_some() || source.style.is_some(),
        key if key.starts_with("draw:stroke") || key.starts_with("svg:stroke") || key.starts_with("draw:marker") => {
            source.line.is_some() || source.style.is_some()
        }
        key if key.starts_with("draw:shadow") => source.effects.is_some() || source.style.is_some(),
        _ => true,
    }
}
fn paragraph_key(key: &str, source: &ParaProps) -> bool {
    match key {
        "fo:text-align" => source.align.is_some(),
        "fo:background-color" => source.background.is_some(),
        "fo:margin-left" => source.margin_left.is_some(),
        "fo:margin-right" => source.margin_right.is_some(),
        "fo:text-indent" => source.indent.is_some(),
        "fo:margin-top" => source.space_before.is_some(),
        "fo:margin-bottom" => source.space_after.is_some(),
        "fo:line-height" => source.line_spacing.is_some() || matches!(source.line_height, Some(deckcraft_model::text::LineHeight::Normal)),
        "style:line-height-at-least" => matches!(source.line_height, Some(deckcraft_model::text::LineHeight::Minimum { .. })),
        "style:writing-mode" => source.rtl.is_some(),
        "style:tab-stop-distance" => source.default_tab.is_some(),
        "style:line-break" => source.latin_line_break.is_some() || source.east_asian_line_break.is_some(),
        "style:punctuation-wrap" => source.hanging_punct.is_some(),
        _ => true,
    }
}
fn run_key(key: &str, source: &RunProps) -> bool {
    match key {
        "fo:font-family" | "style:font-name" => source.font.is_some(),
        "style:font-family-asian" | "style:font-name-asian" => source.font_ea.is_some(),
        "style:font-family-complex" | "style:font-name-complex" => source.font_cs.is_some(),
        "fo:font-size" => source.size.is_some(),
        "style:font-size-asian" => source.size_ea.is_some(),
        "style:font-size-complex" => source.size_cs.is_some(),
        "style:font-weight-asian" => source.bold_ea.is_some(),
        "style:font-weight-complex" => source.bold_cs.is_some(),
        "style:font-style-asian" => source.italic_ea.is_some(),
        "style:font-style-complex" => source.italic_cs.is_some(),
        "style:language-asian" | "style:country-asian" => source.lang_ea.is_some(),
        "style:language-complex" | "style:country-complex" => source.lang_cs.is_some(),
        "fo:font-weight" => source.bold.is_some(),
        "fo:font-style" => source.italic.is_some(),
        "fo:color" => source.fill.is_some(),
        "fo:background-color" => source.highlight.is_some(),
        "style:text-underline-color" => source.underline_color.is_some(),
        key if key.starts_with("style:text-underline-") => source.underline.is_some(),
        key if key.starts_with("style:text-line-through-") => source.strike.is_some(),
        "style:text-position" => source.baseline.is_some(),
        "fo:letter-spacing" => source.spacing.is_some(),
        "style:letter-kerning" => source.kern.is_some(),
        "fo:text-transform" | "fo:font-variant" => source.caps.is_some(),
        "fo:language" | "fo:country" => source.lang.is_some(),
        _ => true,
    }
}
fn filter_style(style: &mut El, bag: &str, mut keep: impl FnMut(&str) -> bool, keep_children: bool) {
    for node in &mut style.children {
        if let Node::El(child) = node
            && child.name == bag
        {
            child.attrs.retain(|key, _| keep(key));
            if !keep_children {
                child.children.clear();
            }
        }
    }
}
fn spans<'a>(el: &'a mut El, out: &mut Vec<&'a mut El>) {
    for node in &mut el.children {
        if let Node::El(child) = node {
            if child.name == "text:span" {
                out.push(child);
            } else {
                spans(child, out);
            }
        }
    }
}
fn paragraphs<'a>(el: &'a mut El, out: &mut Vec<&'a mut El>) {
    for node in &mut el.children {
        if let Node::El(child) = node {
            if child.name == "text:p" {
                out.push(child);
            } else {
                paragraphs(child, out);
            }
        }
    }
}
fn remove_paragraphs(el: &mut El) {
    el.children.retain(|node| !matches!(node,Node::El(child) if child.name=="text:p"));
    for node in &mut el.children {
        if let Node::El(child) = node {
            remove_paragraphs(child);
        }
    }
}

pub(crate) struct RoleSource<'a> {
    pub identity: String,
    pub source: &'a Shape,
    pub body: Option<&'a TextBody>,
    pub shared: String,
    pub paragraph: String,
}
pub(crate) struct Rewritten {
    pub styles: String,
    pub drawing: String,
    pub definitions: String,
}

/// The source options, rather than equality to resolved defaults, determine each
/// local override. Generated styles are indexed once within this top-level shape.
pub(crate) fn rewrite(styles: &str, drawing: &str, roles: &[RoleSource<'_>], master: bool) -> Result<Rewritten> {
    let mut styles = document(styles)?;
    let lookup = index(&styles);
    let mut drawing = document(drawing)?;
    let mut definitions = String::new();
    for role in roles {
        let frame = find_mut(&mut drawing, &role.identity).ok_or_else(|| OdpError("missing generated notes role".into()))?;
        let graphic = frame.attr("draw:style-name").ok_or_else(|| OdpError("missing generated notes graphic style".into()))?.to_string();
        let shared_role = !role.shared.is_empty();
        if master && shared_role {
            let shared = style(&styles, &lookup, &graphic)?;
            definitions.push_str(&format!(
                "<style:style style:name=\"{}\" style:family=\"presentation\">{}</style:style>",
                role.shared,
                contents(shared)
            ));
            frame.attrs.remove("draw:style-name");
            frame.attrs.insert("presentation:style-name".into(), role.shared.clone());
        } else if shared_role {
            let body = role.body.map(|body| &body.body).cloned().unwrap_or_default();
            if body.vert.is_some() != body.rtl_col.is_some()
                || body.columns.is_some() != body.col_spacing.is_some()
                || body.rot.is_some()
                || body.upright.is_some()
                || body.space_first_last.is_some()
            {
                return Err(OdpError(
                    "independent notes text-direction, column, rotation or auxiliary body overrides require additional ODF ownership mapping".into(),
                ));
            }
            filter_style(
                style_mut(&mut styles, &lookup, &graphic)?,
                "style:graphic-properties",
                |key| graphic_key(key, role.source, &body),
                body.columns.is_some() || body.col_spacing.is_some(),
            );
            if role.source.effects.is_some()
                && let Some(graphic) = style_mut(&mut styles, &lookup, &graphic)?.children.iter_mut().find_map(|node| match node {
                    Node::El(el) if el.name == "style:graphic-properties" => Some(el),
                    _ => None,
                })
                && !graphic.attrs.contains_key("draw:shadow")
            {
                graphic.attrs.insert("draw:shadow".into(), "hidden".into());
            }
            let automatic = style_mut(&mut styles, &lookup, &graphic)?;
            automatic.attrs.insert("style:family".into(), "presentation".into());
            automatic.attrs.insert("style:parent-style-name".into(), role.shared.clone());
            frame.attrs.remove("draw:style-name");
            frame.attrs.insert("presentation:style-name".into(), graphic.clone());
        }
        let mut paras = Vec::new();
        paragraphs(frame, &mut paras);
        for (i, para) in paras.into_iter().enumerate() {
            let fallback = Paragraph::default();
            let source = role
                .body
                .and_then(|body| body.paragraphs.get(i))
                .or(master.then_some(&fallback))
                .ok_or_else(|| OdpError("notes paragraph count changed during serialization".into()))?;
            let para_name = para.attr("text:style-name").ok_or_else(|| OdpError("missing generated notes paragraph style".into()))?.to_string();
            // Shared notes publish owned defaults from the dedicated probe below.
            // Unshared roles keep the generic writer's complete typing defaults.
            if shared_role {
                style_mut(&mut styles, &lookup, &para_name)?
                    .children
                    .retain(|node| !matches!(node, Node::El(element) if element.name == "style:text-properties"));
            }
            // normalize inserts one empty run solely to serialize defRPr. Remove it
            // before publication; it never changes paragraph or run identifiers.
            let first = para
                .children
                .iter()
                .position(|node| matches!(node,Node::El(el) if el.name=="text:span"))
                .ok_or_else(|| OdpError("missing notes default-style probe".into()))?;
            let probe = match para.children.remove(first) {
                Node::El(probe) => probe,
                _ => return Err(OdpError("invalid notes style probe".into())),
            };
            let probe_name = probe.attr("text:style-name").ok_or_else(|| OdpError("missing notes default run style".into()))?;
            let probe = style(&styles, &lookup, probe_name)?;
            let mut text_props = probe.child("style:text-properties").map(fragment).unwrap_or_default();
            if master && shared_role && i == 0 {
                definitions.push_str(&format!(
                    "<style:style style:name=\"{}\" style:family=\"paragraph\">{}{text_props}</style:style>",
                    role.paragraph,
                    contents(style(&styles, &lookup, &para_name)?)
                ));
                para.attrs.insert("text:style-name".into(), role.paragraph.clone());
            } else if !master && shared_role {
                let mut probe = document(&text_props)?;
                for node in &mut probe.children {
                    if let Node::El(el) = node {
                        el.attrs.retain(|key, _| run_key(key, &source.def_props));
                    }
                }
                text_props = contents(&probe);
                let target = style_mut(&mut styles, &lookup, &para_name)?;
                target.attrs.insert("style:parent-style-name".into(), role.paragraph.clone());
                filter_style(target, "style:paragraph-properties", |key| paragraph_key(key, &source.props), source.props.tabs.is_some());
                if source.props.rtl == Some(false)
                    && let Some(bag) = target.children.iter_mut().find_map(|node| match node {
                        Node::El(el) if el.name == "style:paragraph-properties" => Some(el),
                        _ => None,
                    })
                {
                    bag.attrs.insert("style:writing-mode".into(), "lr-tb".into());
                }
                if !text_props.is_empty() {
                    target.children.push(Node::El(
                        document(&text_props)?
                            .children
                            .into_iter()
                            .find_map(|node| match node {
                                Node::El(el) => Some(el),
                                _ => None,
                            })
                            .ok_or_else(|| OdpError("missing notes local text properties".into()))?,
                    ));
                }
                let mut elements = Vec::new();
                spans(para, &mut elements);
                if elements.len() != source.runs.len() {
                    return Err(OdpError("notes text spans changed during style serialization".into()));
                }
                for (span, run) in elements.into_iter().zip(&source.runs) {
                    let name = span.attr("text:style-name").ok_or_else(|| OdpError("missing notes run style".into()))?;
                    filter_style(style_mut(&mut styles, &lookup, name)?, "style:text-properties", |key| run_key(key, &run.props), true);
                }
            }
        }
        if master && shared_role && role.source.ph_type() != Some(deckcraft_model::PhType::SlideImage) {
            frame.attrs.insert("draw:text-style-name".into(), role.paragraph.clone());
        }
        if master && role.body.is_none_or(|body| body.paragraphs.is_empty()) {
            remove_paragraphs(frame);
        }
    }
    Ok(Rewritten { styles: contents(&styles), drawing: contents(&drawing), definitions })
}
