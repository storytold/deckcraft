//! Native ODF table objects, including repeated rows/columns and merged cells.
use crate::{
    OdpError, Result,
    read::{Props, Styles, color, length, properties, table_cell_style_warnings, table_text_with_style},
    xml::El,
};
use deckcraft_model::{
    Cell, Fill, Line, Table,
    table::{CellConstant, CellNumberKind, CellValue, MAX_TEMPORAL_BYTES, Row},
    text::Anchor,
};
use std::sync::Arc;

#[path = "table_cell_content.rs"]
mod content;
#[path = "table_formula.rs"]
mod formulas;

const VALUES_WARNING: &str = "ODF typed cell values and number formats outside the supported constant subset are retained in the source archive";
const COVERED_WARNING: &str =
    "ODF covered cell content is retained in the source archive; regenerated tables cannot preserve hidden cell content yet";
const FORMULA_WARNING: &str =
    "ODF cell formulas are retained in the source archive; the cached displayed text remains editable without recalculation";

pub struct TableRead {
    pub table: Table,
    pub warnings: Vec<&'static str>,
}

fn add_warning(out: &mut Vec<&'static str>, message: &'static str) {
    if !out.contains(&message) {
        // A hidden scalar is the more specific loss than its cached paragraph.
        // Preserve that diagnostic priority when structural scanning runs first.
        if message == VALUES_WARNING
            && let Some(index) = out.iter().position(|message| *message == COVERED_WARNING)
        {
            out.insert(index, message);
        } else {
            out.push(message);
        }
    }
}

fn has_math(el: &El) -> bool {
    // The XML parser already bounds nesting at 64. MathML inline content is
    // flattened by the text reader and cannot authenticate a constant baseline.
    el.expanded_name.namespace.as_deref() == Some("http://www.w3.org/1998/Math/MathML") || el.elements().any(has_math)
}

fn format_color_matches(body: &deckcraft_model::TextBody, expected: Option<[u8; 4]>) -> bool {
    let Some([r, g, b, a]) = expected else { return true };
    let expected = Fill::solid(deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::rgba(r, g, b, a)));
    body.paragraphs.iter().flat_map(|p| &p.runs).all(|run| run.props.fill.as_ref().is_none_or(|fill| fill == &expected))
}

fn temporal_lexical(el: &El, attr: &str) -> std::result::Result<String, ()> {
    // Keep the decoded spelling, including admitted XML edge whitespace, but
    // enforce its bound before allocating the stored scalar string.
    let value = el.attr(attr).filter(|value| value.len() <= MAX_TEMPORAL_BYTES).ok_or(())?;
    Ok(value.into())
}

/// A stored scalar owns only its matching attributes. Other combinations stay
/// recoverable; in particular an unsupported value must not silently become a string.
fn stored_value(el: &El) -> std::result::Result<Option<CellValue>, ()> {
    let attrs = ["office:string-value", "office:value", "office:boolean-value", "office:date-value", "office:time-value", "office:currency"];
    let only = |allowed: &[&str]| attrs.iter().all(|key| el.attr(key).is_none() || allowed.contains(key));
    // Formula scalar attributes are untrusted caches, never independent constants.
    // Their complete table graph is admitted only after physical expansion.
    if el.attr("table:formula").is_some() {
        return Ok(None);
    }
    if el.name == "table:covered-table-cell"
        || has_math(el)
        || el.attr("table:formula").is_some()
        || el.attr("table:number-matrix-columns-spanned").is_some()
        || el.attr("table:number-matrix-rows-spanned").is_some()
    {
        return if el.attr("office:value-type").is_none_or(|v| v == "string") && only(&[]) { Ok(None) } else { Err(()) };
    }
    let value = match el.attr("office:value-type") {
        None | Some("string") if only(&[]) => return Ok(None),
        Some("string") if only(&["office:string-value"]) => CellValue::String { value: el.attr("office:string-value").ok_or(())?.into() },
        Some("boolean") if only(&["office:boolean-value"]) => CellValue::Boolean {
            value: match el.attr("office:boolean-value").ok_or(())?.trim_matches([' ', '\t', '\r', '\n']) {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return Err(()),
            },
        },
        Some("date") if only(&["office:date-value"]) => CellValue::Date { lexical: temporal_lexical(el, "office:date-value")? },
        Some("time") if only(&["office:time-value"]) => CellValue::Duration { lexical: temporal_lexical(el, "office:time-value")? },
        Some(kind @ ("float" | "percentage" | "currency"))
            if only(if kind == "currency" { &["office:value", "office:currency"] } else { &["office:value"] }) =>
        {
            CellValue::Number {
                kind: match kind {
                    "percentage" => CellNumberKind::Percentage,
                    "currency" => CellNumberKind::Currency { currency: el.attr("office:currency").map(str::to_owned) },
                    _ => CellNumberKind::Float,
                },
                lexical: el.attr("office:value").ok_or(())?.trim_matches([' ', '\t', '\r', '\n']).into(),
            }
        }
        _ => return Err(()),
    };
    value.validate().map_err(|_| ())?;
    Ok(Some(value))
}

fn charge(expanded: &mut usize, total: &mut u64, bytes: usize, count: usize, repeats: usize) -> Result<()> {
    let bytes = bytes.saturating_mul(count).saturating_mul(repeats);
    *expanded = expanded.saturating_add(bytes);
    *total = total.saturating_add(bytes as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded table content exceeds total size limit".into()));
    }
    if *expanded > 32 * 1024 * 1024 {
        return Err(OdpError("expanded table text exceeds limit".into()));
    }
    Ok(())
}

fn repeat(e: &El, attr: &str) -> Result<usize> {
    let n = e.attr(attr).unwrap_or("1").parse::<usize>().map_err(|_| OdpError("invalid table repeat".into()))?;
    if !(1..=1000).contains(&n) {
        return Err(OdpError("table repeat exceeds limit".into()));
    }
    Ok(n)
}

/// Cell paint belongs to table-cell-properties. Paragraph area and character highlighting
/// can use the same attribute name in separate bags and must not paint the whole cell.
fn cell_properties(styles: &Styles<'_>, name: Option<&str>) -> Result<Props> {
    let mut chain = Vec::new();
    let mut current = name;
    while let Some(name) = current {
        if chain.len() >= 32 {
            return Err(OdpError("cyclic or excessive table-cell property inheritance".into()));
        }
        let Some(style) = styles.get(&("table-cell".into(), name.into())) else { break };
        chain.push(*style);
        current = style.attr("style:parent-style-name");
    }
    let mut props = Props::new();
    let default = styles.get(&("table-cell".into(), String::new())).copied();
    for style in default.into_iter().chain(chain.into_iter().rev()) {
        for bag in style.elements().filter(|element| element.name == "style:table-cell-properties") {
            // A shorthand on a child style replaces inherited side values;
            // explicit sides in the same bag then refine that shorthand.
            for (shorthand, sides) in [
                ("fo:padding", ["fo:padding-left", "fo:padding-right", "fo:padding-top", "fo:padding-bottom"]),
                ("fo:border", ["fo:border-left", "fo:border-right", "fo:border-top", "fo:border-bottom"]),
            ] {
                if let Some(value) = bag.attr(shorthand) {
                    for side in sides {
                        props.insert(side.into(), value.into());
                    }
                }
            }
            props.extend(bag.attrs.clone());
        }
    }
    Ok(props)
}

fn cell_background(props: &Props) -> Result<Option<Fill>> {
    props
        .get("fo:background-color")
        .map(|value| {
            if value == "transparent" {
                Ok(Fill::None)
            } else {
                color(value).map(Fill::solid).ok_or_else(|| OdpError("invalid table-cell background color".into()))
            }
        })
        .transpose()
}

pub fn read(
    el: &El,
    styles: &Styles<'_>,
    width: f64,
    total: &mut u64,
    formula_settings_allowed: bool,
    formula_budget: &mut deckcraft_model::table::FormulaBudget,
) -> Result<TableRead> {
    content::preflight(el, total)?;
    read_inner(el, styles, width, total, formula_settings_allowed, formula_budget, 0)
}

fn read_inner(
    el: &El,
    styles: &Styles<'_>,
    width: f64,
    total: &mut u64,
    formula_settings_allowed: bool,
    formula_budget: &mut deckcraft_model::table::FormulaBudget,
    depth: usize,
) -> Result<TableRead> {
    if depth > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
        return Err(OdpError("nested table depth exceeds limit".into()));
    }
    let mut warnings = warnings(el, styles)?;
    let mut content_warnings = Vec::new();
    let mut table = Table { style: "none".into(), first_row: false, band_row: false, ..Default::default() };
    let table_props = properties(styles, "table", el.attr("table:style-name"))?;
    table.background = table_props.get("fo:background-color").and_then(|s| color(s)).map(Fill::solid);
    let mut columns = Vec::new();
    members(el, "table:table-column", &mut columns);
    let mut relative = Vec::new();
    let mut column_defaults = Vec::new();
    for column in columns {
        let props = properties(styles, "table-column", column.attr("table:style-name"))?;
        let n = repeat(column, "table:number-columns-repeated")?;
        if table.cols.len() + n > 1000 {
            return Err(OdpError("too many table columns".into()));
        }
        *total = total.saturating_add(n.saturating_mul(std::mem::size_of::<Option<&str>>()) as u64);
        if *total > crate::MAX_TOTAL {
            return Err(OdpError("table column style index exceeds total size limit".into()));
        }
        let w = props.get("style:column-width").and_then(|s| length(s)).unwrap_or(0.0);
        table.cols.extend(std::iter::repeat_n(w.max(0.0), n));
        column_defaults.extend(std::iter::repeat_n(column.attr("table:default-cell-style-name"), n));
        let rel = props
            .get("style:rel-column-width")
            .and_then(|s| s.strip_suffix('*'))
            .and_then(crate::geometry::number)
            .filter(|v| *v > 0.0)
            .unwrap_or(1.0);
        relative.extend(std::iter::repeat_n(rel, n));
    }
    let mut rows = Vec::new();
    members(el, "table:table-row", &mut rows);
    let mut cells = 0usize;
    let mut expanded_text = 0usize;
    for row in rows {
        let repeats = repeat(row, "table:number-rows-repeated")?;
        if table.rows.len() + repeats > 1000 {
            return Err(OdpError("too many table rows".into()));
        }
        let props = properties(styles, "table-row", row.attr("table:style-name"))?;
        let mut output = Row {
            height: props.get("style:row-height").or_else(|| props.get("style:min-row-height")).and_then(|s| length(s)).unwrap_or(30.0),
            cells: Vec::new(),
        };
        for cell in row.elements().filter(|e| matches!(e.name.as_str(), "table:table-cell" | "table:covered-table-cell")) {
            let count = repeat(cell, "table:number-columns-repeated")?;
            cells = cells.saturating_add(count.saturating_mul(repeats));
            if cells > 100_000 || output.cells.len() + count > 1000 {
                return Err(OdpError("table cell count exceeds limit".into()));
            }
            // Repeated cells still occupy separate physical columns. Resolve a
            // whole selected style for each contiguous run, never overlay row
            // properties onto a selected explicit or column style.
            let end = output.cells.len() + count;
            while output.cells.len() < end {
                let name = selected_style(cell, row, &column_defaults, output.cells.len());
                let count = (output.cells.len()..end).take_while(|column| selected_style(cell, row, &column_defaults, *column) == name).count();
                let props = cell_properties(styles, name)?;
                let (body, blocks) =
                    content::read_content(cell, styles, name, width, total, formula_settings_allowed, formula_budget, depth, &mut content_warnings)?;
                charge(&mut expanded_text, total, content::blocks_weight(&blocks)?, count, repeats)?;
                let text_bytes: usize = body.paragraphs.iter().flat_map(|p| &p.runs).map(|r| r.text.len()).sum();
                let body_size = body.paragraphs.iter().fold(0usize, |sum, p| {
                    sum.saturating_add(crate::read::paragraph_weight(p))
                        .saturating_add(p.runs.iter().fold(0usize, |sum, r| sum.saturating_add(crate::read::run_weight(&r.props))))
                });
                charge(&mut expanded_text, total, text_bytes.saturating_add(body_size).saturating_add(std::mem::size_of::<Cell>()), count, repeats)?;
                let data = match stored_value(cell) {
                    Ok(Some(value)) => match CellConstant::metadata_bytes_for_display(&value, &body) {
                        Ok(bytes) => {
                            // Charge the complete baseline/record cost before allocation,
                            // multiplied by every eventual row/column attachment.
                            charge(&mut expanded_text, total, bytes, count, repeats)?;
                            let data = CellConstant::from_display(value, &body).map_err(|error| OdpError(error.to_string()))?;
                            if data.metadata_bytes() > bytes {
                                return Err(OdpError("cell constant allocation exceeds precharged metadata".into()));
                            }
                            Some(Arc::new(data))
                        }
                        Err(_) => {
                            add_warning(&mut warnings, VALUES_WARNING);
                            None
                        }
                    },
                    Ok(None) => None,
                    Err(()) => {
                        add_warning(&mut warnings, VALUES_WARNING);
                        None
                    }
                };
                let mut out = Cell {
                    text: body,
                    blocks,
                    data,
                    grid_span: cell.attr("table:number-columns-spanned").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).clamp(1, 1000),
                    row_span: cell.attr("table:number-rows-spanned").and_then(|s| s.parse::<u32>().ok()).unwrap_or(1).clamp(1, 1000),
                    h_merge: cell.name == "table:covered-table-cell",
                    ..Default::default()
                };
                if let Some(format_name) = crate::table_number_styles::selected(styles, name)? {
                    match crate::table_number_styles::read(styles, format_name) {
                        Ok(format) => {
                            let bytes = format.metadata_bytes().map_err(|error| OdpError(error.to_string()))?;
                            charge(&mut expanded_text, total, bytes, count, repeats)?;
                            let matches = match out.data.as_ref() {
                                Some(data) => format.format_value(data.value()).is_ok_and(|value| {
                                    data.display_baseline().iter().map(String::as_str).eq(value.text.split('\n'))
                                        && format_color_matches(&out.text, value.color)
                                }),
                                // A native format may be an unused future display
                                // program on an unbound text cell. Its program/style
                                // proof is sufficient; there is no scalar-derived cache.
                                None => {
                                    cell.attr("table:formula").is_some()
                                        || styles
                                            .get(&("data".into(), format_name.into()))
                                            .is_some_and(|style| crate::table_number_styles::has_program(style))
                                }
                            };
                            if matches {
                                out.number_format = Some(Arc::new(format));
                            } else {
                                add_warning(&mut warnings, VALUES_WARNING);
                            }
                        }
                        Err(_) => add_warning(&mut warnings, VALUES_WARNING),
                    }
                }
                out.fill = cell_background(&props)?;
                out.vertical = props.get("style:writing-mode").and_then(|s| match s.as_str() {
                    "tb-rl" => Some(deckcraft_model::text::TextDir::EaVertical),
                    "tb-lr" => Some(deckcraft_model::text::TextDir::Vertical),
                    "lr-tb" | "rl-tb" => Some(deckcraft_model::text::TextDir::Horizontal),
                    _ => None,
                });
                out.diag_down = props.get("style:diagonal-tl-br").map(|s| border(s)).transpose()?;
                out.diag_up = props.get("style:diagonal-bl-tr").map(|s| border(s)).transpose()?;
                out.anchor = props.get("style:vertical-align").map(|s| match s.as_str() {
                    "middle" => Anchor::Middle,
                    "bottom" => Anchor::Bottom,
                    _ => Anchor::Top,
                });
                let mut margins = [0.0; 4];
                for (i, key) in ["fo:padding-left", "fo:padding-right", "fo:padding-top", "fo:padding-bottom"].iter().enumerate() {
                    if let Some(side) = margins.get_mut(i) {
                        *side = props.get(*key).or_else(|| props.get("fo:padding")).and_then(|s| length(s)).unwrap_or(0.0);
                    }
                }
                out.margins = Some(margins);
                for (i, key) in ["fo:border-left", "fo:border-right", "fo:border-top", "fo:border-bottom"].iter().enumerate() {
                    if let Some(border) = props.get(*key).or_else(|| props.get("fo:border")) {
                        let line = self::border(border)?;
                        if let Some(side) = out.borders.get_mut(i) {
                            *side = Some(line);
                        }
                    }
                }
                output.cells.extend(std::iter::repeat_n(out, count));
            }
        }
        table.rows.extend(std::iter::repeat_n(output, repeats));
    }
    let cols = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0).max(table.cols.len());
    if cols == 0 {
        return Err(OdpError("empty table".into()));
    }
    table.cols.resize(cols, 0.0);
    relative.resize(cols, 1.0);
    let fixed = table.cols.iter().filter(|w| **w > 0.0).sum::<f64>();
    let weight = table.cols.iter().zip(&relative).filter(|(w, _)| **w <= 0.0).map(|(_, weight)| *weight).sum::<f64>();
    for (w, relative) in table.cols.iter_mut().zip(&relative) {
        if *w <= 0.0 {
            *w = if fixed < width && weight > 0.0 { (width - fixed) * relative / weight } else { width / cols as f64 };
        }
    }
    let padding = table.rows.iter().fold(0usize, |sum, row| sum.saturating_add(cols.saturating_sub(row.cells.len())));
    cells = cells.saturating_add(padding);
    if cells > 100_000 {
        return Err(OdpError("padded table cell count exceeds limit".into()));
    }
    let structure = padding
        .saturating_mul(
            std::mem::size_of::<Cell>()
                .saturating_add(std::mem::size_of::<deckcraft_model::Paragraph>())
                .saturating_add(std::mem::size_of::<deckcraft_model::Run>()),
        )
        .saturating_add(table.rows.len().saturating_mul(std::mem::size_of::<Row>()));
    *total = total.saturating_add(structure as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded table structure exceeds total size limit".into()));
    }
    for row in &mut table.rows {
        row.cells.resize_with(cols, || Cell::new(""));
    }
    // Recover vertical coverage from anchor spans; ODF uses one generic covered-cell element.
    for r in 0..table.rows.len() {
        for c in 0..cols {
            let Some(cell) = table.cell(r, c).cloned().filter(|c| !c.is_covered()) else { continue };
            for rr in r..(r + cell.row_span as usize).min(table.rows.len()) {
                for cc in c..(c + cell.grid_span as usize).min(cols) {
                    if (rr, cc) != (r, c)
                        && let Some(covered) = table.cell_mut(rr, cc)
                    {
                        if covered.data.take().is_some() {
                            add_warning(&mut warnings, VALUES_WARNING);
                            add_warning(&mut warnings, COVERED_WARNING);
                        }
                        if !covered.is_covered() && !covered.text.paragraphs.is_empty() {
                            add_warning(&mut warnings, COVERED_WARNING);
                        }
                        covered.h_merge = cc > c;
                        covered.v_merge = rr > r;
                    }
                }
            }
        }
    }
    if formulas::attach(el, &mut table, formula_settings_allowed && !warnings.contains(&VALUES_WARNING), total, formula_budget)? {
        warnings.retain(|warning| *warning != FORMULA_WARNING);
    }
    for warning in content_warnings {
        add_warning(&mut warnings, warning);
    }
    table.validate_content().map_err(|error| OdpError(error.to_string()))?;
    Ok(TableRead { table, warnings })
}

/// ODF 1.3 §19.730.4/.13 selects one style; the selected style's
/// parent/default inheritance is then handled by the ordinary family resolver.
fn selected_style<'a>(cell: &'a El, row: &'a El, columns: &[Option<&'a str>], column: usize) -> Option<&'a str> {
    cell.attr("table:style-name").or_else(|| row.attr("table:default-cell-style-name")).or_else(|| columns.get(column).copied().flatten())
}

/// Expand only structural row/column containers, never descend into nested cell tables.
fn members<'a>(el: &'a El, name: &str, out: &mut Vec<&'a El>) {
    for child in el.elements() {
        if child.name == name {
            out.push(child);
        } else if matches!(
            child.name.as_str(),
            "table:table-header-rows"
                | "table:table-rows"
                | "table:table-row-group"
                | "table:table-header-columns"
                | "table:table-columns"
                | "table:table-column-group"
        ) {
            members(child, name, out);
        }
    }
}
fn border(value: &str) -> Result<Line> {
    use deckcraft_model::style::{Compound, Dash};
    if value.split_whitespace().any(|s| matches!(s, "none" | "hidden")) {
        return Ok(Line::none());
    }
    let width = value.split_whitespace().find_map(length).unwrap_or(1.0);
    if width < 0.0 {
        return Err(OdpError("negative table border width".into()));
    }
    let color = value.split_whitespace().find_map(color).unwrap_or_else(|| deckcraft_model::ColorRef::rgb(deckcraft_model::Rgba::BLACK));
    let mut line = Line::solid(color, width);
    let kind = value.split_whitespace().find(|s| matches!(*s, "double" | "dotted" | "dashed" | "dash-dot" | "dash-dot-dot"));
    match kind {
        Some("double") => line.compound = Some(Compound::Double),
        Some(kind) => {
            line.dash = Some(Dash::Preset {
                name: match kind {
                    "dotted" => "dot",
                    "dashed" => "dash",
                    "dash-dot" => "dashDot",
                    _ => "lgDashDotDot",
                }
                .into(),
            })
        }
        None => {}
    }
    Ok(line)
}

fn warnings(el: &El, styles: &Styles<'_>) -> Result<Vec<&'static str>> {
    fn inspect_style(styles: &Styles<'_>, name: Option<&str>, out: &mut Vec<&'static str>) -> Result<()> {
        fn inspect_border_bags(style: &El, out: &mut Vec<&'static str>) {
            for bag in style.elements().filter(|element| element.name == "style:table-cell-properties") {
                if bag.attrs.values().any(|value| value.split_whitespace().any(|part| matches!(part, "groove" | "ridge" | "inset" | "outset")))
                    || bag.attrs.keys().any(|key| {
                        key.contains("border-line-width") || key.contains("diagonal-tl-br-widths") || key.contains("diagonal-bl-tr-widths")
                    })
                {
                    add_warning(
                        out,
                        "ODF relief borders or custom double-line ratios are retained in the source archive; the closest editable border is used",
                    );
                }
            }
        }
        if let Some(default) = styles.get(&("table-cell".into(), String::new())) {
            inspect_border_bags(default, out);
        }
        let mut current = name;
        let mut depth = 0;
        while let Some(name) = current {
            if depth >= 32 {
                return Err(OdpError("cyclic or excessive table-cell style inheritance".into()));
            }
            depth += 1;
            let style = styles
                .get(&("table-cell".into(), name.into()))
                .filter(|style| style.name == "style:style")
                .ok_or_else(|| OdpError("missing referenced ODF table-cell style".into()))?;
            if style.attr("style:data-style-name").is_some_and(|name| crate::table_number_styles::read(styles, name).is_err()) {
                add_warning(out, VALUES_WARNING);
            }
            inspect_border_bags(style, out);
            current = style.attr("style:parent-style-name");
        }
        for warning in table_cell_style_warnings(styles, name) {
            add_warning(out, warning);
        }
        Ok(())
    }
    fn inspect(el: &El, styles: &Styles<'_>, out: &mut Vec<&'static str>, nested: bool) -> Result<()> {
        if nested && el.name == "table:table" {
            return Ok(());
        }
        if matches!(el.name.as_str(), "table:table-row" | "table:table-column")
            && let Some(name) = el.attr("table:default-cell-style-name")
        {
            // Inspect declarations even when every cell overrides them: source
            // style metadata is not represented merely by painting used cells.
            inspect_style(styles, Some(name), out)?;
        }
        if matches!(el.name.as_str(), "table:table-cell" | "table:covered-table-cell") {
            if el.attr("table:formula").is_some()
                || el.attr("table:number-matrix-columns-spanned").is_some()
                || el.attr("table:number-matrix-rows-spanned").is_some()
            {
                add_warning(out, FORMULA_WARNING);
            }
            if el.name == "table:covered-table-cell"
                && el.children.iter().any(|node| match node {
                    crate::xml::Node::El(_) => true,
                    crate::xml::Node::Text(value) => !value.chars().all(|c| matches!(c, ' ' | '\t' | '\r' | '\n')),
                })
            {
                add_warning(out, COVERED_WARNING);
            }
            if let Some(name) = el.attr("table:style-name") {
                inspect_style(styles, Some(name), out)?;
            }
        }
        for child in el.elements() {
            inspect(child, styles, out, nested || el.name == "table:table")?;
        }
        Ok(())
    }
    let mut out = Vec::new();
    // Data styles belong to named styles, not default-style definitions. Keep this
    // malformed input guarded rather than silently ignoring its metadata.
    if styles.get(&("table-cell".into(), String::new())).is_some_and(|style| style.attr("style:data-style-name").is_some()) {
        add_warning(
            &mut out,
            "ODF default cell style data-format metadata is retained in the source archive; this unsupported declaration is not editable",
        );
    }
    inspect_style(styles, None, &mut out)?;
    inspect(el, styles, &mut out, false)?;
    Ok(out)
}
