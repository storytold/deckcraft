//! Bounded ODF data styles and a scoped, projection-bound native format program.
use crate::{
    OdpError, Result,
    read::Styles,
    xml::{self, El, Node, esc},
};
use deckcraft_model::table::{DateFormatPart, NegativeDisplay, NumberComparison, NumberFormat, NumberFormatSection, NumberLocale, NumberStyle};

pub(crate) const FORMAT_URI: &str = "urn:deckcraft:table:number-format:1";
const MAX_PROGRAM: usize = 256 * 1024;

fn attribute(value: &str) -> String {
    esc(value).replace('\t', "&#x9;").replace('\n', "&#xA;").replace('\r', "&#xD;")
}
fn literal(value: &str) -> String {
    if value.is_empty() { String::new() } else { format!("<number:text>{}</number:text>", esc(value)) }
}
fn color(value: Option<[u8; 4]>) -> String {
    value.map(|[r, g, b, _]| format!("<style:text-properties fo:color=\"#{r:02x}{g:02x}{b:02x}\"/>")).unwrap_or_default()
}
fn condition(comparison: NumberComparison, threshold: &str) -> String {
    let op = match comparison {
        NumberComparison::Less => "<",
        NumberComparison::LessEqual => "<=",
        NumberComparison::Equal => "=",
        NumberComparison::NotEqual => "!=",
        NumberComparison::GreaterEqual => ">=",
        NumberComparison::Greater => ">",
    };
    format!("value(){op}{threshold}")
}
fn kind(style: &NumberStyle) -> &'static str {
    match style {
        NumberStyle::Percentage { .. } => "percentage-style",
        NumberStyle::Currency { .. } => "currency-style",
        NumberStyle::DateTime { .. } => "date-style",
        NumberStyle::Duration { .. } => "time-style",
        NumberStyle::Boolean { .. } => "boolean-style",
        NumberStyle::Text => "text-style",
        _ => "number-style",
    }
}
fn number(dp: u8, min: u8, integers: u8, grouping: bool) -> String {
    format!(
        "<number:number number:decimal-places=\"{dp}\" number:min-decimal-places=\"{min}\" number:min-integer-digits=\"{integers}\" number:grouping=\"{grouping}\"/>"
    )
}
fn body(style: &NumberStyle) -> String {
    match style {
        // General has no exact ODF data-style equivalent. Its visible fallback has
        // maximum native decimal scale; the extension retains canonical semantics.
        NumberStyle::General => {
            "<number:number number:decimal-places=\"96\" number:min-decimal-places=\"0\" number:min-integer-digits=\"1\"/>".into()
        }
        NumberStyle::Number { decimal_places, min_decimal_places, min_integer_digits, grouping } => {
            number(*decimal_places, *min_decimal_places, *min_integer_digits, *grouping)
        }
        NumberStyle::Percentage { decimal_places, min_decimal_places, grouping } => {
            format!("{}{}", number(*decimal_places, *min_decimal_places, 1, *grouping), literal("%"))
        }
        NumberStyle::Currency { decimal_places, min_decimal_places, grouping, code, symbol, symbol_after, space } => {
            let currency = format!("<number:currency-symbol>{}</number:currency-symbol>", esc(if symbol.is_empty() { code } else { symbol }));
            let n = number(*decimal_places, *min_decimal_places, 1, *grouping);
            let gap = literal(if *space { " " } else { "" });
            if *symbol_after { format!("{n}{gap}{currency}") } else { format!("{currency}{gap}{n}") }
        }
        NumberStyle::Scientific { decimal_places, min_exponent_digits, exponent_sign } => format!(
            "<number:scientific-number number:decimal-places=\"{decimal_places}\" number:min-integer-digits=\"1\" number:min-exponent-digits=\"{min_exponent_digits}\" number:forced-exponent-sign=\"{exponent_sign}\"/>"
        ),
        NumberStyle::Fraction { denominator_digits, fixed_denominator } => {
            let denominator = fixed_denominator
                .map(|d| format!(" number:denominator-value=\"{d}\""))
                .unwrap_or_else(|| format!(" number:max-denominator-value=\"{}\"", 10u32.pow(u32::from(*denominator_digits)).saturating_sub(1)));
            format!(
                "<number:fraction number:min-integer-digits=\"1\" number:min-numerator-digits=\"1\" number:min-denominator-digits=\"1\"{denominator}/>"
            )
        }
        NumberStyle::Boolean { .. } => "<number:boolean/>".into(),
        NumberStyle::Text => "<number:text-content/>".into(),
        NumberStyle::DateTime { parts } | NumberStyle::Duration { parts } => parts
            .iter()
            .map(|part| match part {
                DateFormatPart::Literal { text } => literal(text),
                DateFormatPart::Zone => literal("Z"), // native offset remains in the typed date and visible paragraphs
                DateFormatPart::AmPm => "<number:am-pm/>".into(),
                DateFormatPart::Year { long } => date_part("year", *long, ""),
                DateFormatPart::Month { long, textual } => date_part("month", *long, &format!(" number:textual=\"{textual}\"")),
                DateFormatPart::Day { long } => date_part("day", *long, ""),
                DateFormatPart::DayOfWeek { long } => date_part("day-of-week", *long, ""),
                DateFormatPart::Hours { long, .. } => date_part("hours", *long, ""),
                DateFormatPart::Minutes { long } => date_part("minutes", *long, ""),
                DateFormatPart::Seconds { long, decimal_places } => {
                    date_part("seconds", *long, &format!(" number:decimal-places=\"{decimal_places}\""))
                }
            })
            .collect(),
    }
}
fn date_part(name: &str, long: bool, extra: &str) -> String {
    format!("<number:{name} number:style=\"{}\"{extra}/>", if long { "long" } else { "short" })
}

/// All style definitions are regenerated from the validated format. Auxiliary
/// names are deterministic and part of the proof, never dangling map targets.
pub(crate) fn project(format: &NumberFormat, name: &str, metadata: bool) -> Result<Vec<(String, String)>> {
    format.metadata_bytes().map_err(|e| OdpError(e.to_string()))?;
    if name.len() > 256 {
        return Err(OdpError("ODF data-style identity exceeds limit".into()));
    }
    let mut definitions = Vec::new();
    let mut maps = String::new();
    for (index, section) in format.sections.iter().enumerate() {
        let target = format!("{name}_s{index}");
        let child = NumberFormat {
            name: String::new(),
            locale: format.locale,
            style: section.style.clone(),
            prefix: section.prefix.clone(),
            suffix: section.suffix.clone(),
            negative: section.negative,
            color: section.color,
            ..Default::default()
        };
        // Native conditions are first-match, as are ODF style maps.
        definitions.extend(project(&child, &target, false)?);
        maps.push_str(&format!(
            "<style:map style:condition=\"{}\" style:apply-style-name=\"{}\"/>",
            attribute(&condition(section.comparison, &section.threshold)),
            esc(&target)
        ));
    }
    if let Some(zero) = &format.zero_display {
        let target = format!("{name}_zero");
        definitions.push((
            target.clone(),
            format!(
                "<number:number-style style:name=\"{}\" number:language=\"{}\" number:country=\"{}\">{}{}</number:number-style>",
                esc(&target),
                format.locale.language(),
                format.locale.country(),
                color(
                    format
                        .format_value(&deckcraft_model::table::CellValue::Number {
                            kind: deckcraft_model::table::CellNumberKind::Float,
                            lexical: "0".into()
                        })
                        .map_err(|e| OdpError(e.to_string()))?
                        .color
                ),
                literal(zero)
            ),
        ));
        maps = format!("<style:map style:condition=\"value()=0\" style:apply-style-name=\"{}\"/>{maps}", esc(&target));
    }
    if format.negative == NegativeDisplay::Parentheses {
        let target = format!("{name}_negative");
        let mut negative = format.clone();
        negative.negative = NegativeDisplay::Minus;
        negative.prefix = format!("({}", negative.prefix);
        negative.suffix.push(')');
        negative.sections.clear();
        negative.zero_display = None;
        // A conditional negative decimal style formats the magnitude, rather
        // than relying on an application's implicit sign convention.
        let xml = single(&negative, &target, "", None)?.replace("<number:number ", "<number:number number:display-factor=\"-1\" ");
        definitions.push((target.clone(), xml));
        maps.push_str(&format!("<style:map style:condition=\"value()&lt;0\" style:apply-style-name=\"{}\"/>", esc(&target)));
    }
    let program =
        if metadata { Some(serde_json::to_string(format).map_err(|_| OdpError("cannot encode number-format program".into()))?) } else { None };
    definitions.insert(0, (name.into(), single(format, name, &maps, program.as_deref())?));
    Ok(definitions)
}
fn single(format: &NumberFormat, name: &str, maps: &str, program: Option<&str>) -> Result<String> {
    let extension = program.map(|p| format!(" xmlns:dcfmt=\"{FORMAT_URI}\" dcfmt:program=\"{}\"", attribute(p))).unwrap_or_default();
    let overflow = if matches!(format.style, NumberStyle::Duration { .. }) { " number:truncate-on-overflow=\"false\"" } else { "" };
    let tag = kind(&format.style);
    // ODF §16.29.1 forbids adjacent number:text nodes. Coalesce only
    // generated tag boundaries; user literals have already been XML escaped.
    let content = format!("{}{}{}", literal(&format.prefix), body(&format.style), literal(&format.suffix)).replace("</number:text><number:text>", "");
    Ok(format!(
        "<number:{tag} style:name=\"{}\" number:language=\"{}\" number:country=\"{}\"{overflow}{extension}>{}{content}{maps}</number:{tag}>",
        esc(name),
        format.locale.language(),
        format.locale.country(),
        color(format.color)
    ))
}

fn program(el: &El) -> Result<Option<NumberFormat>> {
    let attrs: Vec<_> = el.expanded_attrs.iter().filter(|a| a.name.namespace.as_deref() == Some(FORMAT_URI)).collect();
    if attrs.is_empty() {
        return Ok(None);
    }
    if attrs.len() != 1 || attrs.first().is_none_or(|a| a.name.local.as_ref() != "program") {
        return Err(OdpError("unknown number-format extension field".into()));
    }
    let Some(value) = attrs.first().and_then(|a| el.attr(&a.projected_key)) else { return Err(OdpError("missing number-format program".into())) };
    if value.len() > MAX_PROGRAM {
        return Err(OdpError("number-format program exceeds limit".into()));
    }
    let format: NumberFormat = serde_json::from_str(value).map_err(|_| OdpError("invalid number-format program".into()))?;
    format.metadata_bytes().map_err(|e| OdpError(e.to_string()))?;
    Ok(Some(format))
}
fn equal_projection(actual: &El, expected: &El, root: bool) -> bool {
    if actual.expanded_name != expected.expanded_name {
        return false;
    }
    fn attrs(el: &El, root: bool) -> Vec<(&xml::ExpandedName, Option<&str>)> {
        el.expanded_attrs
            .iter()
            .filter(|a| !(root && a.name.namespace.as_deref() == Some(FORMAT_URI)))
            .map(|a| (&a.name, el.attr(&a.projected_key)))
            .collect::<Vec<_>>()
    }
    let mut a = attrs(actual, root);
    let mut b = attrs(expected, root);
    a.sort_by(|a, b| a.0.cmp(b.0));
    b.sort_by(|a, b| a.0.cmp(b.0));
    if a != b {
        return false;
    }
    fn children(el: &El) -> Vec<&Node> {
        el.children
            .iter()
            .filter(|n| !matches!(n,Node::Text(t) if t.trim().is_empty() && !matches!(el.name.as_str(), "number:text" | "number:currency-symbol")))
            .collect()
    }
    let a = children(actual);
    let b = children(expected);
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (Node::Text(a), Node::Text(b)) => a == b,
            (Node::El(a), Node::El(b)) => equal_projection(a, b, false),
            _ => false,
        })
}
fn projected_el(xml: &str) -> Result<El> {
    let mut doc = xml::header("office:document-styles");
    doc.push('>');
    doc.push_str(xml);
    doc.push_str("</office:document-styles>");
    let mut root = xml::parse(doc.as_bytes())?;
    match root.children.pop() {
        Some(Node::El(el)) => Ok(el),
        _ => Err(OdpError("invalid generated number style".into())),
    }
}

/// Only the exact versioned attribute on a complete matching standard style is
/// exempt from the generic extension diagnostic. Aliases use expanded identity.
pub(crate) fn supported_extension(el: &El) -> bool {
    let Ok(Some(format)) = program(el) else { return false };
    let Some(name) = el.attr("style:name") else { return false };
    let Ok(styles) = project(&format, name, false) else { return false };
    let Some((_, xml)) = styles.first() else { return false };
    projected_el(xml).is_ok_and(|expected| equal_projection(el, &expected, true))
}
pub(crate) fn has_program(el: &El) -> bool {
    el.expanded_attrs.iter().any(|a| a.name.namespace.as_deref() == Some(FORMAT_URI))
}

pub(crate) fn read(styles: &Styles<'_>, name: &str) -> Result<NumberFormat> {
    let el = styles.get(&("data".into(), name.into())).ok_or_else(|| OdpError("missing ODF data style".into()))?;
    if let Some(format) = program(el)? {
        for (identity, xml) in project(&format, name, false)? {
            let actual = styles.get(&("data".into(), identity)).ok_or_else(|| OdpError("missing number-format projection style".into()))?;
            let expected = projected_el(&xml)?;
            if !equal_projection(actual, &expected, true) {
                return Err(OdpError("number-format standard projection does not match program".into()));
            }
        }
        return Ok(format);
    }
    let mut result = plain(el)?;
    for map in el.elements().filter(|e| e.name == "style:map") {
        if result.sections.len() >= 16 || map.attrs.len() != 2 || map.elements().next().is_some() {
            return Err(OdpError("unsupported data-style map".into()));
        }
        let mut condition = map.attr("style:condition").ok_or_else(|| OdpError("missing data-style condition".into()))?;
        if let Some((prefix, expression)) = condition.split_once(':') {
            let qname = format!("{prefix}:condition");
            if map.resolve_qname(&qname).is_none_or(|(uri, _)| uri != Some("urn:oasis:names:tc:opendocument:xmlns:of:1.2")) {
                return Err(OdpError("unsupported data-style condition namespace".into()));
            }
            condition = expression;
        }
        let expr = condition.strip_prefix("value()").ok_or_else(|| OdpError("unsupported data-style condition".into()))?;
        let (comparison, threshold) = [
            ("<=", NumberComparison::LessEqual),
            (">=", NumberComparison::GreaterEqual),
            ("!=", NumberComparison::NotEqual),
            ("<", NumberComparison::Less),
            (">", NumberComparison::Greater),
            ("=", NumberComparison::Equal),
        ]
        .into_iter()
        .find_map(|(op, c)| expr.strip_prefix(op).map(|v| (c, v.trim())))
        .ok_or_else(|| OdpError("unsupported data-style comparison".into()))?;
        let target = map
            .attr("style:apply-style-name")
            .and_then(|n| styles.get(&("data".into(), n.into())))
            .ok_or_else(|| OdpError("missing data-style map target".into()))?;
        if target.elements().any(|e| e.name == "style:map") {
            return Err(OdpError("recursive data-style maps require additional mapping".into()));
        }
        let mut child = plain(target)?;
        if matches!(comparison, NumberComparison::Less | NumberComparison::LessEqual)
            && NumberFormat::default().format_number(threshold).is_ok_and(|value| value == "0")
            && child.prefix.starts_with('(')
            && child.suffix.ends_with(')')
        {
            child.prefix.remove(0);
            child.suffix.pop();
            child.negative = NegativeDisplay::Parentheses;
        }
        result.sections.push(NumberFormatSection {
            comparison,
            threshold: threshold.into(),
            style: child.style,
            prefix: child.prefix,
            suffix: child.suffix,
            negative: child.negative,
            color: child.color,
        });
    }
    result.validate().map_err(|e| OdpError(e.to_string()))?;
    Ok(result)
}
fn allowed(el: &El, keys: &[&str]) -> Result<()> {
    if el.attrs.keys().any(|k| !keys.contains(&k.as_str())) { Err(OdpError("unsupported ODF number-style property".into())) } else { Ok(()) }
}
fn uint(el: &El, key: &str, default: u8) -> Result<u8> {
    el.attr(key).map(|v| v.parse::<u8>().map_err(|_| OdpError("invalid number-style integer".into()))).unwrap_or(Ok(default))
}
fn boolean(el: &El, key: &str, default: bool) -> Result<bool> {
    match el.attr(key) {
        None => Ok(default),
        Some("true" | "1") => Ok(true),
        Some("false" | "0") => Ok(false),
        _ => Err(OdpError("invalid number-style boolean".into())),
    }
}
fn text(el: &El) -> Result<String> {
    let mut out = String::new();
    for node in &el.children {
        match node {
            Node::Text(t) => out.push_str(t),
            _ => return Err(OdpError("unsupported number-style literal children".into())),
        }
        if out.len() > 1024 {
            return Err(OdpError("number-style literal exceeds limit".into()));
        }
    }
    Ok(out)
}
fn plain(el: &El) -> Result<NumberFormat> {
    if el.children.iter().any(|n| matches!(n,Node::Text(t) if !t.trim().is_empty())) {
        return Err(OdpError("unexpected text outside number-style literals".into()));
    }
    allowed(
        el,
        &[
            "style:name",
            "style:display-name",
            "style:volatile",
            "number:language",
            "number:country",
            "number:truncate-on-overflow",
            "number:automatic-order",
            "number:format-source",
        ],
    )?;
    if el.attr("number:automatic-order").is_some_and(|v| v != "false") || el.attr("number:format-source").is_some_and(|v| v != "fixed") {
        return Err(OdpError("host-selected date format is unsupported".into()));
    }
    let locale = match (el.attr("number:language"), el.attr("number:country")) {
        (Some("en"), Some("US")) => NumberLocale::EnUs,
        (Some("fr"), Some("BE")) => NumberLocale::FrBe,
        (Some("de"), Some("DE")) => NumberLocale::DeDe,
        _ => return Err(OdpError("unsupported ODF number-format locale".into())),
    };
    let mut result = NumberFormat { name: el.attr("style:name").unwrap_or("").into(), locale, ..Default::default() };
    let temporal = matches!(el.name.as_str(), "number:date-style" | "number:time-style");
    let mut parts = Vec::new();
    let mut value = None;
    let mut literals = String::new();
    let mut currency = None;
    let mut currency_after = false;
    let mut currency_gap = String::new();
    let mut before = String::new();
    for child in el.elements() {
        match child.name.as_str() {
            "style:map" => continue,
            "style:text-properties" => {
                allowed(child, &["fo:color"])?;
                let v = child
                    .attr("fo:color")
                    .and_then(|v| v.strip_prefix('#'))
                    .filter(|v| v.len() == 6 && v.bytes().all(|b| b.is_ascii_hexdigit()))
                    .ok_or_else(|| OdpError("invalid number-style color".into()))?;
                let n = u32::from_str_radix(v, 16).map_err(|_| OdpError("invalid color".into()))?;
                result.color = Some([(n >> 16) as u8, (n >> 8) as u8, n as u8, 255]);
            }
            "number:text" => {
                allowed(child, &[])?;
                let v = text(child)?;
                if temporal { parts.push(DateFormatPart::Literal { text: v }) } else { literals.push_str(&v) }
            }
            "number:currency-symbol" => {
                if el.name != "number:currency-style" {
                    return Err(OdpError("misplaced currency-symbol formatter".into()));
                }
                allowed(child, &["number:language", "number:country"])?;
                if currency.is_some() {
                    return Err(OdpError("multiple currency symbols".into()));
                }
                currency = Some(text(child)?);
                currency_after = value.is_some();
                if !currency_after {
                    before = std::mem::take(&mut literals)
                } else {
                    currency_gap = std::mem::take(&mut literals);
                }
            }
            "number:number" | "number:scientific-number" | "number:fraction" | "number:boolean" | "number:text-content" => {
                if value.is_some() || temporal {
                    return Err(OdpError("multiple or misplaced number-style values".into()));
                }
                result.prefix = std::mem::take(&mut literals);
                let style = match child.name.as_str() {
                    "number:number" => {
                        allowed(child, &["number:decimal-places", "number:min-decimal-places", "number:min-integer-digits", "number:grouping"])?;
                        if child.attr("number:decimal-places").is_none() {
                            return Err(OdpError("inherited decimal precision requires additional mapping".into()));
                        }
                        let dp = uint(child, "number:decimal-places", 0)?;
                        NumberStyle::Number {
                            decimal_places: dp,
                            min_decimal_places: uint(child, "number:min-decimal-places", dp)?,
                            min_integer_digits: uint(child, "number:min-integer-digits", 1)?,
                            grouping: boolean(child, "number:grouping", false)?,
                        }
                    }
                    "number:scientific-number" => {
                        allowed(
                            child,
                            &["number:decimal-places", "number:min-integer-digits", "number:min-exponent-digits", "number:forced-exponent-sign"],
                        )?;
                        if uint(child, "number:min-integer-digits", 1)? != 1 {
                            return Err(OdpError("engineering number style requires additional mapping".into()));
                        }
                        NumberStyle::Scientific {
                            decimal_places: uint(child, "number:decimal-places", 0)?,
                            min_exponent_digits: uint(child, "number:min-exponent-digits", 1)?,
                            exponent_sign: boolean(child, "number:forced-exponent-sign", true)?,
                        }
                    }
                    "number:fraction" => {
                        allowed(
                            child,
                            &[
                                "number:min-integer-digits",
                                "number:min-numerator-digits",
                                "number:min-denominator-digits",
                                "number:denominator-value",
                                "number:max-denominator-value",
                            ],
                        )?;
                        if child.attr("number:min-integer-digits").is_none()
                            || uint(child, "number:min-integer-digits", 1)? != 1
                            || uint(child, "number:min-numerator-digits", 1)? != 1
                            || uint(child, "number:min-denominator-digits", 1)? != 1
                        {
                            return Err(OdpError("unsupported fraction padding".into()));
                        }
                        NumberStyle::Fraction {
                            denominator_digits: if child.attr("number:denominator-value").is_some() {
                                1
                            } else {
                                match child.attr("number:max-denominator-value") {
                                    Some("9") => 1,
                                    Some("99") => 2,
                                    Some("999") => 3,
                                    Some("9999") => 4,
                                    _ => return Err(OdpError("unbounded or unsupported fraction denominator search".into())),
                                }
                            },
                            fixed_denominator: child
                                .attr("number:denominator-value")
                                .map(|s| s.parse::<u32>().map_err(|_| OdpError("invalid fraction denominator".into())))
                                .transpose()?,
                        }
                    }
                    "number:boolean" => {
                        allowed(child, &[])?;
                        NumberStyle::Boolean { true_text: "TRUE".into(), false_text: "FALSE".into() }
                    }
                    _ => {
                        allowed(child, &[])?;
                        NumberStyle::Text
                    }
                };
                if child.elements().next().is_some() {
                    return Err(OdpError("unsupported numeric format child".into()));
                }
                value = Some(style);
            }
            name if temporal => {
                if !child.children.is_empty() {
                    return Err(OdpError("unexpected date-part content".into()));
                }
                allowed(child, &["number:style", "number:textual", "number:decimal-places", "number:calendar"])?;
                if child.attr("number:calendar").is_some_and(|v| v != "gregorian") {
                    return Err(OdpError("unsupported number-format calendar".into()));
                }
                let long = match child.attr("number:style") {
                    None | Some("short") => false,
                    Some("long") => true,
                    _ => return Err(OdpError("invalid date part length".into())),
                };
                parts.push(match name {
                    "number:year" => DateFormatPart::Year { long },
                    "number:month" => DateFormatPart::Month { long, textual: boolean(child, "number:textual", false)? },
                    "number:day" => DateFormatPart::Day { long },
                    "number:day-of-week" => DateFormatPart::DayOfWeek { long },
                    "number:hours" => DateFormatPart::Hours { long, elapsed: el.attr("number:truncate-on-overflow") == Some("false") },
                    "number:minutes" => DateFormatPart::Minutes { long },
                    "number:seconds" => DateFormatPart::Seconds { long, decimal_places: uint(child, "number:decimal-places", 0)? },
                    "number:am-pm" => DateFormatPart::AmPm,
                    _ => return Err(OdpError("unsupported temporal format part".into())),
                });
            }
            _ => return Err(OdpError("unsupported number-style child".into())),
        }
        if parts.len() > 64 || literals.len() > 1024 {
            return Err(OdpError("number-style program exceeds limit".into()));
        }
    }
    result.suffix = literals;
    result.style = if temporal {
        if el.name == "number:time-style" && el.attr("number:truncate-on-overflow") == Some("false") {
            NumberStyle::Duration { parts }
        } else {
            NumberStyle::DateTime { parts }
        }
    } else {
        value.ok_or_else(|| OdpError("number style has no value formatter".into()))?
    };
    match el.name.as_str() {
        "number:percentage-style" => {
            let NumberStyle::Number { decimal_places, min_decimal_places, grouping, .. } = result.style else {
                return Err(OdpError("invalid percentage format".into()));
            };
            if !result.suffix.ends_with('%') {
                return Err(OdpError("percentage style lacks literal percent".into()));
            }
            result.suffix.pop();
            result.style = NumberStyle::Percentage { decimal_places, min_decimal_places, grouping };
        }
        "number:currency-style" => {
            let NumberStyle::Number { decimal_places, min_decimal_places, grouping, .. } = result.style else {
                return Err(OdpError("invalid currency format".into()));
            };
            let symbol = currency.ok_or_else(|| OdpError("currency style lacks symbol".into()))?;
            let gap = if currency_after { currency_gap.as_str() } else { result.prefix.as_str() };
            let space = match gap {
                " " => true,
                "" => false,
                _ => return Err(OdpError("unsupported currency symbol spacing".into())),
            };
            if !currency_after {
                result.prefix = before;
            }
            result.style = NumberStyle::Currency {
                decimal_places,
                min_decimal_places,
                grouping,
                code: String::new(),
                symbol,
                symbol_after: currency_after,
                space,
            };
        }
        "number:number-style"
            if matches!(result.style, NumberStyle::Number { .. } | NumberStyle::Scientific { .. } | NumberStyle::Fraction { .. }) => {}
        "number:date-style" | "number:time-style" => {}
        "number:boolean-style" if matches!(result.style, NumberStyle::Boolean { .. }) => {}
        "number:text-style" if matches!(result.style, NumberStyle::Text) => {}
        _ => return Err(OdpError("unsupported data-style element".into())),
    }
    result.validate().map_err(|e| OdpError(e.to_string()))?;
    Ok(result)
}

pub(crate) fn selected<'a>(styles: &Styles<'a>, name: Option<&str>) -> Result<Option<&'a str>> {
    let Some(name) = name else { return Ok(None) };
    let mut style: &'a El = styles.get(&("table-cell".into(), name.into())).copied().ok_or_else(|| OdpError("missing table-cell style".into()))?;
    for _ in 0..32 {
        if let Some(data) = style.attr("style:data-style-name") {
            return Ok(Some(data));
        }
        let Some(parent) = style.attr("style:parent-style-name") else { return Ok(None) };
        style = styles.get(&("table-cell".into(), parent.into())).copied().ok_or_else(|| OdpError("missing table-cell parent style".into()))?;
    }
    Err(OdpError("cyclic table-cell data-style inheritance".into()))
}
