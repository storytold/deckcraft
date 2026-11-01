//! Ordered, bounded numeric-rule search driven by measured text overflow.
//! Rules replace constraint values/factors, then the entire dependent layout is solved again.
mod coupled;

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
fn dependent_scope(
    entries: &[constraints::Entry<'_, '_>],
    data: &Data<'_>,
    target: usize,
    property: &str,
    work: &mut usize,
) -> Result<HashSet<usize>, &'static str> {
    let mut affected: HashSet<(usize, String)> = HashSet::new();
    let mut add = |target: usize, property: &str| {
        for i in target..entries[target].end {
            affected.insert((i, property.into()));
            if property == "primFontSz" {
                affected.insert((i, "secFontSz".into()));
            }
        }
    };
    add(target, if matches!(property, "primFontSz" | "secFontSz") { property } else { "*" });
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
                    if r.is_some_and(|r| affected.contains(&(*r, "*".into())) || affected.contains(&(*r, c.attr("refType").unwrap_or("").into()))) {
                        let ty = c.attr("type").unwrap_or("");
                        let ty = if matches!(ty, "primFontSz" | "secFontSz") { ty } else { "*" };
                        for i in *t..entries[*t].end {
                            affected.insert((i, ty.into()));
                            if ty == "primFontSz" {
                                affected.insert((i, "secFontSz".into()));
                            }
                        }
                    }
                }
            }
        }
        if affected.len() == before {
            break;
        }
    }
    Ok(affected.into_iter().map(|(i, _)| constraints::id(entries[i].node)).collect())
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
/// An infeasible trial is a rejected candidate, not a failure of the whole diagram. Never
/// swallow malformed-input, measurement or resource-budget errors while searching.
fn trial<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &constraints::Adjustments,
    work: &mut usize,
) -> Result<Option<Vec<Placed<'a>>>, &'static str> {
    match placed(tree, frame, data, changes, work) {
        Ok(out) => Ok(Some(out)),
        Err("inconsistent diagram constraints" | "inconsistent diagram bounds" | "negative diagram extent") => Ok(None),
        Err(e) => Err(e),
    }
}
fn score(out: &[Placed<'_>], scope: &HashSet<usize>, measure: &mut Measure<'_>, prefer_words: bool, work: &mut usize) -> Result<f64, &'static str> {
    charge(work, out.len())?;
    let values = measure(out, prefer_words)?;
    if values.len() != out.len() || values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        return Err("invalid diagram text measurement");
    }
    Ok(out.iter().zip(values).filter(|(p, _)| scope.contains(&p.node_key)).map(|(_, v)| v).sum())
}

struct Choice<'a> {
    out: Vec<Placed<'a>>,
    score: f64,
    t: f64,
    low: f64,
}
fn choose<'a>(
    prefer_words: bool,
    mut probe: impl FnMut(f64) -> Result<Option<Choice<'a>>, &'static str>,
) -> Result<Option<Choice<'a>>, &'static str> {
    let mut best = probe(1.)?;
    // Geometry can trade width for height, so its endpoint need not be the best fit.
    // Font endpoints keep the fast monotonic path; otherwise search the interior too.
    if !prefer_words || best.as_ref().is_none_or(|c| c.score > 1e-6) {
        for step in 1..16 {
            if let Some(mut candidate) = probe(step as f64 / 16.)? {
                let fit = candidate.score <= 1e-6;
                if best.as_ref().is_none_or(|old| candidate.score < old.score - 1e-6 || (fit && candidate.t < old.t)) {
                    if fit {
                        candidate.low = (step - 1) as f64 / 16.;
                    }
                    best = Some(candidate);
                }
                if fit {
                    break;
                }
            }
        }
    }
    Ok(best)
}

struct Range {
    key: (usize, String),
    start: f64,
    limit: f64,
    is_factor: bool,
    maximum: Option<f64>,
}
impl Range {
    fn set(&self, changes: &mut constraints::Adjustments, t: f64) {
        let value = self.start + (self.limit - self.start) * t;
        changes.insert(self.key.clone(), if self.is_factor { constraints::Adjustment::Factor(value) } else { constraints::Adjustment::Value(value) });
    }
}
fn range(
    control: &Control<'_>,
    entry: &constraints::Entry<'_, '_>,
    solution: &constraints::Solution,
    changes: &constraints::Adjustments,
) -> Result<Option<Range>, &'static str> {
    use constraints::Adjustment::Factor;
    let key = (constraints::id(entry.node), control.property.clone());
    let Some(current) = constraints::lookup(solution, entry.node).and_then(|s| s.get(&control.property)) else { return Ok(None) };
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
        return Ok(None);
    };
    Ok((start.is_finite() && limit.is_finite() && (start - limit).abs() >= 1e-9).then_some(Range {
        key,
        start,
        limit,
        is_factor,
        maximum: control.rule.f64("max").map(|v| v * scale),
    }))
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
    for control in &controls {
        let entry = &entries[control.target];
        let scope = dependent_scope(&entries, data, control.target, &control.property, &mut work)?;
        let prefer_words = matches!(control.property.as_str(), "primFontSz" | "secFontSz");
        let before = score(&out, &scope, measure, prefer_words, &mut work)?;
        if before <= 1e-6 {
            continue;
        }
        let solution = constraints::resolve_adjusted(tree, frame, data, &mut work, &changes)?;
        let Some(Range { key, start, limit, is_factor, .. }) = range(control, entry, &solution, &changes)? else { continue };
        let old = changes.get(&key).copied();
        let selected = choose(prefer_words, |t| {
            let value = start + (limit - start) * t;
            changes.insert(key.clone(), if is_factor { Factor(value) } else { Value(value) });
            let Some(candidate) = trial(tree, frame, data, &changes, &mut work)? else { return Ok(None) };
            let score = score(&candidate, &scope, measure, prefer_words, &mut work)?;
            Ok(Some(Choice { out: candidate, score, t, low: 0. }))
        })?;
        let Some(Choice { out: candidate, score: after, t: selected, low: bracket }) = selected.filter(|c| c.score < before - 1e-6) else {
            if let Some(v) = old {
                changes.insert(key, v);
            } else {
                changes.remove(&key);
            }
            continue;
        };
        if prefer_words {
            font_changed.extend(scope.iter().copied());
        } else if limit > start && matches!(control.property.as_str(), "w" | "h") {
            grown.push((key.clone(), start, is_factor, scope.clone()));
        }
        out = candidate;
        let selected_value = start + (limit - start) * selected;
        changes.insert(key.clone(), if is_factor { Factor(selected_value) } else { Value(selected_value) });
        if after > 1e-6 {
            continue;
        }
        // Stop at the first fitting value rather than always applying the extreme limit.
        // Every trial is measured using the exact renderer's glyph/paragraph metrics.
        let (mut low, mut high) = (bracket, selected);
        let mut best = selected_value;
        for _ in 0..18 {
            let t = (low + high) / 2.0;
            let value = start + (limit - start) * t;
            changes.insert(key.clone(), if is_factor { Factor(value) } else { Value(value) });
            let candidate = trial(tree, frame, data, &changes, &mut work)?;
            if let Some(candidate) = candidate
                && score(&candidate, &scope, measure, prefer_words, &mut work)? <= 1e-6
            {
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
            let rounded = best.floor().max(limit);
            changes.insert(key.clone(), Value(rounded));
            if let Some(candidate) = trial(tree, frame, data, &changes, &mut work)? {
                best = rounded;
                out = candidate;
            }
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
            let candidate = trial(tree, frame, data, &changes, &mut work)?;
            if let Some(candidate) = candidate
                && score(&candidate, &scope, measure, true, &mut work)? <= 1e-6
            {
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
    coupled::search(tree, frame, data, &entries, &controls, measure, &mut changes, &mut out, &mut work)?;
    Ok(out)
}
