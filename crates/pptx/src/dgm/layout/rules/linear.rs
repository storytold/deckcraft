//! Ordered local text rules around a linear allocation pass.
//! Independent children retain their allocated dimensions for subsequent rules.
use super::{
    Choice, Control, Data, Instance, Measure, Placed, Range, Xfrm, charge, choose, constraints, control_scope, geometry, range, score, unit,
};
use std::collections::HashSet;

struct Flow {
    index: usize,
    primary: &'static str,
    children: HashSet<usize>,
}

// This staged path applies to independent, unrotated rectangular text leaves.
// Referenced constraints, other shapes, and enclosing algorithms keep the general
// solver, whose geometry cannot be restored from these local dimensions alone.
fn eligible(entries: &[constraints::Entry<'_, '_>], controls: &[Control<'_>], work: &mut usize) -> Result<Option<Flow>, &'static str> {
    charge(work, controls.len().saturating_add(entries.len()))?;
    let Some(parent) = controls.first().and_then(|c| entries.get(c.target)).and_then(|e| e.parent) else { return Ok(None) };
    let Some(flow) = entries.get(parent) else { return Ok(None) };
    if flow.node.alg != "lin" || flow.node.children.is_empty() {
        return Ok(None);
    }
    if controls.iter().any(|c| {
        entries.get(c.target).is_none_or(|e| e.parent != Some(parent))
            || c.rule.f64("val").is_none()
            || !matches!(c.property.as_str(), "w" | "h" | "primFontSz" | "secFontSz")
            || c.members.len() != 1
    }) {
        return Ok(None);
    }
    let mut ancestor = flow.parent;
    while let Some(index) = ancestor {
        charge(work, 1)?;
        let Some(entry) = entries.get(index) else { return Ok(None) };
        if entry.node.alg != "composite" {
            return Ok(None);
        }
        ancestor = entry.parent;
    }
    let mut children = HashSet::new();
    for entry in entries {
        charge(work, entry.node.constraints.len())?;
        if entry.node.constraints.iter().any(|c| c.attr("refType").is_some_and(|v| !matches!(v, "none" | ""))) {
            return Ok(None);
        }
        if entry.parent == Some(parent) {
            let n = entry.node;
            if n.alg != "tx"
                || !n.children.is_empty()
                || n.number("txRot", 0.) != 0.
                || n.shape.is_some_and(|s| s.attr("type") != Some("rect") || s.f64("rot").unwrap_or(0.) != 0. || s.child("adjLst").is_some())
            {
                return Ok(None);
            }
            children.insert(constraints::id(n));
        }
    }
    let primary = if matches!(flow.node.param("linDir", "fromL").as_str(), "fromT" | "fromB") { "h" } else { "w" };
    Ok(Some(Flow { index: parent, primary, children }))
}

fn placed<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &constraints::Adjustments,
    flow: &Flow,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    let mut solution = constraints::resolve_adjusted(tree, frame, data, work, changes)?;
    charge(work, solution.len())?;
    for style in solution.values_mut() {
        style.font = style.font.map(f64::round);
        style.secondary_font = style.secondary_font.map(f64::round);
    }
    let mut out = Vec::new();
    geometry::place_solved(tree, frame, data, &mut out, 0, &solution, Some(&HashSet::new()), work)?;
    charge(work, out.len())?;
    for p in &mut out {
        if flow.children.contains(&p.node_key)
            && let Some(style) = solution.get(&p.node_key)
        {
            // Local rules may grow along the flow before siblings are packed, but
            // their cross-axis text budget is still the enclosing capacity.
            if flow.primary == "w" {
                p.rect.w = style.rect.w;
                if let Some(t) = &mut p.text_rect {
                    t.w = style.rect.w;
                }
            } else {
                p.rect.h = style.rect.h;
                if let Some(t) = &mut p.text_rect {
                    t.h = style.rect.h;
                }
            }
        }
    }
    geometry::connect(&mut out, data);
    Ok(out)
}

fn allocated<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &constraints::Adjustments,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    let mut solution = constraints::resolve_adjusted(tree, frame, data, work, changes)?;
    charge(work, solution.len())?;
    for style in solution.values_mut() {
        style.font = style.font.map(f64::round);
        style.secondary_font = style.secondary_font.map(f64::round);
    }
    let mut out = Vec::new();
    geometry::place_solved(tree, frame, data, &mut out, 0, &solution, Some(&HashSet::new()), work)?;
    geometry::connect(&mut out, data);
    Ok(out)
}

fn trial<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &constraints::Adjustments,
    flow: &Flow,
    work: &mut usize,
) -> Result<Option<Vec<Placed<'a>>>, &'static str> {
    match placed(tree, frame, data, changes, flow, work) {
        Ok(out) => Ok(Some(out)),
        Err("inconsistent diagram constraints" | "inconsistent diagram bounds" | "negative diagram extent" | "inconsistent diagram extent signs") => {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn search<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    entries: &[constraints::Entry<'_, 'a>],
    controls: &[Control<'a>],
    measure: &mut Measure<'_>,
    work: &mut usize,
) -> Result<Option<Vec<Placed<'a>>>, &'static str> {
    use constraints::Adjustment::Value;
    let Some(flow) = eligible(entries, controls, work)? else { return Ok(None) };
    let mut by_target = std::collections::HashMap::<usize, Vec<&Control<'_>>>::new();
    charge(work, controls.len().saturating_add(entries.len()))?;
    for control in controls {
        by_target.entry(control.target).or_default().push(control);
    }
    // Each child resolves through its first flow-axis rule before the siblings
    // share available space. Remaining authored rules see that new allocation;
    // fitting each child's entire rule list first loses this dependency.
    let mut steps = Vec::with_capacity(controls.len().saturating_add(1));
    let mut rest = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.parent != Some(flow.index) {
            continue;
        }
        let mut crossed = false;
        for c in by_target.remove(&index).unwrap_or_default() {
            if crossed {
                rest.push(Some(c));
            } else {
                steps.push(Some(c));
            }
            crossed |= c.property == flow.primary;
        }
    }
    steps.push(None);
    steps.extend(rest);
    let mut changes = constraints::Adjustments::new();
    let mut grown: Vec<(Range, HashSet<usize>, bool)> = Vec::new();
    let mut out = placed(tree, frame, data, &changes, &flow, work)?;
    for step in steps {
        let Some(control) = step else {
            // Persist capacity allocation only. Measuring/refitting text here
            // and again at the final placement would shrink the flow twice.
            out = allocated(tree, frame, data, &changes, work)?;
            charge(work, out.len().saturating_mul(2))?;
            for p in &out {
                if flow.children.contains(&p.node_key) {
                    changes.insert((p.node_key, "w".into()), Value(p.rect.w / unit("w")));
                    changes.insert((p.node_key, "h".into()), Value(p.rect.h / unit("h")));
                }
            }
            out = placed(tree, frame, data, &changes, &flow, work)?;
            continue;
        };
        let scope = control_scope(entries, data, control, work)?;
        let font_rule = matches!(control.property.as_str(), "primFontSz" | "secFontSz");
        let size_rule = matches!(control.property.as_str(), "w" | "h");
        // Width rules keep searching when an emergency word break hides overflow.
        // Height rules fit the resulting lines without requiring every word intact.
        let prefer_words = font_rule || control.property == "w";
        let before = score(&out, &scope, measure, prefer_words, work)?;
        if before <= 1e-6 {
            continue;
        }
        let solution = constraints::resolve_adjusted(tree, frame, data, work, &changes)?;
        let Some(range) = range(control, entries, data, &solution, &changes)? else { continue };
        let (start, limit, is_factor) = (range.start, range.limit, range.is_factor);
        charge(work, range.keys.len())?;
        let old: Vec<_> = range.keys.iter().map(|k| (k.clone(), changes.get(k).copied())).collect();
        let selected = choose(font_rule || size_rule, |t| {
            let value = start + (limit - start) * t;
            range.set_value(&mut changes, value);
            let Some(candidate) = trial(tree, frame, data, &changes, &flow, work)? else { return Ok(None) };
            let score = score(&candidate, &scope, measure, prefer_words, work)?;
            Ok(Some(Choice { out: candidate, score, t, low: 0. }))
        })?;
        let Some(Choice { out: candidate, score: after, t: selected, low: bracket }) = selected.filter(|c| c.score < before - 1e-6) else {
            for (key, value) in old {
                if let Some(value) = value {
                    changes.insert(key, value);
                } else {
                    changes.remove(&key);
                }
            }
            continue;
        };
        if font_rule {
            for (_, grown_scope, refit) in grown.iter_mut().rev() {
                charge(work, grown_scope.len().min(scope.len()).saturating_add(1))?;
                if !scope.is_disjoint(grown_scope) {
                    *refit = true;
                    break;
                }
            }
        } else if limit > start && matches!(control.property.as_str(), "w" | "h") {
            let mut remembered = range.clone();
            remembered.limit = start + (limit - start) * selected;
            grown.push((remembered, scope.clone(), false));
        }
        out = candidate;
        let selected_value = start + (limit - start) * selected;
        range.set_value(&mut changes, selected_value);
        if after > 1e-6 {
            continue;
        }
        // Stop at the first fitting value rather than always applying the extreme limit.
        // Every trial is measured using the exact renderer's glyph/paragraph metrics.
        let (mut low, mut high) = (bracket, selected);
        let mut best = selected_value;
        // Native successful size-rule endpoints use five bisections over the
        // authored range. Keep the existing interior search for rejected endpoints.
        let steps = if size_rule && selected == 1. && bracket == 0. { 5 } else { 18 };
        for _ in 0..steps {
            let t = (low + high) / 2.0;
            let value = start + (limit - start) * t;
            range.set_value(&mut changes, value);
            let candidate = trial(tree, frame, data, &changes, &flow, work)?;
            if let Some(candidate) = candidate
                && score(&candidate, &scope, measure, prefer_words, work)? <= 1e-6
            {
                high = t;
                best = value;
                out = candidate;
            } else {
                low = t;
            }
            // Font searches stop at a one-point interval. Keep the unrounded
            // fitting endpoint for references.
            if high - low < 1e-6 || font_rule && !is_factor && (high - low) * (start - limit).abs() <= 1. {
                break;
            }
        }
        range.set_value(&mut changes, best);
        charge(work, grown.len())?;
        if let Some((remembered, _, _)) = grown.iter_mut().rev().find(|(r, _, _)| r.key == range.key) {
            remembered.limit = best;
        }
    }
    // Only a later font rule makes earlier box growth redundant. Keep the selected
    // font and return space to neighbours, stopping at the first non-overflowing geometry.
    for (range, scope, refit) in grown {
        let start = range.start;
        if !refit || score(&out, &scope, measure, true, work)? > 1e-6 {
            continue;
        }
        let limit = range.limit;
        range.set_value(&mut changes, start);
        if let Some(candidate) = trial(tree, frame, data, &changes, &flow, work)?
            && score(&candidate, &scope, measure, true, work)? <= 1e-6
        {
            out = candidate;
            continue;
        }
        let (mut low, mut high) = (0.0, 1.0);
        let mut best = limit;
        for _ in 0..5 {
            let t = (low + high) / 2.0;
            let value = start + (limit - start) * t;
            range.set_value(&mut changes, value);
            let candidate = trial(tree, frame, data, &changes, &flow, work)?;
            if let Some(candidate) = candidate
                && score(&candidate, &scope, measure, true, work)? <= 1e-6
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
        range.set_value(&mut changes, best);
    }
    Ok(Some(super::placed(tree, frame, data, &changes, measure, work)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_requires_independent_unrotated_text_leaves_and_obeys_budget() {
        let xml = crate::xml::parse(b"<node/>").unwrap().root;
        let rule = crate::xml::parse(b"<rule type='w' val='80'/>").unwrap().root;
        let reference = crate::xml::parse(b"<constr type='w' refType='w' fact='0.5'/>").unwrap().root;
        for (shape_xml, alg, dependent, supported) in [
            ("<shape type='rect'/>", "lin", false, true),
            ("<shape type='rect' rot='30'/>", "lin", false, false),
            ("<shape type='ellipse'/>", "lin", false, false),
            ("<shape type='rect'/>", "snake", false, false),
            ("<shape type='rect'/>", "lin", true, false),
        ] {
            let shape = crate::xml::parse(shape_xml.as_bytes()).unwrap().root;
            let mut tree = Instance::new(&xml);
            tree.alg = alg.into();
            let mut leaf = Instance::new(&xml);
            leaf.alg = "tx".into();
            leaf.shape = Some(&shape);
            if dependent {
                leaf.constraints.push(&reference);
            }
            tree.children.push(leaf);
            let mut entries = Vec::new();
            constraints::collect(&tree, geometry::BoxStyle::new(Xfrm::new(0., 0., 300., 200.)), None, &mut entries, &mut 0).unwrap();
            let controls = [Control { target: 1, members: vec![1], property: "w".into(), rule: &rule, constraint: None }];
            assert_eq!(eligible(&entries, &controls, &mut 0).unwrap().is_some(), supported, "{shape_xml}, {alg}, dependent={dependent}");
            let mut exhausted = super::super::MAX_WORK;
            assert!(eligible(&entries, &controls, &mut exhausted).is_err());
        }
    }
}
