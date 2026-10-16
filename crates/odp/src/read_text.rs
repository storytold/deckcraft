//! Bounded ODF rich text, paragraph and list style mapping.
use super::*;
use deckcraft_model::text::{Align, Anchor, Bullet, RunKind};

fn family(value: &str) -> String {
    // ODF permits a comma-separated CSS family list; the model stores one preferred family.
    let value = value.trim();
    if let Some(quote) = value.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        return value.get(1..).and_then(|s| s.split_once(quote).map(|(name, _)| name)).unwrap_or(value).trim().to_string();
    }
    value.split(',').next().unwrap_or(value).trim().to_string()
}

// ODF 1.3 §§20.302–303 / 20.256–257: script language and country inherit
// independently, then form the model's BCP-47-style tag. `none` is an explicit clear.
fn script_language(props: &Props, language_key: &str, country_key: &str, inherited: Option<&str>) -> Option<String> {
    if !props.contains_key(language_key) && !props.contains_key(country_key) {
        return None;
    }
    let (old_language, old_country) =
        inherited.and_then(|s| s.split_once('-')).map_or((inherited, None), |(language, country)| (Some(language), Some(country)));
    let language = props.get(language_key).map(String::as_str).or(old_language)?;
    if language == "none" || language.is_empty() {
        return Some(String::new());
    }
    let country = props.get(country_key).map(String::as_str).or(old_country).filter(|value| *value != "none");
    Some(match country {
        Some(country) => format!("{language}-{country}"),
        None => language.into(),
    })
}

pub(super) fn run_props(props: &Props) -> RunProps {
    use deckcraft_model::text::{Caps, Strike};
    let get = |key: &str| props.get(key).map(String::as_str);
    let underline = match get("style:text-underline-style") {
        Some("none") => Some("none"),
        Some("wave") => Some(if get("style:text-underline-type") == Some("double") { "wavyDbl" } else { "wavy" }),
        Some("dotted") => Some("dotted"),
        Some("dash") => Some("dash"),
        Some("long-dash") => Some("dashLong"),
        Some("dot-dash") => Some("dotDash"),
        Some("dot-dot-dash") => Some("dotDotDash"),
        Some(_) => Some(if get("style:text-underline-type") == Some("double") { "dbl" } else { "sng" }),
        None => None,
    };
    let lang = get("fo:language").map(|language| {
        if language == "none" {
            String::new()
        } else {
            match get("fo:country").filter(|s| *s != "none") {
                Some(country) => format!("{language}-{country}"),
                None => language.into(),
            }
        }
    });
    RunProps {
        font: get("fo:font-family").or_else(|| get("style:font-name")).map(family),
        font_ea: get("style:font-family-asian").or_else(|| get("style:font-name-asian")).map(family),
        font_cs: get("style:font-family-complex").or_else(|| get("style:font-name-complex")).map(family),
        size: get("fo:font-size").and_then(length).filter(|n| *n > 0.0 && *n <= 10000.0),
        size_ea: get("style:font-size-asian").and_then(length).filter(|n| (1.0..=4000.0).contains(n)),
        size_cs: get("style:font-size-complex").and_then(length).filter(|n| (1.0..=4000.0).contains(n)),
        bold_ea: get("style:font-weight-asian").map(|s| s == "bold" || s == "700"),
        bold_cs: get("style:font-weight-complex").map(|s| s == "bold" || s == "700"),
        italic_ea: get("style:font-style-asian").map(|s| s == "italic" || s == "oblique"),
        italic_cs: get("style:font-style-complex").map(|s| s == "italic" || s == "oblique"),
        lang_ea: script_language(props, "style:language-asian", "style:country-asian", None),
        lang_cs: script_language(props, "style:language-complex", "style:country-complex", None),
        bold: get("fo:font-weight").map(|s| s == "bold" || s.parse::<u32>().is_ok_and(|n| n >= 600)),
        italic: get("fo:font-style").map(|s| s == "italic" || s == "oblique"),
        strike: get("style:text-line-through-style").map(|s| {
            if s == "none" {
                Strike::None
            } else if get("style:text-line-through-type") == Some("double") {
                Strike::Double
            } else {
                Strike::Single
            }
        }),
        spacing: get("fo:letter-spacing").and_then(|s| if s == "normal" { Some(0.0) } else { length(s) }),
        caps: if get("fo:font-variant") == Some("small-caps") {
            Some(Caps::Small)
        } else {
            get("fo:text-transform").map(|s| if s == "uppercase" { Caps::All } else { Caps::None })
        },
        lang,
        baseline: get("style:text-position").and_then(|s| s.split_whitespace().next()).and_then(|s| match s {
            "super" => Some(0.3),
            "sub" => Some(-0.25),
            s => s.strip_suffix('%').and_then(crate::geometry::number).map(|n| n / 100.0),
        }),
        underline: underline.map(|kind| {
            if get("style:text-underline-mode") == Some("skip-white-space") && kind == "sng" {
                return "words".into();
            }
            if get("style:text-underline-width") == Some("bold") {
                return match kind {
                    "sng" => "heavy".into(),
                    "dotted" => "dottedHeavy".into(),
                    "dash" => "dashHeavy".into(),
                    "dashLong" => "dashLongHeavy".into(),
                    "dotDash" => "dotDashHeavy".into(),
                    "dotDotDash" => "dotDotDashHeavy".into(),
                    "wavy" => "wavyHeavy".into(),
                    other => other.into(),
                };
            }
            kind.into()
        }),
        underline_color: get("style:text-underline-color").and_then(color),
        fill: get("fo:color").and_then(color).map(Fill::solid),
        highlight: get("fo:background-color")
            .and_then(|value| if value == "transparent" { Some(ColorRef::rgb(Rgba::TRANSPARENT)) } else { color(value) }),
        kern: get("style:letter-kerning").map(|s| if s == "true" { 0.0 } else { 10000.0 }),
        ..Default::default()
    }
}

fn apply_run(props: &Props, inherited: &RunProps) -> Result<RunProps> {
    let mut run = run_props(props);
    if let Some(percent) = props.get("fo:font-size").and_then(|v| v.strip_suffix('%')).and_then(crate::geometry::number) {
        run.size = inherited.size.map(|size| size * percent / 100.0).filter(|size| size.is_finite() && *size > 0.0 && *size <= 10000.0);
    }
    // §§20.284–285: percentages use the parent style's script font height, not
    // a sibling fo:font-size declared on this same style.
    for (key, parent, target) in [
        ("style:font-size-asian", inherited.size_ea.or(inherited.size), &mut run.size_ea),
        ("style:font-size-complex", inherited.size_cs.or(inherited.size), &mut run.size_cs),
    ] {
        if let Some(value) = props.get(key) {
            let size = if let Some(percent) = value.strip_suffix('%') {
                let percent =
                    crate::geometry::number(percent).filter(|v| *v > 0.0).ok_or_else(|| OdpError("invalid script font-size percentage".into()))?;
                parent.map(|size| size * percent / 100.0).ok_or_else(|| OdpError("script font-size percentage has no resolved parent size".into()))?
            } else {
                length(value).ok_or_else(|| OdpError("invalid script font size".into()))?
            };
            if !size.is_finite() || !(1.0..=4000.0).contains(&size) {
                return Err(OdpError("script font size exceeds supported range".into()));
            }
            *target = Some(size);
        }
    }
    run.lang_ea = script_language(props, "style:language-asian", "style:country-asian", inherited.lang_ea.as_deref().or(inherited.lang.as_deref()));
    run.lang_cs =
        script_language(props, "style:language-complex", "style:country-complex", inherited.lang_cs.as_deref().or(inherited.lang.as_deref()));
    for (country, language) in [("style:country-asian", &run.lang_ea), ("style:country-complex", &run.lang_cs)] {
        if props.contains_key(country) && language.is_none() {
            return Err(OdpError("script country has no resolved language".into()));
        }
    }
    if props.contains_key("fo:language") || props.contains_key("fo:country") {
        let (old_language, old_country) =
            inherited.lang.as_deref().and_then(|s| s.split_once('-')).map(|(a, b)| (Some(a), Some(b))).unwrap_or((inherited.lang.as_deref(), None));
        let language = props.get("fo:language").map(String::as_str).or(old_language);
        let country = props.get("fo:country").map(String::as_str).or(old_country);
        run.lang = language.map(|language| {
            if language == "none" {
                String::new()
            } else {
                match country.filter(|s| *s != "none") {
                    Some(country) => format!("{language}-{country}"),
                    None => language.into(),
                }
            }
        });
    }
    if let Some(add) = props.get("style:font-size-rel").and_then(|s| length(s)) {
        run.size = inherited.size.map(|v| v + add).filter(|v| *v > 0.0 && *v <= 10000.0);
    }
    run.inherit(inherited);
    Ok(run)
}

fn styled_run(styles: &Styles<'_>, family: &str, name: Option<&str>, base: &RunProps, default: bool) -> Result<RunProps> {
    let mut chain = Vec::new();
    let mut current = name;
    while let Some(name) = current {
        if chain.len() >= 32 {
            return Err(OdpError("cyclic or excessive text style inheritance".into()));
        }
        let Some(style) = styles.get(&(family.into(), name.into())) else { break };
        chain.push(*style);
        current = style.attr("style:parent-style-name");
    }
    let mut run = base.clone();
    let defaults = default.then(|| styles.get(&(family.into(), String::new())).copied()).flatten();
    for style in defaults.into_iter().chain(chain.into_iter().rev()) {
        if let Some(el) = style.child("style:text-properties") {
            let mut props = el.attrs.clone();
            for (key, target) in [
                ("style:font-name", "fo:font-family"),
                ("style:font-name-asian", "style:font-family-asian"),
                ("style:font-name-complex", "style:font-family-complex"),
            ] {
                if let Some(face) = props.get(key).and_then(|n| styles.get(&("font".into(), n.clone()))).and_then(|face| face.attr("svg:font-family"))
                {
                    props.insert(target.into(), face.into());
                }
            }
            run = apply_run(&props, &run)?;
        }
    }
    Ok(run)
}

pub(super) fn hyperlink(url: &str, tooltip: &str) -> deckcraft_model::text::Hyperlink {
    deckcraft_model::text::Hyperlink {
        action: deckcraft_model::text::Action::Url { url: url.into() },
        tooltip: tooltip.into(),
        highlight_click: false,
        sound: None,
        stop_sound: false,
        zoom_return: None,
        sound_name: None,
    }
}

#[derive(Default)]
struct Whitespace {
    started: bool,
    pending: Option<RunProps>,
}
fn flush_space(space: &mut Whitespace, runs: &mut Vec<Run>, budget: &mut usize) -> Result<()> {
    if let Some(props) = space.pending.take() {
        text_charge(budget, 1 + run_weight(&props))?;
        runs.push(Run::with(" ", props));
    }
    Ok(())
}
fn normal_text(value: &str, props: &RunProps, runs: &mut Vec<Run>, budget: &mut usize, space: &mut Whitespace) -> Result<()> {
    let mut normalized = String::new();
    for ch in value.chars() {
        if matches!(ch, ' ' | '\t' | '\r' | '\n') {
            if space.started && space.pending.is_none() {
                space.pending = Some(props.clone());
            }
        } else {
            if let Some(pending) = space.pending.take() {
                if normalized.is_empty() && pending != *props {
                    text_charge(budget, 1 + run_weight(&pending))?;
                    runs.push(Run::with(" ", pending));
                } else {
                    normalized.push(' ');
                }
            }
            normalized.push(ch);
            space.started = true;
        }
    }
    if !normalized.is_empty() {
        text_charge(budget, normalized.len().saturating_add(run_weight(props)))?;
        runs.push(Run::with(normalized, props.clone()));
    }
    Ok(())
}
fn inline(el: &El, styles: &Styles<'_>, inherited: &RunProps, runs: &mut Vec<Run>, budget: &mut usize, space: &mut Whitespace) -> Result<()> {
    // An unstyled nested span inherits its immediate parent. Reapplying document text
    // defaults here incorrectly resets a paragraph's size and a parent's bold/colour.
    let mut props = if matches!(el.name.as_str(), "text:span" | "text:a") {
        styled_run(styles, "text", el.attr("text:style-name"), inherited, false)?
    } else {
        inherited.clone()
    };
    if el.name == "text:a"
        && let Some(url) = el.attr("xlink:href")
    {
        props.link = Some(hyperlink(url, el.attr("office:title").or_else(|| el.attr("xlink:title")).unwrap_or("")));
    }
    for n in &el.children {
        match n {
            Node::Text(s) => normal_text(s, &props, runs, budget, space)?,
            Node::El(e) => match e.name.as_str() {
                "office:annotation" | "text:ruby-text" => {}
                "text:ruby" => {
                    if let Some(base) = e.child("text:ruby-base") {
                        inline(base, styles, &props, runs, budget, space)?;
                    }
                }
                "text:page-number" | "text:date" | "text:time" => {
                    if e.attr("text:fixed") == Some("true") {
                        inline(e, styles, &props, runs, budget, space)?;
                    } else {
                        flush_space(space, runs, budget)?;
                        space.started = true;
                        let value = extras::plain(e);
                        text_charge(budget, value.len().saturating_add(run_weight(&props)))?;
                        runs.push(Run {
                            kind: RunKind::Field { field: if e.name == "text:page-number" { "slidenum" } else { "datetime1" }.into() },
                            ..Run::with(value, props.clone())
                        });
                    }
                }
                "presentation:header" | "presentation:footer" | "presentation:date-time" => {
                    if e.children.iter().any(|child| match child {
                        Node::Text(text) => !text.trim().is_empty(),
                        Node::El(_) => true,
                    }) {
                        return Err(OdpError("presentation notes field must be empty".into()));
                    }
                    flush_space(space, runs, budget)?;
                    space.started = true;
                    text_charge(budget, run_weight(&props))?;
                    runs.push(Run {
                        kind: RunKind::Field {
                            field: match e.name.as_str() {
                                "presentation:header" => "header",
                                "presentation:footer" => "footer",
                                _ => "notesdatetime",
                            }
                            .into(),
                        },
                        ..Run::with("", props.clone())
                    });
                }
                "text:s" => {
                    flush_space(space, runs, budget)?;
                    space.started = true;
                    let count = e.attr("text:c").unwrap_or("1").parse::<usize>().map_err(|_| OdpError("invalid space count".into()))?;
                    if count > 100_000 {
                        return Err(OdpError("excessive space count".into()));
                    }
                    text_charge(budget, count.saturating_add(run_weight(&props)))?;
                    runs.push(Run::with(" ".repeat(count), props.clone()));
                }
                "text:tab" => {
                    flush_space(space, runs, budget)?;
                    space.started = true;
                    text_charge(budget, 1 + run_weight(&props))?;
                    runs.push(Run::with("\t", props.clone()));
                }
                "text:line-break" => {
                    flush_space(space, runs, budget)?;
                    space.started = true;
                    text_charge(budget, run_weight(&props))?;
                    runs.push(Run { kind: RunKind::Break, ..Run::with("", props.clone()) });
                }
                _ => inline(e, styles, &props, runs, budget, space)?,
            },
        }
    }
    Ok(())
}

fn paragraph_with(
    e: &El,
    styles: &Styles<'_>,
    level: u8,
    budget: &mut usize,
    base: &RunProps,
    base_para: &deckcraft_model::text::ParaProps,
) -> Result<Paragraph> {
    let props = properties(styles, "paragraph", e.attr("text:style-name"))?;
    let defaults = styled_run(styles, "text", None, &RunProps::default(), true)?;
    let defaults = styled_run(styles, "paragraph", None, &defaults, true)?;
    let mut inherited = base.clone();
    inherited.inherit(&defaults);
    let def_props = styled_run(styles, "paragraph", e.attr("text:style-name"), &inherited, false)?;
    let mut p = Paragraph { level, def_props, ..Default::default() };
    p.props.background = paragraph_background(styles, e.attr("text:style-name"), base_para.background.as_ref())?;
    p.props.rtl = props.get("style:writing-mode").map(|s| s == "rl-tb");
    p.props.align = props.get("fo:text-align").map(|s| match s.as_str() {
        "center" => Align::Center,
        "right" => Align::Right,
        "end" => {
            if p.props.rtl == Some(true) {
                Align::Left
            } else {
                Align::Right
            }
        }
        "start" => {
            if p.props.rtl == Some(true) {
                Align::Right
            } else {
                Align::Left
            }
        }
        "justify" => Align::Justify,
        _ => Align::Left,
    });
    p.props.margin_left = props.get("fo:margin-left").and_then(|s| length(s));
    p.props.margin_right = props.get("fo:margin-right").and_then(|s| length(s));
    p.props.indent = props.get("fo:text-indent").and_then(|s| length(s));
    p.props.space_before = props.get("fo:margin-top").and_then(|s| length(s)).map(deckcraft_model::text::Spacing::Pts);
    p.props.space_after = props.get("fo:margin-bottom").and_then(|s| length(s)).map(deckcraft_model::text::Spacing::Pts);
    match crate::loss_report::paragraph_line_height_for_read(styles, e.attr("text:style-name"), |bytes| text_charge(budget, bytes))? {
        Ok((spacing, policy)) => {
            p.props.line_spacing = spacing;
            p.props.line_height = policy;
        }
        Err(crate::loss_report::FeatureCode::InvalidValue) => {
            return Err(OdpError("invalid ODF paragraph minimum/additional line-height value".into()));
        }
        Err(_) => {
            // Unsupported combinations retain their archive and blocking audit/warning.
            // Do not invent an order or silently turn a modifier into exact spacing.
        }
    }
    p.props.default_tab = props.get("style:tab-stop-distance").and_then(|s| length(s)).filter(|v| *v > 0.0);
    p.props.latin_line_break = props.get("style:line-break").map(|s| s == "normal");
    p.props.hanging_punct = props.get("style:punctuation-wrap").map(|s| s == "hanging");
    p.props.east_asian_line_break = props.get("style:line-break").map(|s| s == "strict");
    p.props.font_align = props.get("style:vertical-align").map(|s| {
        match s.as_str() {
            "top" => "t",
            "middle" => "ctr",
            "bottom" => "b",
            _ => "base",
        }
        .into()
    });
    p.props.tabs = tabs(e, styles)?;
    p.props.inherit(base_para);
    text_charge(budget, paragraph_weight(&p))?;
    inline(e, styles, &p.def_props, &mut p.runs, budget, &mut Whitespace::default())?;
    Ok(p)
}

/// Backgrounds are paragraph properties, not text highlights. The generic property bag
/// deliberately combines property kinds, so it cannot distinguish their identical FO name.
fn paragraph_background(styles: &Styles<'_>, name: Option<&str>, base: Option<&Fill>) -> Result<Option<Fill>> {
    let base = base.map(|fill| ParagraphBackground { fill: fill.clone(), extension: None });
    Ok(paragraph_background_with_source(styles, "paragraph", name, base.as_ref())?.map(|background| background.fill))
}

#[derive(Clone)]
struct ParagraphBackground {
    fill: Fill,
    // A resolved model default has no source encoding; warning traversal retains that
    // context separately for shape text styles, without adding provenance to the model.
    extension: Option<bool>,
}

fn paragraph_background_with_source(
    styles: &Styles<'_>,
    family: &str,
    name: Option<&str>,
    base: Option<&ParagraphBackground>,
) -> Result<Option<ParagraphBackground>> {
    fn local(style: &El) -> Result<Option<ParagraphBackground>> {
        for name in ["style:paragraph-properties", "loext:graphic-properties"] {
            if style.elements().filter(|child| child.name == name).take(2).count() > 1 {
                return Err(OdpError("duplicate paragraph background property elements".into()));
            }
        }
        let mut background = None;
        if let Some(value) = style.child("style:paragraph-properties").and_then(|p| p.attr("fo:background-color")) {
            let fill = if value == "transparent" {
                Fill::None
            } else {
                Fill::solid(color(value).ok_or_else(|| OdpError("invalid paragraph background color".into()))?)
            };
            background = Some(ParagraphBackground { fill, extension: Some(false) });
        }
        // The recognized extension is intentionally narrower than general graphic fills.
        // Everything else is still reported by the namespace-aware foreign-content guard.
        if let Some(fill) = style.child("loext:graphic-properties").filter(|fill| fill.supported_paragraph_fill(style)) {
            let fill = match fill.attr("draw:fill") {
                Some("none") => Fill::None,
                Some("solid") => Fill::solid(
                    fill.attr("draw:fill-color").and_then(color).ok_or_else(|| OdpError("invalid paragraph background fill color".into()))?,
                ),
                _ => return Ok(background),
            };
            background = Some(ParagraphBackground { fill, extension: Some(true) });
        }
        Ok(background)
    }
    let mut chain = Vec::new();
    let mut current = name;
    while let Some(name) = current {
        if chain.len() >= 32 {
            return Err(OdpError("cyclic or excessive paragraph background style inheritance".into()));
        }
        let Some(style) = styles.get(&(family.into(), name.into())) else { break };
        chain.push(*style);
        current = style.attr("style:parent-style-name");
    }
    let mut background = styles.get(&(family.into(), String::new())).map(|style| local(style)).transpose()?.flatten();
    // A shape's named text style is closer to its text than the document default.
    if let Some(base) = base {
        background = Some(base.clone());
    }
    for style in chain.into_iter().rev() {
        if let Some(value) = local(style)? {
            if background.as_ref().is_some_and(|old| {
                !old.fill.is_none()
                    && !value.fill.is_none()
                    && old.fill != value.fill
                    && old.extension.zip(value.extension).is_some_and(|(a, b)| a != b)
            }) {
                return Err(OdpError("conflicting standard and extension paragraph background inheritance is not supported".into()));
            }
            background = Some(value);
        }
    }
    Ok(background)
}

fn shape_background(el: &El, styles: &Styles<'_>, base: Option<&ParagraphBackground>) -> Result<Option<ParagraphBackground>> {
    let presentation = paragraph_background_with_source(styles, "presentation", el.attr("presentation:style-name"), base)?;
    paragraph_background_with_source(styles, "graphic", el.attr("draw:style-name"), presentation.as_ref())
}

pub(super) fn text(el: &El, styles: &Styles<'_>, paragraphs: &mut Vec<Paragraph>, level: u8) -> Result<()> {
    text_with(el, styles, paragraphs, level, &RunProps::default(), &deckcraft_model::text::ParaProps::default())
}

pub(super) fn text_with(
    el: &El,
    styles: &Styles<'_>,
    paragraphs: &mut Vec<Paragraph>,
    level: u8,
    base: &RunProps,
    base_para: &deckcraft_model::text::ParaProps,
) -> Result<()> {
    walk_text(el, styles, paragraphs, level, None, false, &mut 0, base, base_para, &mut ListState::default())
}

fn walk_text(
    el: &El,
    styles: &Styles<'_>,
    paragraphs: &mut Vec<Paragraph>,
    depth: u8,
    list: Option<&El>,
    in_list: bool,
    budget: &mut usize,
    base: &RunProps,
    base_para: &deckcraft_model::text::ParaProps,
    state: &mut ListState,
) -> Result<()> {
    walk_text_elements(el.elements(), styles, paragraphs, depth, list, in_list, budget, base, base_para, state)
}

fn walk_text_elements<'a>(
    elements: impl Iterator<Item = &'a El>,
    styles: &Styles<'_>,
    paragraphs: &mut Vec<Paragraph>,
    depth: u8,
    list: Option<&El>,
    in_list: bool,
    budget: &mut usize,
    base: &RunProps,
    base_para: &deckcraft_model::text::ParaProps,
    state: &mut ListState,
) -> Result<()> {
    for e in elements {
        match e.name.as_str() {
            "text:p" | "text:h" => {
                let mut p = paragraph_with(e, styles, depth.saturating_sub(1), budget, base, base_para)?;
                if in_list {
                    let definition = list.and_then(|s| s.elements().find(|l| l.attr("text:level").and_then(|v| v.parse::<u8>().ok()) == Some(depth)));
                    if let Some(definition) = definition {
                        text_charge(budget, definition.attr("text:bullet-char").map(str::len).unwrap_or(0))?;
                        if let Some(props) = definition.child("style:list-level-properties") {
                            text_charge(budget, props.attr("style:font-name").map(str::len).unwrap_or(0))?;
                        }
                        p.props.bullet = Some(match definition.name.as_str() {
                            "text:list-level-style-number" => Bullet::AutoNum {
                                scheme: number_scheme(definition),
                                start_at: definition.attr("text:start-value").and_then(|n| n.parse().ok()).unwrap_or(1),
                            },
                            _ => Bullet::Char { char: definition.attr("text:bullet-char").unwrap_or("•").into() },
                        });
                        if let Some(props) = definition.child("style:list-level-properties") {
                            if let Some(space) = props.attr("text:space-before").and_then(length) {
                                let label = props.attr("text:min-label-width").and_then(length).unwrap_or(0.0);
                                p.props.margin_left = Some(space + label);
                                p.props.indent = Some(-label);
                            }
                            if let Some(alignment) = props.child("style:list-level-label-alignment") {
                                p.props.margin_left = alignment.attr("fo:margin-left").and_then(length);
                                p.props.indent = alignment.attr("fo:text-indent").and_then(length);
                            }
                            p.props.bullet_font = props.attr("style:font-name").map(str::to_string);
                        }
                        if let Some(text_props) = definition.child("style:text-properties") {
                            let bullet = run_props(&text_props.attrs);
                            p.props.bullet_font = bullet.font.or(p.props.bullet_font);
                            p.props.bullet_color = text_props.attr("fo:color").and_then(color);
                            p.props.bullet_size = definition
                                .child("style:list-level-properties")
                                .and_then(|e| e.attr("text:bullet-relative-size"))
                                .and_then(|s| s.strip_suffix('%'))
                                .and_then(crate::geometry::number)
                                .map(|v| v / 100.0);
                        }
                        if let Some(font) = &p.props.bullet_font
                            && let Some(face) = styles.get(&("font".into(), font.clone())).and_then(|f| f.attr("svg:font-family"))
                        {
                            p.props.bullet_font = Some(family(face));
                        }
                    } else {
                        p.props.bullet = Some(Bullet::Char { char: "•".into() });
                    }
                }
                paragraphs.push(p);
            }
            "text:list" => {
                let named = e.attr("text:style-name").and_then(|n| styles.get(&("list".into(), n.into()))).copied().or(list);
                if depth >= 9 {
                    return Err(OdpError("text list depth exceeds model limit".into()));
                }
                let start = paragraphs.len();
                walk_text(e, styles, paragraphs, depth.saturating_add(1), named, true, budget, base, base_para, state)?;
                finish_list(e, named, paragraphs, start, depth, state)?;
            }
            "text:list-item" => {
                let start = paragraphs.len();
                walk_text(e, styles, paragraphs, depth, list, in_list, budget, base, base_para, state)?;
                let mut first = true;
                for paragraph in paragraphs.iter_mut().skip(start) {
                    if paragraph.level == depth.saturating_sub(1) {
                        if first {
                            first = false;
                        } else {
                            paragraph.props.bullet = Some(Bullet::None);
                        }
                    }
                }
                if let Some(value) = e.attr("text:start-value").and_then(|n| n.parse::<u32>().ok())
                    && let Some(p) = paragraphs.get_mut(start)
                    && matches!(p.props.bullet, Some(Bullet::AutoNum { .. }))
                {
                    p.props.number_restart = Some(value.max(1));
                }
            }
            "text:list-header" => walk_text(e, styles, paragraphs, depth, list, false, budget, base, base_para, state)?,
            "draw:text-box" => walk_text(e, styles, paragraphs, depth, list, in_list, budget, base, base_para, state)?,
            _ => {}
        }
    }
    Ok(())
}

fn tabs(el: &El, styles: &Styles<'_>) -> Result<Option<Vec<deckcraft_model::text::TabStop>>> {
    let mut current = el.attr("text:style-name");
    for _ in 0..32 {
        let Some(name) = current else { return Ok(None) };
        let Some(style) = styles.get(&("paragraph".into(), name.into())) else { return Ok(None) };
        if let Some(stops) = style.child("style:paragraph-properties").and_then(|e| e.child("style:tab-stops")) {
            let mut out = Vec::new();
            for stop in stops.elements().filter(|e| e.name == "style:tab-stop") {
                if out.len() >= 128 {
                    return Err(OdpError("too many text tab stops".into()));
                }
                if let Some(pos) = stop.attr("style:position").and_then(length).filter(|v| *v >= 0.0) {
                    out.push(deckcraft_model::text::TabStop {
                        pos,
                        align: match stop.attr("style:type").unwrap_or("left") {
                            "right" => "r",
                            "center" => "ctr",
                            "char" => "dec",
                            _ => "l",
                        }
                        .into(),
                    });
                }
            }
            out.sort_by(|a, b| a.pos.total_cmp(&b.pos));
            return Ok(Some(out));
        }
        current = style.attr("style:parent-style-name");
    }
    Err(OdpError("cyclic or excessive paragraph style inheritance".into()))
}

pub(super) fn shape_defaults(el: &El, props: &Props, styles: &Styles<'_>) -> Result<Paragraph> {
    let mut paragraph = El::default();
    paragraph.name = "text:p".into();
    if let Some(name) = el.attr("draw:text-style-name") {
        paragraph.attrs.insert("text:style-name".into(), name.into());
    }
    let mut run = run_props(props);
    // Shape style text highlights and paragraph areas share an FO attribute name, too.
    // Only text-properties may supply the inherited character highlight.
    let text = styled_run(styles, "presentation", el.attr("presentation:style-name"), &RunProps::default(), true)?;
    let styled = styled_run(styles, "graphic", el.attr("draw:style-name"), &text, true)?;
    run.highlight = styled.highlight;
    // Flattened property maps lose percentage ownership. Script fields must resolve
    // through the actual common/default/parent style chain before paragraph fallback.
    run.size_ea = styled.size_ea;
    run.size_cs = styled.size_cs;
    run.bold_ea = styled.bold_ea;
    run.bold_cs = styled.bold_cs;
    run.italic_ea = styled.italic_ea;
    run.italic_cs = styled.italic_cs;
    run.lang_ea = styled.lang_ea;
    run.lang_cs = styled.lang_cs;
    let base = deckcraft_model::text::ParaProps { background: shape_background(el, styles, None)?.map(|value| value.fill), ..Default::default() };
    paragraph_with(&paragraph, styles, 0, &mut 0, &run, &base)
}

pub(super) fn body_props(props: &Props, el: &El, styles: &Styles<'_>) -> deckcraft_model::text::BodyProps {
    use deckcraft_model::text::{AutoFit, BodyProps, TextDir};
    let pad = |key: &str| props.get(key).or_else(|| props.get("fo:padding")).and_then(|s| length(s)).unwrap_or(0.0);
    let mode = props.get("style:writing-mode").map(String::as_str);
    let columns =
        el.child("draw:text-box").and_then(|e| e.child("style:columns")).or_else(|| el.child("style:columns")).or_else(|| style_columns(el, styles));
    BodyProps {
        inset_l: Some(pad("fo:padding-left")),
        inset_r: Some(pad("fo:padding-right")),
        inset_t: Some(pad("fo:padding-top")),
        inset_b: Some(pad("fo:padding-bottom")),
        anchor: props.get("draw:textarea-vertical-align").map(|s| match s.as_str() {
            "middle" => Anchor::Middle,
            "bottom" => Anchor::Bottom,
            "justify" => Anchor::Justified,
            _ => Anchor::Top,
        }),
        anchor_ctr: props.get("draw:textarea-horizontal-align").map(|s| s == "center"),
        wrap: Some(props.get("fo:wrap-option").is_none_or(|s| s != "no-wrap")),
        autofit: Some(
            if props.get("draw:fit-to-size").is_some_and(|s| matches!(s.as_str(), "true" | "all" | "shrink-to-fit"))
                || props.get("style:shrink-to-fit").is_some_and(|s| s == "true")
            {
                AutoFit::Shrink { font_scale: Some(1.0), line_reduction: 0.0 }
            } else if props.get("draw:auto-grow-height").is_some_and(|s| s == "true") {
                AutoFit::Shape
            } else {
                AutoFit::None
            },
        ),
        vert: mode.and_then(|s| match s {
            "tb-rl" => Some(TextDir::EaVertical),
            "tb-lr" => Some(TextDir::Vertical),
            "lr-tb" | "rl-tb" => Some(TextDir::Horizontal),
            _ => None,
        }),
        rot: el.child("draw:enhanced-geometry").and_then(|g| g.attr("draw:text-rotate-angle")).and_then(angle),
        rtl_col: mode.map(|s| s == "rl-tb" || s == "tb-rl"),
        columns: columns.and_then(|e| e.attr("fo:column-count")).and_then(|s| s.parse::<u32>().ok()).filter(|n| (1..=16).contains(n)),
        col_spacing: columns.and_then(|e| e.attr("fo:column-gap")).and_then(length),
        ..Default::default()
    }
}

pub(super) fn resolve_links(p: &mut Presentation) {
    let mut targets = BTreeMap::new();
    for slide in &p.slides {
        if !slide.name.is_empty() {
            targets.insert(slide.name.clone(), slide.id);
        }
        for (key, value) in &slide.tags {
            if key == "odp:id" {
                targets.insert(value.clone(), slide.id);
            }
        }
    }
    fn decode(value: &str) -> Option<String> {
        let mut out = Vec::with_capacity(value.len());
        let mut chars = value.bytes();
        while let Some(ch) = chars.next() {
            if ch == b'%' {
                let a = (chars.next()? as char).to_digit(16)?;
                let b = (chars.next()? as char).to_digit(16)?;
                out.push((a * 16 + b) as u8);
            } else {
                out.push(ch);
            }
        }
        String::from_utf8(out).ok()
    }
    fn link(value: &mut Option<deckcraft_model::text::Hyperlink>, targets: &BTreeMap<String, SlideId>) {
        if let Some(link) = value
            && let deckcraft_model::text::Action::Url { url } = &link.action
            && let Some(fragment) = url.strip_prefix('#')
            && let Some(id) = targets.get(fragment).copied().or_else(|| decode(fragment).and_then(|name| targets.get(&name).copied()))
        {
            link.action = deckcraft_model::text::Action::Slide { slide: id };
        }
    }
    fn run(props: &mut RunProps, targets: &BTreeMap<String, SlideId>) {
        link(&mut props.link, targets);
        link(&mut props.hover, targets);
    }
    fn list(style: &mut deckcraft_model::text::ListStyle, targets: &BTreeMap<String, SlideId>) {
        for level in style.levels.iter_mut().flatten() {
            run(&mut level.run, targets);
        }
    }
    fn body(body: &mut TextBody, targets: &BTreeMap<String, SlideId>) {
        list(&mut body.list_style, targets);
        for para in &mut body.paragraphs {
            run(&mut para.def_props, targets);
            run(&mut para.end_props, targets);
            for item in &mut para.runs {
                run(&mut item.props, targets);
            }
        }
    }
    fn shapes(items: &mut [Shape], targets: &BTreeMap<String, SlideId>, depth: usize) {
        if depth >= 64 {
            return;
        }
        for shape in items {
            link(&mut shape.click, targets);
            link(&mut shape.hover, targets);
            if let Some(text) = &mut shape.text {
                body(text, targets);
            }
            match &mut shape.kind {
                ShapeKind::Group { children, .. } => shapes(children, targets, depth + 1),
                ShapeKind::Table(table) => {
                    for row in &mut table.rows {
                        for cell in &mut row.cells {
                            body(&mut cell.text, targets);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    fn master(master: &mut Master, targets: &BTreeMap<String, SlideId>) {
        shapes(&mut master.shapes, targets, 0);
        for layout in &mut master.layouts {
            shapes(&mut layout.shapes, targets, 0);
        }
        list(&mut master.title_style, targets);
        list(&mut master.body_style, targets);
        list(&mut master.other_style, targets);
    }
    for slide in &mut p.slides {
        let slide = Arc::make_mut(slide);
        shapes(&mut slide.shapes, &targets, 0);
        body(&mut slide.notes, &targets);
        if let Some(page) = &mut slide.notes_page {
            shapes(&mut page.shapes, &targets, 0);
        }
    }
    for item in &mut p.masters {
        master(Arc::make_mut(item), &targets);
    }
    if let Some(item) = &mut p.notes_master {
        master(Arc::make_mut(item), &targets);
    }
    for item in &mut p.notes_masters {
        master(Arc::make_mut(&mut item.master), &targets);
    }
    if let Some(item) = &mut p.handout_master {
        master(Arc::make_mut(item), &targets);
    }
    list(&mut p.default_text_style, &targets);
}

fn number_scheme(definition: &El) -> String {
    let family = match definition.attr("style:num-format").unwrap_or("1") {
        "a" => "alphaLc",
        "A" => "alphaUc",
        "i" => "romanLc",
        "I" => "romanUc",
        _ => "arabic",
    };
    let prefix = definition.attr("style:num-prefix").unwrap_or("");
    let suffix = definition.attr("style:num-suffix").unwrap_or(".");
    let ending = match (prefix, suffix) {
        ("(", ")") => "ParenBoth",
        (_, ")") => "ParenR",
        (_, ".") => "Period",
        (_, "-") => "Minus",
        _ => "Plain",
    };
    format!("{family}{ending}")
}

fn angle(value: &str) -> Option<f64> {
    if let Some(n) = value.strip_suffix("deg") {
        crate::geometry::number(n)
    } else if let Some(n) = value.strip_suffix("grad") {
        crate::geometry::number(n).map(|n| n * 0.9)
    } else if let Some(n) = value.strip_suffix("rad") {
        crate::geometry::number(n).map(f64::to_degrees)
    } else {
        crate::geometry::number(value)
    }
}

#[derive(Default)]
struct ListState {
    previous: [u32; 9],
    named: BTreeMap<String, [u32; 9]>,
}
fn finish_list(el: &El, style: Option<&El>, paragraphs: &mut [Paragraph], start: usize, depth: u8, state: &mut ListState) -> Result<()> {
    let level = usize::from(depth);
    let initial = style
        .and_then(|s| s.elements().find(|e| e.attr("text:level").and_then(|s| s.parse::<u8>().ok()) == Some(depth + 1)))
        .and_then(|s| s.attr("text:start-value"))
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1)
        .max(1);
    let continued = el
        .attr("text:continue-list")
        .and_then(|name| state.named.get(name))
        .and_then(|values| values.get(level))
        .copied()
        .or_else(|| (el.attr("text:continue-numbering") == Some("true")).then(|| state.previous.get(level).copied().unwrap_or(0)));
    let mut next = continued.map(|n| n.saturating_add(1)).unwrap_or(initial);
    let mut first = true;
    let mut interrupted = false;
    let mut last = continued.unwrap_or(0);
    for para in paragraphs.iter_mut().skip(start).filter(|p| p.level == depth) {
        if let Some(Bullet::AutoNum { start_at, .. }) = &mut para.props.bullet {
            let explicit = para.props.number_restart;
            if let Some(value) = explicit {
                next = value.max(1);
            }
            para.props.number_restart = if first || interrupted || explicit.is_some() { Some(next) } else { None };
            *start_at = 1;
            first = false;
            interrupted = false;
            if !para.is_empty() {
                last = next;
                next = next.saturating_add(1);
            }
        } else if !para.is_empty() {
            interrupted = true;
        }
    }
    if let Some(value) = state.previous.get_mut(level) {
        *value = last;
    }
    if let Some(name) = el.attr("xml:id").or_else(|| el.attr("text:id")) {
        if state.named.len() >= 1000 {
            return Err(OdpError("too many continued list identifiers".into()));
        }
        state.named.insert(name.into(), state.previous);
    }
    Ok(())
}

pub(super) fn paragraph_weight(p: &Paragraph) -> usize {
    let tabs = p
        .props
        .tabs
        .as_ref()
        .map(|t| {
            t.iter().fold(0usize, |sum, t| sum.saturating_add(std::mem::size_of::<deckcraft_model::text::TabStop>()).saturating_add(t.align.len()))
        })
        .unwrap_or(0);
    let bullet = match &p.props.bullet {
        Some(Bullet::Char { char }) => char.len(),
        Some(Bullet::AutoNum { scheme, .. }) => scheme.len(),
        _ => 0,
    };
    std::mem::size_of::<Paragraph>()
        .saturating_add(run_weight(&p.def_props))
        .saturating_add(run_weight(&p.end_props))
        .saturating_add(tabs)
        .saturating_add(bullet)
        .saturating_add(p.props.bullet_font.as_ref().map(String::len).unwrap_or(0))
        .saturating_add(p.props.font_align.as_ref().map(String::len).unwrap_or(0))
}

/// Report model constraints on styles actually used by this text, alongside source recovery.
pub(super) fn warnings(el: &El, styles: &Styles<'_>, shape_props: &Props) -> Vec<&'static str> {
    let mut warnings = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    fn add(out: &mut Vec<&'static str>, message: &'static str) {
        if !out.contains(&message) {
            out.push(message);
        }
    }
    fn inspect(el: &El, out: &mut Vec<&'static str>) {
        if el.name == "style:paragraph-properties" {
            if el.attrs.iter().any(|(key, value)| {
                (key == "fo:padding" || key.starts_with("fo:padding-")) && length(value) != Some(0.0)
                    || (key == "fo:border" || key.starts_with("fo:border-")) && value != "none"
                    || key == "style:join-border" && value != "false"
                    || key == "style:shadow" && value != "none"
                    || key == "style:background-transparency" && value != "0%"
            }) {
                add(
                    out,
                    "ODF paragraph padding, borders, joined areas or background effects are retained in the source archive; paragraph-area geometry is incomplete",
                );
            }
            if el.child("style:background-image").is_some() {
                add(out, "ODF paragraph background images are retained in the source archive; editable paragraph image fills are not supported");
            }
        }
        if el.attr("style:leader-style").is_some_and(|v| v != "none") || el.attr("style:char").is_some_and(|v| v != ".") {
            add(out, "ODF tab leaders or custom decimal characters are retained in the source archive; their rendering is incomplete");
        }
        if el.attr("fo:hyphenate") == Some("true") {
            add(out, "ODF automatic hyphenation is retained in the source archive; layout does not reproduce it yet");
        }
        if el.attr("style:text-rotation-angle").is_some()
            || el.attr("style:text-combine").is_some_and(|v| v != "none")
            || el.attr("style:text-scale").is_some_and(|v| v != "100%")
        {
            add(out, "ODF character rotation/combining/scaling is retained in the source archive; run-level layout mapping is incomplete");
        }
        if el.attr("style:text-position").is_some_and(|v| !crate::loss_report::supported_text_position(v)) {
            add(out, "ODF custom superscript/subscript sizing is retained in the source archive; rendering uses the model's fixed script scale");
        }
        if el.attr("text:display-levels").and_then(|v| v.parse::<u32>().ok()).is_some_and(|v| v > 1) {
            add(out, "ODF compound list labels are retained in the source archive; editable numbering currently shows one level");
        }
        if el.name == "text:list-level-style-image" {
            add(out, "ODF image bullets are retained in the source archive; list bullet rendering is incomplete");
        }
        if el.name == "text:ruby" {
            add(out, "ODF ruby annotations are retained in the source archive; the base text remains editable");
        }
        if el.name == "text:list-level-style-number" {
            let prefix = el.attr("style:num-prefix").unwrap_or("");
            let suffix = el.attr("style:num-suffix").unwrap_or(".");
            if !matches!((prefix, suffix), ("", "" | "." | ")" | "-") | ("(", ")")) {
                add(out, "ODF custom list label prefixes/suffixes are retained in the source archive; the closest editable numbering scheme is used");
            }
        }
    }
    fn inspect_tree(el: &El, paragraph: bool, out: &mut Vec<&'static str>) {
        inspect(el, out);
        if !paragraph
            && (el.attr("style:line-height-at-least").is_some()
                || el.attr("style:line-spacing").is_some()
                || el.attr("fo:line-height") == Some("normal"))
        {
            add(out, "ODF minimum/additional line spacing in this style family is not mapped; source formatting is retained for recovery");
        }
        for child in el.elements() {
            inspect_tree(child, paragraph, out);
        }
    }
    fn style<'a>(
        family: &str,
        name: &'a str,
        styles: &Styles<'a>,
        seen: &mut std::collections::BTreeSet<(String, String)>,
        out: &mut Vec<&'static str>,
    ) {
        let mut next = Some(name);
        let mut check_line_height = family == "paragraph";
        for _ in 0..32 {
            let Some(name) = next else { break };
            let key = (family.into(), name.into());
            if !seen.insert(key.clone()) {
                break;
            }
            let Some(el) = styles.get(&key) else { break };
            if check_line_height {
                check_line_height = false;
                match crate::loss_report::paragraph_line_height(styles, (!name.is_empty()).then_some(name), |_| Ok(())) {
                    Ok(Ok(_)) => {}
                    Ok(Err(crate::loss_report::FeatureCode::InvalidValue)) => {
                        add(out, "ODF invalid minimum/additional line-height value is retained in the source archive; edited export is blocked");
                    }
                    _ => {
                        add(
                            out,
                            "ODF additional or combined line spacing has no verified model equivalent; source formatting is retained for recovery",
                        );
                    }
                }
            }
            inspect_tree(el, family == "paragraph", out);
            next = el.attr("style:parent-style-name");
        }
    }
    fn walk<'a>(el: &'a El, styles: &Styles<'a>, seen: &mut std::collections::BTreeSet<(String, String)>, out: &mut Vec<&'static str>) {
        inspect(el, out);
        let family = match el.name.as_str() {
            "text:p" | "text:h" => "paragraph",
            "text:list" => "list",
            _ => "text",
        };
        if let Some(name) = el.attr("text:style-name") {
            style(family, name, styles, seen, out);
        }
        if matches!(el.name.as_str(), "text:p" | "text:h") {
            style("paragraph", "", styles, seen, out);
            style("text", "", styles, seen, out);
        }
        if let Some(name) = el.attr("draw:text-style-name") {
            style("paragraph", name, styles, seen, out);
        }
        if el.name.starts_with("draw:") {
            for (family, key) in [("graphic", "draw:style-name"), ("presentation", "presentation:style-name")] {
                style(family, "", styles, seen, out);
                if let Some(name) = el.attr(key) {
                    style(family, name, styles, seen, out);
                }
            }
        }
        if el.name == "table:table-cell" {
            style("table-cell", "", styles, seen, out);
            if let Some(name) = el.attr("table:style-name") {
                style("table-cell", name, styles, seen, out);
            }
        }
        for child in el.elements() {
            walk(child, styles, seen, out);
        }
    }
    if shape_props.contains_key("style:line-height-at-least")
        || shape_props.contains_key("style:line-spacing")
        || shape_props.get("fo:line-height").is_some_and(|value| value == "normal")
    {
        add(&mut warnings, "ODF minimum/additional line spacing in this style family is not mapped; source formatting is retained for recovery");
    }
    if let Some(name) = el.attr("draw:text-style-name") {
        style("paragraph", name, styles, &mut seen, &mut warnings);
    }
    fn background_context(el: &El, styles: &Styles<'_>, base: Option<&ParagraphBackground>, out: &mut Vec<&'static str>) {
        let shape = if matches!(
            el.name.as_str(),
            "draw:frame"
                | "draw:custom-shape"
                | "draw:rect"
                | "draw:ellipse"
                | "draw:circle"
                | "draw:line"
                | "draw:connector"
                | "draw:path"
                | "draw:polygon"
                | "draw:polyline"
        ) {
            match shape_background(el, styles, base) {
                Ok(value) => value,
                Err(_) => {
                    add(
                        out,
                        "ODF conflicting or malformed paragraph background styles are retained in the source archive; editable background inheritance is incomplete",
                    );
                    base.cloned()
                }
            }
        } else {
            base.cloned()
        };
        let base = shape.as_ref();
        let resolved = if matches!(el.name.as_str(), "text:p" | "text:h") {
            paragraph_background_with_source(styles, "paragraph", el.attr("text:style-name"), base)
        } else if el.name == "table:table-cell" {
            // Cell text has its own style defaults; the containing table frame's text
            // style does not become a paragraph background on each cell.
            paragraph_background_with_source(styles, "table-cell", el.attr("table:style-name"), None)
        } else if let Some(name) = el.attr("draw:text-style-name") {
            paragraph_background_with_source(styles, "paragraph", Some(name), base)
        } else {
            Ok(base.cloned())
        };
        let resolved = match resolved {
            Ok(value) => value,
            Err(_) => {
                add(
                    out,
                    "ODF conflicting or malformed paragraph background styles are retained in the source archive; editable background inheritance is incomplete",
                );
                base.cloned()
            }
        };
        for child in el.elements() {
            background_context(child, styles, resolved.as_ref(), out);
        }
    }
    background_context(el, styles, None, &mut warnings);
    walk(el, styles, &mut seen, &mut warnings);
    warnings
}

fn style_columns<'a>(el: &'a El, styles: &Styles<'a>) -> Option<&'a El> {
    for (family, key) in [("graphic", "draw:style-name"), ("presentation", "presentation:style-name")] {
        let mut current = el.attr(key);
        for _ in 0..32 {
            let Some(name) = current else { break };
            let Some(style) = styles.get(&(family.into(), name.into())) else { break };
            if let Some(columns) = style.child("style:graphic-properties").and_then(|e| e.child("style:columns")) {
                return Some(columns);
            }
            current = style.attr("style:parent-style-name");
        }
        if let Some(columns) =
            styles.get(&(family.into(), String::new())).and_then(|s| s.child("style:graphic-properties")).and_then(|e| e.child("style:columns"))
        {
            return Some(columns);
        }
    }
    None
}

/// Table-cell text properties are inherited defaults, below explicit paragraph/run styles.
pub(super) fn table_text_with_style(el: &El, styles: &Styles<'_>, name: Option<&str>, paragraphs: &mut Vec<Paragraph>) -> Result<()> {
    let base = styled_run(styles, "table-cell", name, &RunProps::default(), true)?;
    let base_para = deckcraft_model::text::ParaProps {
        background: paragraph_background_with_source(styles, "table-cell", name, None)?.map(|value| value.fill),
        ..Default::default()
    };
    text_with(el, styles, paragraphs, 0, &base, &base_para)?;
    if paragraphs.is_empty()
        && let Some(value) = el
            .attr("office:string-value")
            .or_else(|| el.attr("office:value"))
            .or_else(|| el.attr("office:boolean-value"))
            .or_else(|| el.attr("office:date-value"))
            .or_else(|| el.attr("office:time-value"))
    {
        let paragraph = El::default();
        let mut out = paragraph_with(&paragraph, styles, 0, &mut 0, &base, &base_para)?;
        out.runs.push(Run::with(value, out.def_props.clone()));
        paragraphs.push(out);
    }
    Ok(())
}

/// Read one source-order cell text member without traversing adjacent tables.
#[derive(Default)]
pub(crate) struct TableTextState {
    budget: usize,
    lists: ListState,
}

pub(super) fn table_text_member(
    el: &El,
    styles: &Styles<'_>,
    name: Option<&str>,
    paragraphs: &mut Vec<Paragraph>,
    state: &mut TableTextState,
) -> Result<()> {
    let base = styled_run(styles, "table-cell", name, &RunProps::default(), true)?;
    let base_para = deckcraft_model::text::ParaProps {
        background: paragraph_background_with_source(styles, "table-cell", name, None)?.map(|value| value.fill),
        ..Default::default()
    };
    walk_text_elements(std::iter::once(el), styles, paragraphs, 0, None, false, &mut state.budget, &base, &base_para, &mut state.lists)
}
