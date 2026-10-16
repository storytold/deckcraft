//! Admit a complete same-table formula graph, never an individual source cache.
use crate::{OdpError, Result, xml::El};
use deckcraft_model::{
    Table, TextBody,
    table::{CellFormula, FORMULA_PROFILE, FORMULA_PROFILE_V1_FORMATTED, FORMULA_PROFILE_V2, FormulaOutcome},
};
use std::sync::Arc;

pub(crate) const FORMULA_URI: &str = "urn:oasis:names:tc:opendocument:xmlns:of:1.2";
pub(crate) const V2_URI: &str = "urn:deckcraft:formula:presentation-decimal-v2:1";
pub(crate) const LEGACY_FORMATTED_URI: &str = "urn:deckcraft:formula:exact-decimal-v1-formatted:1";
pub(crate) const LEGACY_URI: &str = "urn:deckcraft:formula:exact-decimal-v1:1";
const MAX_FORMULAS: usize = 128;
struct Candidate {
    row: usize,
    col: usize,
    formula: Arc<CellFormula>,
    cache: FormulaOutcome,
    body: TextBody,
}
fn source_expression(el: &El) -> Option<(&str, &str)> {
    let value = el.attr("table:formula")?;
    if value.len() > 4096 + 130 {
        return None;
    }
    let (prefix, expression) = value.split_once(':')?;
    if prefix.is_empty() || prefix.len() > 128 || expression.len() > 4096 || !expression.starts_with('=') {
        return None;
    }
    let name = format!("{prefix}:formula");
    let profile = match el.resolve_qname(&name)?.0? {
        FORMULA_URI | LEGACY_URI => FORMULA_PROFILE,
        V2_URI => FORMULA_PROFILE_V2,
        LEGACY_FORMATTED_URI => FORMULA_PROFILE_V1_FORMATTED,
        _ => return None,
    };
    Some((profile, expression))
}
fn source_cache(el: &El) -> Option<FormulaOutcome> {
    let attrs = ["office:string-value", "office:value", "office:boolean-value", "office:date-value", "office:time-value", "office:currency"];
    let only = |allowed: &str| attrs.iter().all(|a| *a == allowed || el.attr(a).is_none());
    match el.attr("office:value-type")? {
        "float" if only("office:value") => {
            let v = el.attr("office:value")?;
            if v.len() > 256 {
                return None;
            }
            let v = v.trim_matches([' ', '\t', '\r', '\n']);
            (v.len() <= 128).then(|| FormulaOutcome::Number { lexical: v.into() })
        }
        "boolean" if only("office:boolean-value") => Some(FormulaOutcome::Boolean {
            value: match el.attr("office:boolean-value")?.trim() {
                "true" | "1" => true,
                "false" | "0" => false,
                _ => return None,
            },
        }),
        "string" if only("office:string-value") => {
            let v = el.attr("office:string-value")?;
            (v.len() <= 128).then(|| FormulaOutcome::String { value: v.into() })
        }
        _ => None,
    }
}
fn ordinary_source_display(cell: &El) -> bool {
    fn spans(el: &El) -> bool {
        el.children.iter().all(|node| match node {
            crate::xml::Node::Text(_) => true,
            crate::xml::Node::El(child) => match child.name.as_str() {
                "text:span" => spans(child),
                "text:s" | "text:tab" => child.children.is_empty(),
                _ => false,
            },
        })
    }
    let mut paragraphs = 0usize;
    for node in &cell.children {
        match node {
            crate::xml::Node::Text(t) if t.bytes().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')) => {}
            crate::xml::Node::El(child)
                if child.name == "text:section" && paragraphs == 0 && child.attr("text:name").is_some_and(|n| n.starts_with("dc-cell-first-v1-")) =>
            {
                return ordinary_source_display(child);
            }
            crate::xml::Node::El(child) if matches!(child.name.as_str(), "table:table" | "text:section") => break,
            crate::xml::Node::El(child) if child.name == "text:p" && paragraphs == 0 && spans(child) => paragraphs += 1,
            _ => return false,
        }
    }
    paragraphs == 1
}
fn scan(el: &El, count: &mut usize, root: bool, multiplicity: usize) -> bool {
    if !root && el.name == "table:table" {
        return true;
    }
    if el.attr("table:number-matrix-columns-spanned").is_some()
        || el.attr("table:number-matrix-rows-spanned").is_some()
        || el.name == "table:calculation-settings"
    {
        return false;
    }
    let repeat = if el.name == "table:table-row" {
        super::repeat(el, "table:number-rows-repeated").ok()
    } else if matches!(el.name.as_str(), "table:table-cell" | "table:covered-table-cell") {
        super::repeat(el, "table:number-columns-repeated").ok()
    } else {
        Some(1)
    };
    let Some(multiplicity) = repeat.and_then(|n| multiplicity.checked_mul(n)) else { return false };
    if el.attr("table:formula").is_some() {
        *count = count.saturating_add(multiplicity);
        if *count > MAX_FORMULAS {
            return false;
        }
    }
    el.elements().all(|child| scan(child, count, false, multiplicity))
}

fn charge(total: &mut u64, bytes: usize) -> Result<()> {
    *total = total.checked_add(bytes as u64).ok_or_else(|| OdpError("formula import metadata exceeds total size limit".into()))?;
    if *total > crate::MAX_TOTAL {
        return Err(OdpError("formula import metadata exceeds total size limit".into()));
    }
    Ok(())
}
fn legacy_error(cache: &FormulaOutcome) -> Option<FormulaOutcome> {
    use deckcraft_model::table::FormulaError::*;
    let FormulaOutcome::String { value } = cache else { return None };
    [InputNotNumeric, NumericLimit, DependencyError, InvalidReference]
        .into_iter()
        .find(|code| code.display() == value)
        .map(|code| FormulaOutcome::Error { code })
}
fn rows(root: &El, table: &Table, row: &mut usize, candidates: &mut Vec<Candidate>, total: &mut u64) -> Result<bool> {
    for child in root.elements() {
        if child.name == "table:table-row" {
            let repeats = super::repeat(child, "table:number-rows-repeated")?;
            let mut col = 0usize;
            for cell in child.elements().filter(|c| matches!(c.name.as_str(), "table:table-cell" | "table:covered-table-cell")) {
                let count = super::repeat(cell, "table:number-columns-repeated")?;
                if cell.attr("table:formula").is_some() {
                    if cell.name != "table:table-cell" || !ordinary_source_display(cell) {
                        return Ok(false);
                    }
                    let Some((profile, expression)) = source_expression(cell) else { return Ok(false) };
                    let Some(cache) = source_cache(cell) else { return Ok(false) };
                    for rr in 0..repeats {
                        for cc in 0..count {
                            let owner_row = row.checked_add(rr).ok_or_else(|| OdpError("formula row exceeds limit".into()))?;
                            let owner_col = col.checked_add(cc).ok_or_else(|| OdpError("formula column exceeds limit".into()))?;
                            let Some(output) = table.cell(owner_row, owner_col) else { return Ok(false) };
                            if output.data.is_some() || candidates.len() >= MAX_FORMULAS {
                                return Ok(false);
                            }
                            charge(total, 64 * 1024 + expression.len() + 128)?;
                            let outcome = if profile == FORMULA_PROFILE || profile == FORMULA_PROFILE_V1_FORMATTED {
                                legacy_error(&cache).unwrap_or_else(|| cache.clone())
                            } else {
                                cache.clone()
                            };
                            let mut body = output.text.clone();
                            if matches!(&cache,FormulaOutcome::String{value} if value.is_empty())
                                && let Some(paragraph) = body.paragraphs.first_mut().filter(|p| p.runs.is_empty())
                            {
                                paragraph.runs.push(deckcraft_model::text::Run::new(""));
                            }
                            // ODF repetition is identical source content, not a copy
                            // command. Preserve reference tokens at every physical owner.
                            let formula = match CellFormula::from_profile_display(profile, expression.into(), outcome, &body) {
                                Ok(f) => f,
                                Err(_) => return Ok(false),
                            };
                            candidates.push(Candidate { row: owner_row, col: owner_col, formula: Arc::new(formula), cache: cache.clone(), body });
                        }
                    }
                }
                col = col.checked_add(count).ok_or_else(|| OdpError("formula column index exceeds limit".into()))?;
            }
            *row = row.checked_add(repeats).ok_or_else(|| OdpError("formula row index exceeds limit".into()))?;
        } else if matches!(child.name.as_str(), "table:table-header-rows" | "table:table-rows" | "table:table-row-group")
            && !rows(child, table, row, candidates, total)?
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn cache_matches(cache: &FormulaOutcome, computed: &FormulaOutcome) -> bool {
    match (cache, computed) {
        (FormulaOutcome::Number { lexical: a }, FormulaOutcome::Number { lexical: b }) => {
            let f = deckcraft_model::table::NumberFormat::default();
            f.format_number(a).ok().zip(f.format_number(b).ok()).is_some_and(|(a, b)| a == b)
        }
        (FormulaOutcome::String { value }, FormulaOutcome::Error { code }) => value == code.display(),
        _ => cache == computed,
    }
}
/// Namespace declares semantics; recomputation authenticates every scalar and
/// display. Failed proof rolls back the whole table. Resource exhaustion propagates.
pub(super) fn attach(
    root: &El,
    table: &mut Table,
    allowed: bool,
    total: &mut u64,
    budget: &mut deckcraft_model::table::FormulaBudget,
) -> Result<bool> {
    let mut count = 0;
    if !allowed || !scan(root, &mut count, true, 1) || count == 0 {
        return Ok(false);
    }
    let mut candidates = Vec::new();
    charge(total, count.saturating_mul(std::mem::size_of::<Candidate>()))?;
    candidates.try_reserve_exact(count).map_err(|_| OdpError("cannot allocate formula import candidates".into()))?;
    if !rows(root, table, &mut 0, &mut candidates, total)? || candidates.len() != count {
        return Ok(false);
    }
    for candidate in &candidates {
        let Some(cell) = table.cell_mut(candidate.row, candidate.col) else { return Ok(false) };
        // An empty ODF paragraph carries no run. The constructor above supplied
        // its empty ordinary run so a legitimate empty String result is current.
        cell.text = candidate.body.clone();
        cell.formula = Some(candidate.formula.clone());
    }
    // Recalculation must not fix a forged source cache or paragraph. Compare its
    // fresh projection before restoring source-rich text and publishing metadata.
    let mut valid = table.recalculate_with_budget(budget).is_ok();
    budget.check_limits().map_err(|e| OdpError(e.to_string()))?;
    if valid {
        for candidate in &candidates {
            let Some(cell) = table.cell(candidate.row, candidate.col) else {
                valid = false;
                break;
            };
            let Some(formula) = &cell.formula else {
                valid = false;
                break;
            };
            if !cache_matches(&candidate.cache, formula.outcome()) {
                valid = false;
                break;
            }
            if let Some(format) = &cell.number_format {
                use deckcraft_model::table::{CellNumberKind, CellValue};
                let value = match formula.outcome() {
                    FormulaOutcome::Number { lexical } => Some(CellValue::Number { kind: CellNumberKind::Float, lexical: lexical.clone() }),
                    FormulaOutcome::Boolean { value } => Some(CellValue::Boolean { value: *value }),
                    FormulaOutcome::String { value } => Some(CellValue::String { value: value.clone() }),
                    FormulaOutcome::Error { .. } => None,
                };
                if let Some(value) = value
                    && !format.format_value(&value).is_ok_and(|value| super::format_color_matches(&candidate.body, value.color))
                {
                    valid = false;
                    break;
                }
            }
            if formula.uses_formatted_display() {
                if CellFormula::display_text(&cell.text).ok() != CellFormula::display_text(&candidate.body).ok() {
                    valid = false;
                    break;
                }
            } else if !candidate.formula.matches_display(&candidate.body).unwrap_or(false) {
                valid = false;
                break;
            }
        }
    }
    for candidate in &candidates {
        let Some(cell) = table.cell_mut(candidate.row, candidate.col) else { return Ok(false) };
        cell.text = candidate.body.clone();
        if valid {
            let Some(formula) = &cell.formula else { return Ok(false) };
            cell.formula = Some(Arc::new(
                CellFormula::from_profile_display(formula.profile(), formula.expression().into(), formula.outcome().clone(), &cell.text)
                    .map_err(|e| OdpError(e.to_string()))?,
            ));
        } else {
            cell.formula = None;
        }
    }
    if valid && table.validate_formulas_with_budget(budget).is_err() {
        budget.check_limits().map_err(|e| OdpError(e.to_string()))?;
        for candidate in &candidates {
            if let Some(cell) = table.cell_mut(candidate.row, candidate.col) {
                cell.formula = None;
            }
        }
        return Ok(false);
    }
    Ok(valid)
}
