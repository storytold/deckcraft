//! Source-order cell blocks. Standard sections preserve explicit text-block boundaries.
use super::*;
use deckcraft_model::TextBody;
use deckcraft_model::table::{CellBlock, CellTextBlock, MAX_CELL_BLOCKS, MAX_NESTED_TABLE_DEPTH, NestedTable};

/// Preflight the expanded tree before cloning repeated rows/cells. This counts
/// descendants once for every eventual attachment, including nested repeats.
pub(super) fn preflight(root: &El, total: &mut u64) -> Result<()> {
    fn walk(el: &El, depth: usize, multiplicity: usize, cells: &mut usize, bytes: &mut usize) -> Result<()> {
        let depth = depth + usize::from(el.name == "table:table");
        if depth > MAX_NESTED_TABLE_DEPTH + 1 {
            return Err(OdpError("nested table depth exceeds limit".into()));
        }
        let repeat = if el.name == "table:table-row" {
            super::repeat(el, "table:number-rows-repeated")?
        } else if matches!(el.name.as_str(), "table:table-cell" | "table:covered-table-cell") {
            super::repeat(el, "table:number-columns-repeated")?
        } else {
            1
        };
        let multiplicity = multiplicity
            .checked_mul(repeat)
            .filter(|n| *n <= 1_000_000)
            .ok_or_else(|| OdpError("expanded table repeat product exceeds limit".into()))?;
        if matches!(el.name.as_str(), "table:table-cell" | "table:covered-table-cell") {
            *cells = cells
                .checked_add(multiplicity)
                .filter(|n| *n <= 1_000_000)
                .ok_or_else(|| OdpError("aggregate nested table cell count exceeds limit".into()))?;
        }
        let local =
            el.attrs.iter().fold(el.name.len() + 128, |sum, (key, value)| sum.saturating_add(key.len()).saturating_add(value.len())).saturating_add(
                el.children.iter().filter_map(|n| if let crate::xml::Node::Text(t) = n { Some(t.len()) } else { None }).sum::<usize>(),
            );
        *bytes = bytes.saturating_add(local.saturating_mul(multiplicity).saturating_mul(4));
        if *bytes > 32 * 1024 * 1024 {
            return Err(OdpError("expanded nested table allocation exceeds limit".into()));
        }
        for child in el.elements() {
            walk(child, depth, multiplicity, cells, bytes)?;
        }
        Ok(())
    }
    let mut bytes = 0;
    walk(root, 0, 1, &mut 0, &mut bytes)?;
    *total = total.saturating_add(bytes as u64);
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("expanded nested table allocation exceeds total limit".into()));
    }
    Ok(())
}

fn named_id(el: &El, prefix: &str, attr: &str) -> Option<u32> {
    el.attr(attr)?.strip_prefix(prefix)?.split('-').next()?.parse::<u32>().ok().filter(|id| *id > 0)
}

pub(super) fn read_content(
    source: &El,
    styles: &Styles<'_>,
    name: Option<&str>,
    width: f64,
    total: &mut u64,
    settings: bool,
    budget: &mut deckcraft_model::table::FormulaBudget,
    depth: usize,
    warnings: &mut Vec<&'static str>,
) -> Result<(TextBody, Vec<CellBlock>)> {
    let mut first = TextBody::default();
    let mut blocks: Vec<CellBlock> = Vec::new();
    let mut current: Option<usize> = None;
    let mut next = 1u32;
    let mut text_state = crate::read::TableTextState::default();
    for member in source.elements() {
        let section = member.name == "text:section";
        if member.name == "table:table" {
            let result = super::read_inner(member, styles, width, total, settings, budget, depth + 1)?;
            for warning in result.warnings {
                super::add_warning(warnings, warning);
            }
            let id = named_id(member, "dc-cell-table-v1-", "table:name").unwrap_or(next);
            if blocks.iter().any(|b| b.id() == id) {
                return Err(OdpError("duplicate cell block identity".into()));
            }
            next = next.max(id).checked_add(1).ok_or_else(|| OdpError("cell block identity exceeds limit".into()))?;
            blocks.push(CellBlock::Table(NestedTable { id, table: Box::new(result.table) }));
            current = None;
        } else if section {
            // Native sections contain only text. Foreign sections with tables are
            // recursively imported as source-order blocks by this same adapter.
            let (body, nested) = read_content(member, styles, name, width, total, settings, budget, depth, warnings)?;
            if member.attrs.keys().any(|k| k != "text:name" && k != "xml:id") {
                super::add_warning(warnings, "ODF section metadata outside cell text-block boundaries is retained in the source archive");
            }
            let first_section = member.attr("text:name").is_some_and(|n| n.starts_with("dc-cell-first-v1-"));
            if first_section && first.paragraphs.is_empty() && blocks.is_empty() {
                first = body;
            } else {
                let id = named_id(member, "dc-cell-text-v1-", "text:name").unwrap_or(next);
                if blocks.iter().any(|b| b.id() == id) {
                    return Err(OdpError("duplicate cell block identity".into()));
                }
                next = next.max(id).checked_add(1).ok_or_else(|| OdpError("cell block identity exceeds limit".into()))?;
                blocks.push(CellBlock::Text(CellTextBlock { id, body }));
            }
            for block in nested {
                if blocks.iter().any(|b| b.id() == block.id()) {
                    return Err(OdpError("ambiguous cell section block identity".into()));
                }
                next = next.max(block.id()).checked_add(1).ok_or_else(|| OdpError("cell block identity exceeds limit".into()))?;
                blocks.push(block);
            }
            current = None;
        } else if matches!(member.name.as_str(), "text:p" | "text:h" | "text:list") {
            if blocks.is_empty() {
                crate::read::table_text_member(member, styles, name, &mut first.paragraphs, &mut text_state)?;
            } else {
                let index = if let Some(index) = current {
                    index
                } else {
                    let index = blocks.len();
                    blocks.push(CellBlock::Text(CellTextBlock { id: next, body: TextBody::default() }));
                    next = next.checked_add(1).ok_or_else(|| OdpError("cell block identity exceeds limit".into()))?;
                    current = Some(index);
                    index
                };
                let Some(CellBlock::Text(block)) = blocks.get_mut(index) else {
                    return Err(OdpError("invalid cell text-block owner".into()));
                };
                crate::read::table_text_member(member, styles, name, &mut block.body.paragraphs, &mut text_state)?;
            }
        }
        if blocks.len() > MAX_CELL_BLOCKS {
            return Err(OdpError("cell block count exceeds limit".into()));
        }
    }
    // Preserve the pre-existing typed-value fallback only when there were no
    // actual content members; an empty leading body before a table stays empty.
    if source.elements().next().is_none() {
        super::table_text_with_style(source, styles, name, &mut first.paragraphs)?;
    }
    Ok((first, blocks))
}

/// Repetition clones inherited text properties as well as source bytes. Charge
/// their actual model weight before cloning continuation trees, so a tiny cell
/// referencing a large inherited style cannot amplify below the XML estimate.
pub(super) fn blocks_weight(blocks: &[CellBlock]) -> Result<usize> {
    fn text_weight(body: &TextBody) -> usize {
        body.paragraphs.iter().fold(std::mem::size_of::<TextBody>(), |sum, p| {
            sum.saturating_add(crate::read::paragraph_weight(p)).saturating_add(
                p.runs.iter().fold(0usize, |sum, r| sum.saturating_add(r.text.len()).saturating_add(crate::read::run_weight(&r.props))),
            )
        })
    }
    fn walk(blocks: &[CellBlock], depth: usize) -> Result<usize> {
        if depth > MAX_NESTED_TABLE_DEPTH {
            return Err(OdpError("nested table allocation depth exceeds limit".into()));
        }
        let mut bytes = blocks.len().saturating_mul(std::mem::size_of::<CellBlock>());
        for block in blocks {
            bytes = bytes.saturating_add(match block {
                CellBlock::Text(block) => text_weight(&block.body),
                CellBlock::Table(child) => {
                    let mut bytes = child
                        .table
                        .cols
                        .len()
                        .saturating_mul(std::mem::size_of::<f64>())
                        .saturating_add(child.table.rows.len().saturating_mul(std::mem::size_of::<deckcraft_model::table::Row>()));
                    for cell in child.table.rows.iter().flat_map(|r| &r.cells) {
                        bytes = bytes
                            .saturating_add(std::mem::size_of::<Cell>())
                            .saturating_add(text_weight(&cell.text))
                            .saturating_add(walk(&cell.blocks, depth + 1)?);
                        if let Some(data) = &cell.data {
                            bytes = bytes.saturating_add(data.metadata_bytes());
                        }
                        if let Some(format) = &cell.number_format {
                            bytes = bytes.saturating_add(format.metadata_bytes().map_err(|e| OdpError(e.to_string()))?);
                        }
                        if let Some(formula) = &cell.formula {
                            bytes = bytes.saturating_add(64 * 1024).saturating_add(formula.expression().len());
                        }
                        if bytes > 32 * 1024 * 1024 {
                            return Err(OdpError("nested table attachment exceeds allocation limit".into()));
                        }
                    }
                    bytes
                }
            });
            if bytes > 32 * 1024 * 1024 {
                return Err(OdpError("nested table attachment exceeds allocation limit".into()));
            }
        }
        Ok(bytes)
    }
    walk(blocks, 0)
}
