//! Native table serialization; stored values require explicit model ownership.
use super::*;

/// XML literal whitespace in attributes is normalized by XML readers. Character
/// references preserve the stored scalar independently of displayed paragraphs.
fn value_attribute(value: &str) -> Result<String> {
    if value.len() > 8192 {
        return Err(OdpError("stored table value exceeds ODP XML attribute size limit".into()));
    }
    Ok(esc(value).replace('\t', "&#x9;").replace('\n', "&#xA;").replace('\r', "&#xD;"))
}

fn constant_attributes(cell: &Cell) -> Result<String> {
    use deckcraft_model::table::{CellConstantState, CellFormulaState, CellNumberKind, CellValue, FormulaOutcome};
    if cell.formula.is_some() {
        let state = cell.formula_state().map_err(|error| OdpError(error.to_string()))?;
        let CellFormulaState::Current(formula) = state else {
            return Err(OdpError("stale table formula display requires reconciliation before ODP export".into()));
        };
        let uri = match formula.profile() {
            deckcraft_model::table::FORMULA_PROFILE_V2 => "urn:deckcraft:formula:presentation-decimal-v2:1",
            deckcraft_model::table::FORMULA_PROFILE_V1_FORMATTED => "urn:deckcraft:formula:exact-decimal-v1-formatted:1",
            _ if formula.expression().contains("[#REF!]") => "urn:deckcraft:formula:exact-decimal-v1:1",
            _ => "urn:oasis:names:tc:opendocument:xmlns:of:1.2",
        };
        let cache = match formula.outcome() {
            FormulaOutcome::Number { lexical } => format!("office:value-type=\"float\" office:value=\"{}\"", value_attribute(lexical)?),
            FormulaOutcome::Boolean { value } => format!("office:value-type=\"boolean\" office:boolean-value=\"{value}\""),
            FormulaOutcome::String { value } => format!("office:value-type=\"string\" office:string-value=\"{}\"", value_attribute(value)?),
            FormulaOutcome::Error { code } => format!("office:value-type=\"string\" office:string-value=\"{}\"", code.display()),
        };
        return Ok(format!("xmlns:dcformula=\"{uri}\" table:formula=\"dcformula:{}\" {cache}", value_attribute(formula.expression())?));
    }
    let state = cell.constant_state().map_err(|error| OdpError(error.to_string()))?;
    let CellConstantState::Current(data) = state else { return Ok("office:value-type=\"string\"".into()) };
    // Baselines are exact logical text. XML normalization and the ordinary text
    // writer cannot preserve literal CR/LF inside a run or other C0 controls.
    // Refuse rather than importing a different display and rebinding its value.
    for paragraph in &cell.text.paragraphs {
        for run in &paragraph.runs {
            if !run
                .content()
                .chars()
                .all(|c| matches!(c, '\t' | '\u{b}' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
            {
                return Err(OdpError("stored table value display contains characters requiring additional ODP text representation".into()));
            }
        }
    }
    Ok(match data.value() {
        CellValue::String { value } => format!("office:value-type=\"string\" office:string-value=\"{}\"", value_attribute(value)?),
        CellValue::Boolean { value } => format!("office:value-type=\"boolean\" office:boolean-value=\"{value}\""),
        CellValue::Date { lexical } => format!("office:value-type=\"date\" office:date-value=\"{}\"", value_attribute(lexical)?),
        CellValue::Duration { lexical } => format!("office:value-type=\"time\" office:time-value=\"{}\"", value_attribute(lexical)?),
        CellValue::Number { kind, lexical } => {
            let (kind, currency) = match kind {
                CellNumberKind::Float => ("float", None),
                CellNumberKind::Percentage => ("percentage", None),
                CellNumberKind::Currency { currency } => ("currency", currency.as_deref()),
            };
            let currency =
                currency.map(|value| value_attribute(value).map(|value| format!(" office:currency=\"{value}\""))).transpose()?.unwrap_or_default();
            format!("office:value-type=\"{kind}\" office:value=\"{}\"{currency}", value_attribute(lexical)?)
        }
    })
}

impl Writer<'_> {
    pub(super) fn table(&mut self, ctx: &Ctx<'_>, table: &Table, out: &mut String) -> Result<()> {
        table.validate_content().map_err(|error| OdpError(error.to_string()))?;
        self.table_inner(ctx, table, out, 0)
    }
    fn table_inner(&mut self, ctx: &Ctx<'_>, table: &Table, out: &mut String, depth: usize) -> Result<()> {
        if depth > deckcraft_model::table::MAX_NESTED_TABLE_DEPTH {
            return Err(OdpError("nested table depth exceeds limit".into()));
        }
        use deckcraft_model::{
            style::{Compound, Dash},
            text::{Anchor, TextDir},
        };
        // ODF §20.183 and §20.259/261 border values cannot carry arbitrary
        // drawing-line paints, marker geometry or dash lengths.
        let border_value = |line: &Line| -> Result<String> {
            let width = line.width.unwrap_or(1.0);
            styles::finite(&[width])?;
            if width < 0.0 {
                return Err(OdpError("negative table cell border width".into()));
            }
            let color = match &line.fill {
                None | Some(Fill::None) => return Ok("none".into()),
                Some(Fill::Solid { color }) => ctx.color(color, None),
                _ => return Err(OdpError("table cell border paint cannot be represented faithfully in ODP".into())),
            };
            if width == 0.0 || color.a != 255 {
                return Err(OdpError("table cell border opacity or zero width cannot be represented faithfully in ODP".into()));
            }
            if line.cap.is_some() || line.join.is_some() || [&line.head, &line.tail].into_iter().flatten().any(|end| end.kind != "none") {
                return Err(OdpError("table cell border caps, joins or markers cannot be represented faithfully in ODP".into()));
            }
            let dash = match &line.dash {
                None => "solid",
                Some(Dash::Preset { name }) => match name.as_str() {
                    "solid" => "solid",
                    "dot" => "dotted",
                    "dash" => "dashed",
                    "dashDot" => "dash-dot",
                    "lgDashDotDot" => "dash-dot-dot",
                    _ => return Err(OdpError("table cell border dash preset cannot be represented faithfully in ODP".into())),
                },
                Some(Dash::Custom { .. }) => return Err(OdpError("custom table cell border dashes cannot be represented faithfully in ODP".into())),
            };
            let kind = match line.compound.unwrap_or(Compound::Single) {
                Compound::Single => dash,
                Compound::Double if dash == "solid" => "double",
                _ => return Err(OdpError("table cell compound border cannot be represented faithfully in ODP".into())),
            };
            Ok(format!("{width}pt {kind} #{}", color.hex()))
        };
        if ctx.pres.table_styles.iter().find(|d| d.id.eq_ignore_ascii_case(&table.style)).is_some_and(|d| d.background.is_some()) {
            return Err(OdpError("layered file-defined table background paints cannot be represented faithfully in ODP yet".into()));
        }
        let table_background = match &table.background {
            None => None,
            Some(Fill::None) => Some("transparent".into()),
            Some(Fill::Solid { color }) => {
                let color = ctx.color(color, None);
                if color.a != 255 {
                    return Err(OdpError("table-wide background opacity cannot be represented faithfully in ODP yet".into()));
                }
                Some(format!("#{}", color.hex()))
            }
            _ => return Err(OdpError("table-wide background paint cannot be represented faithfully in ODP yet".into())),
        };
        let fill_value = |fill: &Fill, ph: Option<Rgba>| -> Result<String> {
            match fill {
                Fill::None => Ok("fo:background-color=\"transparent\"".into()),
                Fill::Solid { color } => {
                    let color = ctx.color(color, ph);
                    if color.a != 255 {
                        return Err(OdpError("table cell fill opacity cannot be represented faithfully in ODP yet".into()));
                    }
                    Ok(format!("fo:background-color=\"#{}\"", color.hex()))
                }
                _ => Err(OdpError("table cell fill paint cannot be represented faithfully in ODP yet".into())),
            }
        };
        if let Some(color) = table_background {
            let style = self.style("table", &format!("<style:table-properties fo:background-color=\"{color}\"/>"));
            out.push_str(&format!("<table:table table:style-name=\"{style}\">"));
        } else {
            out.push_str("<table:table>");
        }
        for width in &table.cols {
            styles::finite(&[*width])?;
            if *width < 0.0 {
                return Err(OdpError("negative table column width".into()));
            }
            let style = self.style("table-column", &format!("<style:table-column-properties style:column-width=\"{width}pt\"/>"));
            out.push_str(&format!("<table:table-column table:style-name=\"{style}\"/>"));
        }
        for (r, row) in table.rows.iter().enumerate() {
            styles::finite(&[row.height])?;
            if row.height < 0.0 {
                return Err(OdpError("negative table row height".into()));
            }
            let style = self.style("table-row", &format!("<style:table-row-properties style:row-height=\"{}pt\"/>", row.height));
            out.push_str(&format!("<table:table-row table:style-name=\"{style}\">"));
            for (c, cell) in row.cells.iter().enumerate() {
                if cell.is_covered() {
                    if cell.data.is_some() || cell.formula.is_some() {
                        return Err(OdpError("covered table cells cannot carry stored constants".into()));
                    }
                    out.push_str("<table:covered-table-cell/>");
                    continue;
                }
                let look = deckcraft_render::table::look(ctx, table, r, c);
                let mut props = if let Some(fill) = &cell.fill {
                    fill_value(fill, None)?
                } else if let Some((fill, ph)) = &look.paint {
                    fill_value(fill, *ph)?
                } else if let Some(color) = look.fill {
                    fill_value(&Fill::solid(ColorRef::rgb(color)), None)?
                } else {
                    String::new()
                };
                for (i, key) in ["left", "right", "top", "bottom"].iter().enumerate() {
                    if let Some(line) = cell.borders.get(i).and_then(Option::as_ref) {
                        props.push_str(&format!(" fo:border-{key}=\"{}\"", border_value(line)?));
                    } else if let Some((color, width)) = look.sides.get(i).copied().flatten().or(look.border) {
                        styles::finite(&[width])?;
                        if width < 0.0 {
                            return Err(OdpError("negative inherited table cell border width".into()));
                        }
                        let value =
                            if width == 0.0 || color.a == 0 { "none".into() } else { border_value(&Line::solid(ColorRef::rgb(color), width))? };
                        props.push_str(&format!(" fo:border-{key}=\"{value}\""));
                    }
                }
                for (key, line) in [("style:diagonal-tl-br", &cell.diag_down), ("style:diagonal-bl-tr", &cell.diag_up)] {
                    if let Some(line) = line {
                        props.push_str(&format!(" {key}=\"{}\"", border_value(line)?));
                    }
                }
                // §20.404.6: the cell direction also supplies its text body's direction.
                if let Some(direction) = cell.vertical.or(cell.text.body.vert) {
                    let mode = match direction {
                        TextDir::Horizontal => "lr-tb",
                        TextDir::Vertical => "tb-lr",
                        TextDir::EaVertical => "tb-rl",
                        _ => return Err(OdpError("table cell writing mode cannot be represented faithfully in ODP".into())),
                    };
                    props.push_str(&format!(" style:writing-mode=\"{mode}\""));
                }
                let [l, right, t, b] = cell.margins.unwrap_or([7.2, 7.2, 3.6, 3.6]);
                styles::finite(&[l, right, t, b])?;
                if [l, right, t, b].iter().any(|v| *v < 0.0) {
                    return Err(OdpError("negative table cell padding".into()));
                }
                let anchor = match cell.anchor.unwrap_or(Anchor::Top) {
                    Anchor::Top => "top",
                    Anchor::Middle => "middle",
                    Anchor::Bottom => "bottom",
                    _ => return Err(OdpError("table cell distributed alignment cannot be represented faithfully in ODP".into())),
                };
                props.push_str(&format!(" fo:padding-left=\"{l}pt\" fo:padding-right=\"{right}pt\" fo:padding-top=\"{t}pt\" fo:padding-bottom=\"{b}pt\" style:vertical-align=\"{anchor}\""));
                let style = if let Some(format) = &cell.number_format {
                    let data_name = format!("N{}", self.next);
                    self.next = self.next.saturating_add(1);
                    let definitions = crate::table_number_styles::project(format, &data_name, true)?;
                    let size = definitions.iter().fold(0usize, |sum, (_, xml)| sum.saturating_add(xml.len()));
                    if self.styles.len().saturating_add(size) > crate::MAX_PART as usize {
                        return Err(OdpError("number styles exceed ODP part limit".into()));
                    }
                    for (_, xml) in definitions {
                        self.styles.push_str(&xml);
                    }
                    let name = format!("S{}", self.next);
                    self.next = self.next.saturating_add(1);
                    self.styles.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"table-cell\" style:data-style-name=\"{data_name}\"><style:table-cell-properties {props}/></style:style>"));
                    name
                } else {
                    self.style("table-cell", &format!("<style:table-cell-properties {props}/>"))
                };
                let value = constant_attributes(cell)?;
                out.push_str(&format!(
                    "<table:table-cell table:style-name=\"{style}\" {value} table:number-columns-spanned=\"{}\" table:number-rows-spanned=\"{}\">",
                    cell.grid_span.max(1),
                    cell.row_span.max(1)
                ));
                let body = deckcraft_render::table::cell_body(ctx, table, r, c, cell);
                let shape = Shape { text: Some(body.clone()), ..Default::default() };
                if !cell.blocks.is_empty() {
                    out.push_str(&format!("<text:section text:name=\"dc-cell-first-v1-{}\">", self.next));
                    self.next = self.next.saturating_add(1);
                }
                if !body.paragraphs.is_empty() {
                    self.text_content(ctx, &shape, &body, out)?;
                }
                if !cell.blocks.is_empty() {
                    out.push_str("</text:section>");
                }
                for block in &cell.blocks {
                    match block {
                        deckcraft_model::table::CellBlock::Text(block) => {
                            out.push_str(&format!("<text:section text:name=\"dc-cell-text-v1-{}-{}\">", block.id, self.next));
                            self.next = self.next.saturating_add(1);
                            let shape = Shape { text: Some(block.body.clone()), ..Default::default() };
                            if !block.body.paragraphs.is_empty() {
                                self.text_content(ctx, &shape, &block.body, out)?;
                            }
                            out.push_str("</text:section>");
                        }
                        deckcraft_model::table::CellBlock::Table(block) => {
                            let start = out.len();
                            self.table_inner(ctx, &block.table, out, depth + 1)?;
                            let name = format!(" table:name=\"dc-cell-table-v1-{}-{}\"", block.id, self.next);
                            self.next = self.next.saturating_add(1);
                            out.insert_str(start + "<table:table".len(), &name);
                        }
                    }
                    if out.len() > crate::MAX_PART as usize {
                        return Err(OdpError("nested table XML exceeds part size limit".into()));
                    }
                }
                out.push_str("</table:table-cell>");
            }
            out.push_str("</table:table-row>");
        }
        out.push_str("</table:table>");
        Ok(())
    }
}
