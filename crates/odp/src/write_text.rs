//! Editable ODF text export and bounded rich typography.
use super::*;

// The first script-language slice uses ODF language/country pairs. More elaborate
// tags require script/RFC attributes and stay guarded until those consumers are proven.
fn script_language_attrs(language: &str, suffix: &str) -> Result<String> {
    if language.is_empty() {
        return Ok(format!(" style:language-{suffix}=\"none\" style:country-{suffix}=\"none\""));
    }
    let (lang, country) = language.split_once('-').map_or((language, None), |(lang, country)| (lang, Some(country)));
    if !(2..=8).contains(&lang.len())
        || !lang.bytes().all(|c| c.is_ascii_alphabetic())
        || country
            .is_some_and(|c| !(c.len() == 2 && c.bytes().all(|b| b.is_ascii_alphabetic()) || c.len() == 3 && c.bytes().all(|b| b.is_ascii_digit())))
    {
        return Err(OdpError("script language tag requires unsupported ODF script/RFC language attributes".into()));
    }
    Ok(format!(" style:language-{suffix}=\"{}\" style:country-{suffix}=\"{}\"", esc(lang), esc(country.unwrap_or("none"))))
}

fn script_attrs(props: &RunProps) -> Result<String> {
    let mut attrs = String::new();
    for (suffix, size, bold, italic, language) in [
        ("asian", props.size_ea, props.bold_ea, props.italic_ea, props.lang_ea.as_deref()),
        ("complex", props.size_cs, props.bold_cs, props.italic_cs, props.lang_cs.as_deref()),
    ] {
        if let Some(size) = size {
            styles::finite(&[size])?;
            if !(1.0..=4000.0).contains(&size) {
                return Err(OdpError("script font size exceeds supported range".into()));
            }
            attrs.push_str(&format!(" style:font-size-{suffix}=\"{size}pt\""));
        }
        if let Some(bold) = bold {
            attrs.push_str(&format!(" style:font-weight-{suffix}=\"{}\"", if bold { "bold" } else { "normal" }));
        }
        if let Some(italic) = italic {
            attrs.push_str(&format!(" style:font-style-{suffix}=\"{}\"", if italic { "italic" } else { "normal" }));
        }
        if let Some(language) = language {
            attrs.push_str(&script_language_attrs(language, suffix)?);
        }
    }
    Ok(attrs)
}

impl Writer<'_> {
    pub(super) fn text_content(&mut self, ctx: &Ctx<'_>, s: &Shape, body: &TextBody, out: &mut String) -> Result<()> {
        let mut markers = self.comment_markers.remove(&s.id);
        if body.paragraphs.is_empty() {
            if let Some(markers) = markers {
                out.push_str("<text:p>");
                markers.finish(out)?;
                out.push_str("</text:p>");
            }
            return Ok(());
        }
        let mut counters = [None::<u32>; 9];
        for (paragraph_index, p) in body.paragraphs.iter().enumerate() {
            if paragraph_index > 0
                && let Some(markers) = &mut markers
            {
                markers.advance(1)?;
            }
            let para = resolve::para(ctx, s, p);
            if para.line_height.is_some() && para.line_spacing.is_some() {
                return Err(OdpError("conflicting native paragraph line-height and line-spacing settings".into()));
            }
            if para.line_height.is_some_and(|policy| !policy.valid()) {
                return Err(OdpError("invalid native paragraph line-height policy".into()));
            }
            if self.next >= 200_000 {
                return Err(OdpError("too many text styles to export".into()));
            }
            for v in [para.margin_left, para.margin_right, para.indent].into_iter().flatten() {
                styles::finite(&[v])?;
            }
            for v in [para.space_before, para.space_after, para.line_spacing].into_iter().flatten() {
                let v = match v {
                    deckcraft_model::text::Spacing::Pts(v) | deckcraft_model::text::Spacing::Pct(v) => v,
                };
                styles::finite(&[v])?;
            }
            let align = match para.align {
                Some(Align::Center) => "center",
                Some(Align::Right) => "right",
                Some(Align::Justify | Align::JustLow | Align::Distributed) => "justify",
                _ => "left",
            };
            let mut para_props = format!("fo:text-align=\"{align}\"");
            if let Some(background) = &para.background {
                let value = match background {
                    Fill::None => "transparent".into(),
                    Fill::Solid { color } => {
                        let color = ctx.color(color, None);
                        if color.a != 255 {
                            return Err(OdpError("paragraph background opacity cannot be represented faithfully in ODP yet".into()));
                        }
                        format!("#{}", color.hex())
                    }
                    _ => return Err(OdpError("paragraph background paint cannot be represented faithfully in ODP yet".into())),
                };
                para_props.push_str(&format!(" fo:background-color=\"{value}\""));
            }
            for (key, value) in [("fo:margin-left", para.margin_left), ("fo:margin-right", para.margin_right), ("fo:text-indent", para.indent)] {
                if let Some(value) = value {
                    para_props.push_str(&format!(" {key}=\"{value}pt\""));
                }
            }
            for (key, value) in [("fo:margin-top", para.space_before), ("fo:margin-bottom", para.space_after), ("fo:line-height", para.line_spacing)]
            {
                if let Some(value) = value {
                    let value = match value {
                        deckcraft_model::text::Spacing::Pts(v) => format!("{v}pt"),
                        deckcraft_model::text::Spacing::Pct(v) => format!("{}%", v * 100.0),
                    };
                    para_props.push_str(&format!(" {key}=\"{value}\""));
                }
            }
            if let Some(policy) = para.line_height {
                use deckcraft_model::text::LineHeight;
                match policy {
                    LineHeight::Normal => para_props.push_str(" fo:line-height=\"normal\""),
                    LineHeight::Minimum { points } => para_props.push_str(&format!(" style:line-height-at-least=\"{points}pt\"")),
                }
            }
            if para.rtl == Some(true) {
                para_props.push_str(" style:writing-mode=\"rl-tb\"");
            }
            if let Some(distance) = para.default_tab {
                styles::finite(&[distance])?;
                if distance <= 0.0 {
                    return Err(OdpError("non-positive default tab distance".into()));
                }
                para_props.push_str(&format!(" style:tab-stop-distance=\"{distance}pt\""));
            }
            if let Some(value) = para.latin_line_break {
                para_props.push_str(&format!(" style:line-break=\"{}\"", if value { "normal" } else { "strict" }));
            }
            if let Some(value) = para.hanging_punct {
                para_props.push_str(&format!(" style:punctuation-wrap=\"{}\"", if value { "hanging" } else { "simple" }));
            }
            let mut stops = String::new();
            if let Some(tabs) = &para.tabs {
                if tabs.len() > 128 {
                    return Err(OdpError("too many text tab stops".into()));
                }
                stops.push_str("<style:tab-stops>");
                for tab in tabs {
                    styles::finite(&[tab.pos])?;
                    if tab.pos < 0.0 {
                        return Err(OdpError("negative text tab position".into()));
                    }
                    let kind = match tab.align.as_str() {
                        "l" => "left",
                        "r" => "right",
                        "ctr" => "center",
                        "dec" => "char",
                        _ => return Err(OdpError("unknown tab alignment".into())),
                    };
                    stops.push_str(&format!(
                        "<style:tab-stop style:position=\"{}pt\" style:type=\"{kind}\"{}/>",
                        tab.pos,
                        if kind == "char" { " style:char=\".\"" } else { "" }
                    ));
                }
                stops.push_str("</style:tab-stops>");
            }
            let defaults = resolve::run(ctx, s, p, &RunProps::default());
            let end = resolve::run(ctx, s, p, &p.end_props);
            let default_family = resolve::font_family(ctx, &defaults);
            if resolve::font_family(ctx, &end) != default_family || end.size != defaults.size {
                return Err(OdpError("distinct paragraph-end Latin font or size cannot be represented faithfully in ODP yet".into()));
            }
            let size = defaults.size.unwrap_or(18.0);
            styles::finite(&[size])?;
            if size <= 0.0 {
                return Err(OdpError("non-positive paragraph default text size".into()));
            }
            // Paragraph defaults also govern empty paragraphs and subsequently typed text;
            // styled spans alone cannot preserve their Latin font/size. Keep the existing
            // script end-marker mapping, but refuse distinct Latin end-only formatting.
            let marker =
                format!("<style:text-properties fo:font-family=\"{}\" fo:font-size=\"{size}pt\"{}/>", esc(&default_family), script_attrs(&end)?);
            let style = self.style("paragraph", &format!("<style:paragraph-properties {para_props}>{stops}</style:paragraph-properties>{marker}"));
            let list_depth = if matches!(para.bullet, Some(Bullet::Char { .. } | Bullet::AutoNum { .. })) { p.level.min(8) as usize + 1 } else { 0 };
            let level = usize::from(p.level.min(8));
            let list_number = if let Some(Bullet::AutoNum { start_at, .. }) = &para.bullet {
                let next = para
                    .number_restart
                    .unwrap_or_else(|| counters.get(level).copied().flatten().map(|n| n.saturating_add(1)).unwrap_or((*start_at).max(1)))
                    .max(1);
                if !p.is_empty() {
                    if let Some(counter) = counters.get_mut(level) {
                        *counter = Some(next);
                    }
                    for counter in counters.iter_mut().skip(level + 1) {
                        *counter = None;
                    }
                } else if para.number_restart.is_some()
                    && let Some(counter) = counters.get_mut(level)
                {
                    *counter = Some(next.saturating_sub(1));
                }
                Some(next)
            } else {
                if !p.is_empty() {
                    for counter in counters.iter_mut().skip(level) {
                        *counter = None;
                    }
                }
                None
            };
            if list_depth > 0 {
                self.next += 1;
                let list = format!("L{}", self.next);
                self.styles.push_str(&format!("<text:list-style style:name=\"{list}\">"));
                for level in 1..=list_depth {
                    let mut level_props = if let Some(left) = para.margin_left {
                        format!(
                            "<style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" fo:margin-left=\"{left}pt\" fo:text-indent=\"{}pt\"/></style:list-level-properties>",
                            para.indent.unwrap_or(0.0)
                        )
                    } else {
                        String::new()
                    };
                    if para.bullet_font.is_some() || para.bullet_color.is_some() {
                        let mut attrs = String::new();
                        if let Some(font) = &para.bullet_font {
                            attrs.push_str(&format!(" fo:font-family=\"{}\"", esc(font)));
                        }
                        if let Some(color) = &para.bullet_color {
                            let color = ctx.color(color, None);
                            attrs.push_str(&format!(" fo:color=\"#{}\"", color.hex()));
                        }
                        level_props.push_str(&format!("<style:text-properties{attrs}/>"));
                    }
                    if let Some(size) = para.bullet_size {
                        styles::finite(&[size])?;
                        if size <= 0.0 {
                            return Err(OdpError("non-positive bullet relative size".into()));
                        }
                        if level_props.contains("<style:list-level-properties ") {
                            level_props = level_props.replacen(
                                "<style:list-level-properties ",
                                &format!("<style:list-level-properties text:bullet-relative-size=\"{}%\" ", size * 100.0),
                                1,
                            );
                        } else {
                            level_props.insert_str(0, &format!("<style:list-level-properties text:bullet-relative-size=\"{}%\"/>", size * 100.0));
                        }
                    }
                    match &para.bullet {
                        Some(Bullet::Char {char})=>self.styles.push_str(&format!("<text:list-level-style-bullet text:level=\"{level}\" text:bullet-char=\"{}\">{level_props}</text:list-level-style-bullet>",esc(char))),
                        Some(Bullet::AutoNum {scheme,..})=>{
                            let format=if scheme.starts_with("alphaUc") {"A"} else if scheme.starts_with("alphaLc") {"a"} else if scheme.starts_with("romanUc") {"I"} else if scheme.starts_with("romanLc") {"i"} else {"1"};
                            let suffix=if scheme.contains("Paren") { ")" } else if scheme.ends_with("Plain") {""} else if scheme.ends_with("Minus") {"-"} else { "." };
                            let prefix=if scheme.ends_with("ParenBoth") {"("} else {""};
                            let start=list_number.unwrap_or(1);
                            self.styles.push_str(&format!("<text:list-level-style-number text:level=\"{level}\" style:num-format=\"{format}\" style:num-prefix=\"{prefix}\" style:num-suffix=\"{suffix}\" text:start-value=\"{start}\">{level_props}</text:list-level-style-number>"));
                        }
                        _=>{}
                    }
                }
                self.styles.push_str("</text:list-style>");
                for _ in 0..list_depth {
                    out.push_str(&format!("<text:list text:style-name=\"{list}\"><text:list-item>"));
                }
            } else if matches!(para.bullet, Some(Bullet::Picture { .. })) {
                return Err(OdpError("picture bullets need an image list style".into()));
            }
            let target = if s.id.0 != 0 { format!(" xml:id=\"{}-p{paragraph_index}\"", self.shape_identity(s.id)) } else { String::new() };
            out.push_str(&format!("<text:p{target} text:style-name=\"{style}\">"));
            for run in &p.runs {
                let props = resolve::run(ctx, s, p, &run.props);
                for v in [props.size, props.spacing, props.baseline].into_iter().flatten() {
                    styles::finite(&[v])?;
                }
                if props.size.is_some_and(|v| v <= 0.0) {
                    return Err(OdpError("non-positive text size".into()));
                }
                let c = resolve::text_color(ctx, &props);
                if c.a != 255 {
                    return Err(OdpError("transparent text color cannot be represented faithfully in ODP yet".into()));
                }
                if props.fill.as_ref().is_some_and(|fill| !matches!(fill, Fill::Solid { .. }))
                    || props.outline.is_some()
                    || props.shadow.is_some()
                    || props.inner_shadow.is_some()
                    || props.glow.is_some()
                    || props.reflection.is_some()
                    || props.soft_edge.is_some()
                {
                    return Err(OdpError("this rich text fill or effect cannot be represented faithfully in ODP yet".into()));
                }
                let mut extras = underline_attrs(props.underline.as_deref())?;
                for (key, font) in [("style:font-family-asian", &props.font_ea), ("style:font-family-complex", &props.font_cs)] {
                    if let Some(font) = font {
                        extras.push_str(&format!(" {key}=\"{}\"", esc(font)));
                    }
                }
                extras.push_str(&script_attrs(&props)?);
                if let Some(color) = &props.underline_color {
                    let color = ctx.color(color, None);
                    extras.push_str(&format!(" style:text-underline-color=\"#{}\"", color.hex()));
                }
                if let Some(color) = &props.highlight {
                    let color = ctx.color(color, None);
                    if color.a == 0 {
                        extras.push_str(" fo:background-color=\"transparent\"");
                    } else if color.a == 255 {
                        extras.push_str(&format!(" fo:background-color=\"#{}\"", color.hex()));
                    } else {
                        return Err(OdpError("partly transparent character highlighting cannot be represented faithfully in ODP".into()));
                    }
                }
                if let Some(kern) = props.kern {
                    styles::finite(&[kern])?;
                    if kern > 0.0 && kern != 10000.0 && (run.props.kern.is_some() || p.def_props.kern.is_some()) {
                        return Err(OdpError("font-size-dependent kerning thresholds cannot be represented faithfully in ODP yet".into()));
                    }
                    extras.push_str(&format!(" style:letter-kerning=\"{}\"", if kern <= props.size.unwrap_or(18.0) { "true" } else { "false" }));
                }
                if let Some(lang) = &props.lang {
                    if lang.is_empty() {
                        extras.push_str(" fo:language=\"none\" fo:country=\"none\"");
                    } else {
                        let (language, country) = lang.split_once('-').map(|(a, b)| (a, Some(b))).unwrap_or((lang, None));
                        extras.push_str(&format!(" fo:language=\"{}\"", esc(language)));
                        if let Some(country) = country {
                            extras.push_str(&format!(" fo:country=\"{}\"", esc(country)));
                        }
                    }
                }
                if props.caps == Some(deckcraft_model::text::Caps::Small) {
                    extras.push_str(" fo:font-variant=\"small-caps\"");
                }

                if let Some(spacing) = props.spacing {
                    extras.push_str(&format!(" fo:letter-spacing=\"{spacing}pt\""));
                }
                if let Some(baseline) = props.baseline {
                    extras.push_str(&format!(" style:text-position=\"{}% {}%\"", baseline * 100.0, if baseline == 0.0 { 100 } else { 66 }));
                }
                if let Some(strike) = props.strike {
                    extras.push_str(&format!(
                        " style:text-line-through-style=\"{}\"",
                        if strike == deckcraft_model::text::Strike::None { "none" } else { "solid" }
                    ));
                    extras.push_str(&format!(
                        " style:text-line-through-type=\"{}\"",
                        if strike == deckcraft_model::text::Strike::Double { "double" } else { "single" }
                    ));
                }
                if let Some(caps) = props.caps {
                    extras
                        .push_str(&format!(" fo:text-transform=\"{}\"", if caps == deckcraft_model::text::Caps::All { "uppercase" } else { "none" }));
                }
                let style = self.style("text", &format!("<style:text-properties fo:font-family=\"{}\" fo:font-size=\"{}pt\" fo:font-weight=\"{}\" fo:font-style=\"{}\" fo:color=\"#{}\" {extras}/>", esc(&resolve::font_family(ctx, &props)), props.size.unwrap_or(18.0), if props.bold == Some(true) { "bold" } else { "normal" }, if props.italic == Some(true) { "italic" } else { "normal" }, c.hex()));
                let link = props.link.as_ref().map(|l| objects::link_href(self.pres, l)).transpose()?;
                if let Some(url) = &link {
                    out.push_str(&format!(
                        "<text:a xlink:href=\"{}\" xlink:type=\"simple\" office:title=\"{}\">",
                        esc(url),
                        esc(props.link.as_ref().map(|l| l.tooltip.as_str()).unwrap_or(""))
                    ));
                }
                out.push_str(&format!("<text:span text:style-name=\"{style}\">"));
                if let Some(markers) = &mut markers {
                    markers.flush(out)?;
                }
                match &run.kind {
                    RunKind::Text => {
                        if let Some(markers) = &mut markers {
                            markers.text(&run.text, out)?;
                        } else {
                            out.push_str(&text(&run.text));
                        }
                    }
                    RunKind::Break => {
                        out.push_str("<text:line-break/>");
                        if let Some(markers) = &mut markers {
                            markers.advance(1)?;
                        }
                    }
                    RunKind::Field { field } => {
                        let value = match field.as_str() {
                            "slidenum" => format!("<text:page-number>{}</text:page-number>", text(&run.text)),
                            "header" => "<presentation:header/>".into(),
                            "footer" => "<presentation:footer/>".into(),
                            "notesdatetime" => "<presentation:date-time/>".into(),
                            _ if field.starts_with("datetime") => format!("<text:date>{}</text:date>", text(&run.text)),
                            _ => return Err(OdpError(format!("field '{field}' cannot be exported faithfully yet"))),
                        };
                        if let Some(markers) = &mut markers {
                            markers.advance(
                                u32::try_from(run.text.chars().count()).map_err(|_| OdpError("comment field display exceeds limit".into()))?,
                            )?;
                        }
                        out.push_str(&value);
                    }
                    RunKind::Math { .. } => return Err(OdpError("OMML equations cannot be exported as editable ODF formulas yet".into())),
                }
                out.push_str("</text:span>");
                if link.is_some() {
                    out.push_str("</text:a>");
                }
            }
            if let Some(markers) = &mut markers {
                markers.flush(out)?;
            }
            out.push_str("</text:p>");
            for _ in 0..list_depth {
                out.push_str("</text:list-item></text:list>");
            }
        }
        if let Some(markers) = markers {
            markers.finish(out)?;
        }
        Ok(())
    }
}

fn underline_attrs(value: Option<&str>) -> Result<String> {
    let value = value.unwrap_or("none");
    let (style, kind, width, mode) = match value {
        "none" => ("none", "single", "auto", "continuous"),
        "sng" => ("solid", "single", "auto", "continuous"),
        "dbl" => ("solid", "double", "auto", "continuous"),
        "heavy" => ("solid", "single", "bold", "continuous"),
        "words" => ("solid", "single", "auto", "skip-white-space"),
        "dotted" => ("dotted", "single", "auto", "continuous"),
        "dottedHeavy" => ("dotted", "single", "bold", "continuous"),
        "dash" => ("dash", "single", "auto", "continuous"),
        "dashHeavy" => ("dash", "single", "bold", "continuous"),
        "dashLong" => ("long-dash", "single", "auto", "continuous"),
        "dashLongHeavy" => ("long-dash", "single", "bold", "continuous"),
        "dotDash" => ("dot-dash", "single", "auto", "continuous"),
        "dotDashHeavy" => ("dot-dash", "single", "bold", "continuous"),
        "dotDotDash" => ("dot-dot-dash", "single", "auto", "continuous"),
        "dotDotDashHeavy" => ("dot-dot-dash", "single", "bold", "continuous"),
        "wavy" => ("wave", "single", "auto", "continuous"),
        "wavyHeavy" => ("wave", "single", "bold", "continuous"),
        "wavyDbl" => ("wave", "double", "auto", "continuous"),
        _ => return Err(OdpError(format!("underline '{value}' cannot be represented faithfully in ODP yet"))),
    };
    Ok(format!(
        "style:text-underline-style=\"{style}\" style:text-underline-type=\"{kind}\" style:text-underline-width=\"{width}\" style:text-underline-mode=\"{mode}\""
    ))
}

impl Writer<'_> {
    pub(super) fn text_body_style(&self, body: &deckcraft_model::text::BodyProps) -> Result<(String, String)> {
        use deckcraft_model::text::{AutoFit, TextDir};
        let mut attrs = String::new();
        match body.autofit.unwrap_or(AutoFit::None) {
            AutoFit::None => attrs.push_str("draw:auto-grow-height=\"false\" style:shrink-to-fit=\"false\""),
            AutoFit::Shape => attrs.push_str("draw:auto-grow-height=\"true\" style:shrink-to-fit=\"false\""),
            AutoFit::Shrink { font_scale, line_reduction } => {
                styles::finite(&[font_scale.unwrap_or(1.0), line_reduction])?;
                if font_scale.is_some_and(|v| (v - 1.0).abs() > 1e-9) || line_reduction != 0.0 {
                    return Err(OdpError("pre-scaled text autofit cannot be represented faithfully in ODP yet".into()));
                }
                attrs.push_str("draw:auto-grow-height=\"false\" style:shrink-to-fit=\"true\"");
            }
        }
        let mode = match body.vert.unwrap_or(TextDir::Horizontal) {
            TextDir::Horizontal => {
                if body.rtl_col == Some(true) {
                    "rl-tb"
                } else {
                    "lr-tb"
                }
            }
            TextDir::EaVertical => "tb-rl",
            _ => return Err(OdpError("this vertical text orientation cannot be represented faithfully in ODP yet".into())),
        };
        attrs.push_str(&format!(" style:writing-mode=\"{mode}\""));
        if let Some(center) = body.anchor_ctr {
            attrs.push_str(&format!(" draw:textarea-horizontal-align=\"{}\"", if center { "center" } else { "left" }));
        }
        if body.rot.is_some_and(|v| v != 0.0) {
            return Err(OdpError("independent text rotation needs enhanced geometry export support".into()));
        }
        if body.warp.is_some() || !body.warp_adj.is_empty() {
            return Err(OdpError("text warp cannot be represented faithfully in ODP yet".into()));
        }
        let columns = body.columns.unwrap_or(1);
        let gap = body.col_spacing.unwrap_or(0.0);
        styles::finite(&[gap])?;
        if !(1..=16).contains(&columns) || gap < 0.0 {
            return Err(OdpError("invalid text column settings".into()));
        }
        let children = if body.columns.is_some() || body.col_spacing.is_some() {
            format!("<style:columns fo:column-count=\"{columns}\" fo:column-gap=\"{gap}pt\"/>")
        } else {
            String::new()
        };
        Ok((attrs, children))
    }
}
