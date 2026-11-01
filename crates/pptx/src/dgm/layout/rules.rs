//! Ordered, bounded numeric-rule search driven by measured text overflow.
//! Rules replace constraint values/factors, then the entire dependent layout is solved again.
use super::{Data, Instance, MAX_WORK, Placed, constraints, geometry};
use crate::xml::El;
use deckcraft_geom::Xfrm;
use std::collections::{HashMap, HashSet};

pub(crate) type Measure<'m> = dyn FnMut(&[Placed<'_>], bool) -> Result<Vec<f64>, &'static str> + 'm;

fn charge(work: &mut usize, amount: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(amount);
    if *work > MAX_WORK { Err("diagram rule budget exceeded") } else { Ok(()) }
}
fn unit(property: &str) -> f64 {
    if matches!(property, "primFontSz" | "secFontSz" | "pyraAcctRatio") || property.starts_with("user") { 1.0 } else { 72.0 / 25.4 }
}
struct Control<'a> {
    target: usize,
    property: String,
    rule: &'a El,
    constraint: Option<&'a El>,
}
fn controls<'a>(entries: &[constraints::Entry<'_, 'a>], data: &Data<'_>, work: &mut usize) -> Result<Vec<Control<'a>>, &'static str> {
    let mut effective = HashMap::new();
    for owner in (0..entries.len()).rev() {
        for c in &entries[owner].node.constraints {
            charge(work, entries[owner].end - owner)?;
            if matches!(c.attr("op"), Some("gte" | "lte")) {
                continue;
            }
            for target in constraints::selected(entries, owner, c.attr("for").unwrap_or("self"), c.attr("forName"), c.attr("ptType"), data) {
                let ty = c.attr("type").unwrap_or("");
                for property in if ty == "diam" { vec!["w", "h"] } else { vec![ty] } {
                    effective.insert((target, property.to_string()), *c);
                }
            }
        }
    }
    let mut out = Vec::new();
    // XML order is significant: grow a box first, or shrink its text first, as authored.
    for (owner, entry) in entries.iter().enumerate() {
        for rule in &entry.node.rules {
            charge(work, entry.end - owner)?;
            let ty = rule.attr("type").unwrap_or("");
            if rule.f64("val").zip(rule.f64("max")).is_some_and(|(v, m)| v > m) {
                return Err("diagram rule limit exceeds its maximum");
            }
            for target in constraints::selected(entries, owner, rule.attr("for").unwrap_or("self"), rule.attr("forName"), rule.attr("ptType"), data) {
                for property in if ty == "diam" { vec!["w", "h"] } else { vec![ty] } {
                    out.push(Control {
                        target,
                        property: property.into(),
                        rule,
                        constraint: effective.get(&(target, property.to_string())).copied(),
                    });
                }
            }
        }
    }
    Ok(out)
}
// Reference-linked text nodes must be measured together. A short source label must not
// suppress a font rule while a longer dependent label still overflows.
fn dependent_scope(entries: &[constraints::Entry<'_, '_>], data: &Data<'_>, target: usize, work: &mut usize) -> Result<HashSet<usize>, &'static str> {
    let mut affected: HashSet<usize> = (target..entries[target].end).collect();
    for _ in 0..entries.len() {
        let before = affected.len();
        for (owner, entry) in entries.iter().enumerate() {
            for c in &entry.node.constraints {
                if c.attr("refType").is_none_or(|t| matches!(t, "" | "none")) {
                    continue;
                }
                charge(work, entry.end - owner)?;
                let refs = constraints::selected(entries, owner, c.attr("refFor").unwrap_or("self"), c.attr("refForName"), c.attr("refPtType"), data);
                let targets = constraints::selected(entries, owner, c.attr("for").unwrap_or("self"), c.attr("forName"), c.attr("ptType"), data);
                for (i, t) in targets.iter().enumerate() {
                    let r = if refs.len() == targets.len() { refs.get(i) } else { refs.first() };
                    if r.is_some_and(|r| affected.contains(r)) {
                        affected.extend(*t..entries[*t].end);
                    }
                }
            }
        }
        if affected.len() == before {
            break;
        }
    }
    Ok(affected.into_iter().map(|i| constraints::id(entries[i].node)).collect())
}
fn placed<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &constraints::Adjustments,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    let solution = constraints::resolve_adjusted(tree, frame, data, work, changes)?;
    let mut out = Vec::new();
    geometry::place_solved(tree, frame, data, &mut out, 0, &solution, work)?;
    geometry::connect(&mut out, data);
    Ok(out)
}
fn score(out: &[Placed<'_>], scope: &HashSet<usize>, measure: &mut Measure<'_>, prefer_words: bool, work: &mut usize) -> Result<f64, &'static str> {
    charge(work, out.len())?;
    let values = measure(out, prefer_words)?;
    if values.len() != out.len() || values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err("invalid diagram text measurement");
    }
    Ok(out.iter().zip(values).filter(|(p, _)| scope.contains(&p.node_key)).map(|(_, v)| v).sum())
}

pub(super) fn search<'a>(tree: &Instance<'a>, frame: Xfrm, data: &Data<'a>, measure: &mut Measure<'_>) -> Result<Vec<Placed<'a>>, &'static str> {
    use constraints::Adjustment::{Factor, Value};
    let mut work = 0;
    let mut entries = Vec::new();
    constraints::collect(tree, geometry::BoxStyle::new(frame), None, &mut entries, &mut work)?;
    let controls = controls(&entries, data, &mut work)?;
    let mut changes = constraints::Adjustments::new();
    let mut grown = Vec::new();
    let mut font_changed = HashSet::new();
    let mut out = placed(tree, frame, data, &changes, &mut work)?;
    for control in controls {
        let entry = &entries[control.target];
        let scope = dependent_scope(&entries, data, control.target, &mut work)?;
        let prefer_words = matches!(control.property.as_str(), "primFontSz" | "secFontSz");
        let before = score(&out, &scope, measure, prefer_words, &mut work)?;
        if before <= 1e-6 {
            continue;
        }
        let key = (constraints::id(entry.node), control.property.clone());
        let solution = constraints::resolve_adjusted(tree, frame, data, &mut work, &changes)?;
        let Some(current) = constraints::lookup(&solution, entry.node).and_then(|s| s.get(&control.property)) else { continue };
        let scale = unit(&control.property);
        let (start, limit, is_factor) = if let Some(value) = control.rule.f64("val") {
            (current / scale, value, false)
        } else if let Some(factor) = control.rule.f64("fact") {
            let Some(c) = control.constraint.filter(|c| c.attr("refType").is_some_and(|t| !matches!(t, "" | "none" | "unknown"))) else {
                return Err("factor rule has no reference constraint");
            };
            let start = match changes.get(&key) {
                Some(Factor(v)) => *v,
                _ => c.f64("fact").unwrap_or(1.0),
            };
            let limit = if let Some(max) = control.rule.f64("max") {
                if current.abs() > 1e-9 { factor.min(max * scale / current * start) } else { factor }
            } else {
                factor
            };
            (start, limit, true)
        } else {
            continue;
        };
        if !start.is_finite() || !limit.is_finite() || (start - limit).abs() < 1e-9 {
            continue;
        }
        let old = changes.get(&key).copied();
        changes.insert(key.clone(), if is_factor { Factor(limit) } else { Value(limit) });
        let candidate = placed(tree, frame, data, &changes, &mut work)?;
        let after = score(&candidate, &scope, measure, prefer_words, &mut work)?;
        if after >= before - 1e-6 {
            if let Some(v) = old {
                changes.insert(key, v);
            } else {
                changes.remove(&key);
            }
            continue;
        }
        if prefer_words {
            font_changed.extend(scope.iter().copied());
        } else if limit > start && matches!(control.property.as_str(), "w" | "h") {
            grown.push((key.clone(), start, is_factor, scope.clone()));
        }
        out = candidate;
        if after > 1e-6 {
            continue;
        }
        // Stop at the first fitting value rather than always applying the extreme limit.
        // Every trial is measured using the exact renderer's glyph/paragraph metrics.
        let (mut low, mut high) = (0.0, 1.0);
        let mut best = limit;
        for _ in 0..18 {
            let t = (low + high) / 2.0;
            let value = start + (limit - start) * t;
            changes.insert(key.clone(), if is_factor { Factor(value) } else { Value(value) });
            let candidate = placed(tree, frame, data, &changes, &mut work)?;
            if score(&candidate, &scope, measure, prefer_words, &mut work)? <= 1e-6 {
                high = t;
                best = value;
                out = candidate;
            } else {
                low = t;
            }
            if high - low < 1e-6 {
                break;
            }
        }
        if prefer_words && !is_factor && start > limit {
            best = best.floor().max(limit);
            changes.insert(key.clone(), Value(best));
            out = placed(tree, frame, data, &changes, &mut work)?;
        }
        changes.insert(key, if is_factor { Factor(best) } else { Value(best) });
    }
    // A later font rule can make previously required box growth redundant. Keep the selected
    // font and return space to neighbours, stopping at the first non-overflowing geometry.
    for (key, start, is_factor, scope) in grown {
        if scope.is_disjoint(&font_changed) || score(&out, &scope, measure, true, &mut work)? > 1e-6 {
            continue;
        }
        let Some(change) = changes.get(&key).copied() else { continue };
        let (Value(limit) | Factor(limit)) = change;
        let (mut low, mut high) = (0.0, 1.0);
        let mut best = limit;
        for _ in 0..18 {
            let t = (low + high) / 2.0;
            let value = start + (limit - start) * t;
            changes.insert(key.clone(), if is_factor { Factor(value) } else { Value(value) });
            let candidate = placed(tree, frame, data, &changes, &mut work)?;
            if score(&candidate, &scope, measure, true, &mut work)? <= 1e-6 {
                high = t;
                best = value;
                out = candidate;
            } else {
                low = t;
            }
            if high - low < 1e-6 {
                break;
            }
        }
        changes.insert(key, if is_factor { Factor(best) } else { Value(best) });
    }
    Ok(out)
}
