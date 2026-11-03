//! Ordered local text rules around a linear allocation pass.
//! Independent children retain their allocated dimensions for subsequent rules.
use super::{
    Choice, Control, Data, Instance, Measure, Placed, Range, Xfrm, charge, choose, constraints, control_scope, geometry, progress, range, score, unit,
};
use std::collections::HashSet;

struct Growth {
    range: Range,
    scope: HashSet<usize>,
    refit: bool,
    authored_limit: f64,
}

struct Flow {
    index: usize,
    primary: &'static str,
    children: HashSet<usize>,
}

// Independent text leaves retain local footprint dimensions between rule stages.
// References and enclosing algorithms keep the general solver because their
// dependent geometry cannot be restored from these local dimensions alone.
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
            if n.alg != "tx" || !n.children.is_empty() {
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
    let mut out = footprints(tree, frame, data, &solution, false, work)?;
    charge(work, out.len())?;
    for p in &mut out {
        if flow.children.contains(&p.node_key)
            && let Some(style) = solution.get(&p.node_key)
        {
            // Local rules may grow along the flow before siblings are packed, but
            // their cross-axis text budget is still the enclosing capacity.
            if flow.primary == "w" {
                p.rect.w = style.rect.w;
            } else {
                p.rect.h = style.rect.h;
            }
        }
    }
    super::super::orientation::apply(&mut out, work)?;
    geometry::connect(&mut out, data);
    Ok(out)
}

fn footprints<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    solution: &constraints::Solution,
    refit: bool,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    let mut out = Vec::new();
    let style = constraints::lookup(solution, tree).unwrap_or_else(|| geometry::BoxStyle::new(frame));
    let refits = if refit { HashSet::from([constraints::id(tree)]) } else { HashSet::new() };
    geometry::place_inner(tree, style, Some(frame), data, &mut out, 0, solution, Some(&refits), work)?;
    out.retain(|p| p.shape.is_some() || (p.text_enabled && p.points.iter().any(|pt| pt.attr("type") != Some("doc"))));
    Ok(out)
}

fn allocated<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &constraints::Adjustments,
    refit: bool,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    let mut solution = constraints::resolve_adjusted(tree, frame, data, work, changes)?;
    charge(work, solution.len())?;
    for style in solution.values_mut() {
        style.font = style.font.map(f64::round);
        style.secondary_font = style.secondary_font.map(f64::round);
    }
    footprints(tree, frame, data, &solution, refit, work)
}

fn retain_allocation(
    out: &[Placed<'_>],
    changes: &mut constraints::Adjustments,
    flow: &Flow,
    local_primary: &std::collections::HashMap<usize, f64>,
    work: &mut usize,
) -> Result<bool, &'static str> {
    use constraints::Adjustment::Value;
    charge(work, out.len().saturating_mul(2))?;
    let mut reduced = false;
    for p in out {
        if !flow.children.contains(&p.node_key) {
            continue;
        }
        let actual = if flow.primary == "w" { p.rect.w } else { p.rect.h } / unit(flow.primary);
        if let Some(&before) = local_primary.get(&p.node_key) {
            reduced |= before - actual > 16. * f64::EPSILON * before.abs().max(actual.abs()).max(1.);
        }
        changes.insert((p.node_key, "w".into()), Value(p.rect.w / unit("w")));
        changes.insert((p.node_key, "h".into()), Value(p.rect.h / unit("h")));
    }
    Ok(reduced)
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

fn text_budget_recedes(before: &[Placed<'_>], after: &[Placed<'_>], scope: &HashSet<usize>, work: &mut usize) -> Result<bool, &'static str> {
    charge(work, before.len())?;
    if before.len() != after.len() {
        return Err("diagram rule shape binding changed");
    }
    let smaller = |a: f64, b: f64| a - b > 16. * f64::EPSILON * a.abs().max(b.abs()).max(1.);
    Ok(before.iter().zip(after).filter(|(p, _)| scope.contains(&p.node_key)).any(|(a, b)| {
        let a = a.text_rect.unwrap_or(a.rect);
        let b = b.text_rect.unwrap_or(b.rect);
        smaller(a.w, b.w) || smaller(a.h, b.h)
    }))
}

fn retain_growth(
    range: &Range,
    choice: &Choice<'_>,
    before: &[Placed<'_>],
    scope: &HashSet<usize>,
    primary: &str,
    work: &mut usize,
) -> Result<bool, &'static str> {
    if !progress::retain_endpoint(range, choice) {
        return Ok(false);
    }
    // A cross-axis endpoint clipped to the existing capacity reserves no new
    // text budget. Remembering it would alter a later font reclaim.
    Ok(range.key.1 == primary || text_budget_recedes(before, &choice.out, scope, work)? || text_budget_recedes(&choice.out, before, scope, work)?)
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
    let Some(flow) = eligible(entries, controls, work)? else { return Ok(None) };
    let nested = entries.get(flow.index).is_some_and(|entry| entry.parent.is_some());
    let out = search_pass(tree, frame, data, entries, controls, &flow, !nested, measure, work)?;
    // A nested allocation must first settle its whole text scope. Incomplete
    // sibling fits keep the allocation pass's search history. A root flow owns
    // its final refit and can retain local checkpoints even with residual text.
    if nested && score(&out, &flow.children, measure, true, work)? <= 1e-6 {
        let candidate = search_pass(tree, frame, data, entries, controls, &flow, true, measure, work)?;
        if score(&candidate, &flow.children, measure, true, work)? <= 1e-6 {
            return Ok(Some(candidate));
        }
    }
    Ok(Some(out))
}

struct Step<'c, 'a> {
    control: Option<&'c Control<'a>>,
    replay: bool,
}

fn steps<'c, 'a>(
    entries: &[constraints::Entry<'_, 'a>],
    controls: &'c [Control<'a>],
    flow: &Flow,
    work: &mut usize,
) -> Result<Vec<Step<'c, 'a>>, &'static str> {
    let mut by_target = std::collections::HashMap::<usize, Vec<&Control<'_>>>::new();
    charge(work, controls.len().saturating_add(entries.len()))?;
    for control in controls {
        by_target.entry(control.target).or_default().push(control);
    }
    // Each child resolves through its first flow-axis rule before the siblings
    // share available space. Remaining authored rules see that new allocation;
    // fitting each child's entire rule list first loses this dependency.
    let mut steps = Vec::with_capacity(controls.len().saturating_mul(2).saturating_add(1));
    let mut rest = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        if entry.parent != Some(flow.index) {
            continue;
        }
        let mut crossed = false;
        for c in by_target.remove(&index).unwrap_or_default() {
            if crossed {
                rest.push(Step { control: Some(c), replay: false });
            } else {
                steps.push(Step { control: Some(c), replay: false });
                if c.property != flow.primary {
                    rest.push(Step { control: Some(c), replay: true });
                }
            }
            crossed |= c.property == flow.primary;
        }
    }
    steps.push(Step { control: None, replay: false });
    steps.extend(rest);
    Ok(steps)
}

#[derive(Default)]
struct Replay {
    original: std::collections::HashMap<(usize, String), Range>,
    seen: HashSet<(usize, String)>,
}

// Capacity allocation can change wrapping after a child's local rules ran.
// Nested flows reconsider their cross dimension from its original seed; a root
// preserves a fitting checkpoint. Repeated rules then consume the preceding
// result in authored order, rather than resetting the same seed repeatedly.
#[allow(clippy::too_many_arguments)]
fn replay<'a>(
    state: &mut Replay,
    control: &Control<'a>,
    entries: &[constraints::Entry<'_, 'a>],
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &mut constraints::Adjustments,
    flow: &Flow,
    scope: &HashSet<usize>,
    out: &mut Vec<Placed<'a>>,
    measure: &mut Measure<'_>,
    work: &mut usize,
) -> Result<bool, &'static str> {
    charge(work, 1)?;
    let Some(entry) = entries.get(control.target) else { return Ok(false) };
    let key = (constraints::id(entry.node), control.property.clone());
    if !matches!(control.property.as_str(), "w" | "h") || !state.seen.insert(key.clone()) {
        return Ok(true);
    }
    if flow.index == 0 && score(out, scope, measure, control.property == "w", work)? <= 1e-6 {
        return Ok(false);
    }
    let Some(initial) = state.original.get(&key) else { return Ok(false) };
    initial.set_value(changes, initial.start);
    *out = placed(tree, frame, data, changes, flow, work)?;
    Ok(true)
}

#[allow(clippy::too_many_arguments)]
fn search_pass<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    entries: &[constraints::Entry<'_, 'a>],
    controls: &[Control<'a>],
    flow: &Flow,
    retain_checkpoint: bool,
    measure: &mut Measure<'_>,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    use constraints::Adjustment::Value;
    let steps = steps(entries, controls, flow, work)?;
    let mut changes = constraints::Adjustments::new();
    let mut local_primary = std::collections::HashMap::new();
    let mut replay_state = Replay::default();
    // Remember an actual capacity reduction, not merely that allocation ran.
    // A root rule search retains that reduction when it reaches final text fit.
    let mut allocated_fit = false;
    let mut grown = Vec::<Growth>::new();
    let mut coupled = HashSet::new();
    let mut out = placed(tree, frame, data, &changes, flow, work)?;
    for Step { control: step, replay: repeat_cross } in steps {
        // Apply the previous font rule before later dimensions change its fit budget.
        out = reclaim(tree, frame, data, &mut changes, flow, &mut grown, measure, work, out)?;
        let Some(control) = step else {
            // Persist capacity allocation only. Measuring/refitting text here
            // and again at the final placement would shrink the flow twice.
            let local = constraints::resolve_adjusted(tree, frame, data, work, &changes)?;
            charge(work, flow.children.len())?;
            for key in &flow.children {
                if let Some(value) = local.get(key).and_then(|style| style.get(flow.primary)) {
                    local_primary.insert(*key, value / unit(flow.primary));
                }
            }
            out = allocated(tree, frame, data, &changes, false, work)?;
            allocated_fit |= retain_allocation(&out, &mut changes, flow, &local_primary, work)?;
            out = placed(tree, frame, data, &changes, flow, work)?;
            continue;
        };
        let scope = control_scope(entries, data, control, work)?;
        if repeat_cross
            && (!allocated_fit
                || !replay(&mut replay_state, control, entries, tree, frame, data, &mut changes, flow, &scope, &mut out, measure, work)?)
        {
            continue;
        }
        let font_rule = matches!(control.property.as_str(), "primFontSz" | "secFontSz");
        let size_rule = matches!(control.property.as_str(), "w" | "h");
        // Width rules keep searching when an emergency word break hides overflow.
        // Height rules fit the resulting lines without requiring every word intact.
        let prefer_words = font_rule || control.property == "w";
        let solution = constraints::resolve_adjusted(tree, frame, data, work, &changes)?;
        let Some(range) = range(control, entries, data, &solution, &changes)? else { continue };
        if !repeat_cross {
            replay_state.original.entry(range.key.clone()).or_insert_with(|| range.clone());
        }
        let before = score(&out, &scope, measure, prefer_words, work)?;
        if before <= 1e-6 {
            continue;
        }
        let (start, limit, is_factor) = (range.start, range.limit, range.is_factor);
        if size_rule && !is_factor && limit <= start {
            continue;
        }
        charge(work, range.keys.len())?;
        let old: Vec<_> = range.keys.iter().map(|k| (k.clone(), changes.get(k).copied())).collect();
        let probe = |t| {
            let value = start + (limit - start) * t;
            range.set_value(&mut changes, value);
            let Some(candidate) = trial(tree, frame, data, &changes, flow, work)? else { return Ok(None) };
            if size_rule && limit > start && t == 1. && text_budget_recedes(&out, &candidate, &scope, work)? {
                coupled.insert(control.target);
            }
            let score = score(&candidate, &scope, measure, prefer_words, work)?;
            Ok(Some(Choice { out: candidate, score, t, low: 0. }))
        };
        let selected = if size_rule && !is_factor { progress::choose_growth(probe)? } else { choose(font_rule || size_rule, probe)? };
        let retain_endpoint = selected.as_ref().map(|c| retain_growth(&range, c, &out, &scope, flow.primary, work)).transpose()?.unwrap_or(false);
        let Some(Choice { out: candidate, score: after, t: selected, low: bracket }) =
            selected.filter(|c| c.score < before - 1e-6 || retain_endpoint || font_rule && limit < start && c.score <= before + 1e-6)
        else {
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
            for growth in grown.iter_mut().rev() {
                charge(work, growth.scope.len().min(scope.len()).saturating_add(1))?;
                if !scope.is_disjoint(&growth.scope) {
                    growth.refit = true;
                    break;
                }
            }
        } else if limit > start && matches!(control.property.as_str(), "w" | "h") {
            let mut remembered = range.clone();
            remembered.limit = start + (limit - start) * selected;
            grown.push(Growth { range: remembered, scope: scope.clone(), refit: false, authored_limit: limit });
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
            let candidate = trial(tree, frame, data, &changes, flow, work)?;
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
        // A local cross-size fit is another valid search checkpoint. Allocation
        // can require more room, but it does not discard a smaller fitting result
        // reached from that checkpoint. Keep both bounded search paths.
        if retain_checkpoint
            && size_rule
            && control.property != flow.primary
            && limit > start
            && selected == 1.
            && bracket == 0.
            && let Some(&primary) = local_primary.get(&range.key.0)
        {
            charge(work, changes.len().saturating_mul(2))?;
            let mut local = changes.clone();
            local.insert((range.key.0, flow.primary.into()), Value(primary));
            if let Some((checkpoint, _)) = fit_growth(tree, frame, data, &range, start, limit, &mut local, flow, &scope, measure, work)? {
                let mut resumed = changes.clone();
                if let Some((value, candidate)) = fit_growth(tree, frame, data, &range, checkpoint, limit, &mut resumed, flow, &scope, measure, work)?
                    && value < best
                {
                    best = value;
                    out = candidate;
                }
            }
        }
        range.set_value(&mut changes, best);
        charge(work, grown.len())?;
        if let Some(growth) = grown.iter_mut().rev().find(|g| g.range.key == range.key) {
            growth.range.limit = best;
        }
    }
    let mut out = reclaim(tree, frame, data, &mut changes, flow, &mut grown, measure, work, out)?;
    // Orientation can trade one text dimension for the other. Such scopes retain
    // the joint fallback; a clipped parent budget alone does not reopen allocation.
    charge(work, controls.len().saturating_mul(2))?;
    let dimension_rules = controls.iter().any(|c| matches!(c.property.as_str(), "w" | "h"));
    let controls: Vec<_> = controls.iter().filter(|c| coupled.contains(&c.target)).cloned().collect();
    super::coupled::search(tree, frame, data, entries, &controls, measure, &mut changes, &mut out, work)?;
    finish(tree, frame, data, &mut changes, flow, dimension_rules && !allocated_fit, measure, work)
}

#[allow(clippy::too_many_arguments)]
fn finish<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &mut constraints::Adjustments,
    flow: &Flow,
    retain_fit: bool,
    measure: &mut Measure<'_>,
    work: &mut usize,
) -> Result<Vec<Placed<'a>>, &'static str> {
    use constraints::Adjustment::Value;
    // Final text-aware placement sees the reclaimed dimensions in rule order.
    let mut out = super::placed(tree, frame, data, changes, measure, work)?;
    // A root dimension search with unfinished text also retains a finite fit of
    // its allocated footprints. Do not repeat a capacity fit already performed
    // above, or fit again after a later font/height rule resolved the overflow.
    if flow.index == 0 && retain_fit && score(&out, &flow.children, measure, true, work)? > 1e-6 {
        let retained = allocated(tree, frame, data, changes, true, work)?;
        charge(work, retained.len().saturating_mul(2))?;
        for p in retained {
            if flow.children.contains(&p.node_key) {
                changes.insert((p.node_key, "w".into()), Value(p.rect.w / unit("w")));
                changes.insert((p.node_key, "h".into()), Value(p.rect.h / unit("h")));
            }
        }
        out = super::placed(tree, frame, data, changes, measure, work)?;
    }
    Ok(out)
}

// Probe the original endpoint and retain the smallest successful point on the
// same five-bisection interval as a size rule. Each trial recomputes text areas;
// rounded painted dimensions must not become the next search's input.
#[allow(clippy::too_many_arguments)]
fn fit_growth<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    range: &Range,
    start: f64,
    limit: f64,
    changes: &mut constraints::Adjustments,
    flow: &Flow,
    scope: &HashSet<usize>,
    measure: &mut Measure<'_>,
    work: &mut usize,
) -> Result<Option<(f64, Vec<Placed<'a>>)>, &'static str> {
    let prefer_words = range.key.1 == "w";
    range.set_value(changes, start);
    if let Some(candidate) = trial(tree, frame, data, changes, flow, work)?
        && score(&candidate, scope, measure, prefer_words, work)? <= 1e-6
    {
        return Ok(Some((start, candidate)));
    }
    if start >= limit {
        return Ok(None);
    }
    range.set_value(changes, limit);
    let Some(mut out) = trial(tree, frame, data, changes, flow, work)? else { return Ok(None) };
    if score(&out, scope, measure, prefer_words, work)? > 1e-6 {
        return Ok(None);
    }
    let (mut low, mut high, mut best) = (0., 1., limit);
    for _ in 0..5 {
        let t = (low + high) / 2.;
        let value = start + (limit - start) * t;
        range.set_value(changes, value);
        if let Some(candidate) = trial(tree, frame, data, changes, flow, work)?
            && score(&candidate, scope, measure, prefer_words, work)? <= 1e-6
        {
            high = t;
            best = value;
            out = candidate;
        } else {
            low = t;
        }
    }
    range.set_value(changes, best);
    Ok(Some((best, out)))
}

#[allow(clippy::too_many_arguments)]
fn reclaim<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    changes: &mut constraints::Adjustments,
    flow: &Flow,
    grown: &mut [Growth],
    measure: &mut Measure<'_>,
    work: &mut usize,
    mut out: Vec<Placed<'a>>,
) -> Result<Vec<Placed<'a>>, &'static str> {
    for Growth { range, scope, refit, authored_limit } in grown {
        charge(work, 1)?;
        if !*refit {
            continue;
        }
        *refit = false;
        let start = range.start;
        // Reclaim with the same metric as growth: emergency word wrapping
        // does not prevent returning height that the smaller font no longer needs.
        let prefer_words = range.key.1 == "w";
        if score(&out, scope, measure, prefer_words, work)? > 1e-6 {
            continue;
        }
        // Height reclaim repeats the authored search interval after a font change.
        // Keep the accepted endpoint separately: failed trials must not replace
        // a valid fitted height with an infeasible authored maximum.
        let limit = if prefer_words { range.limit } else { *authored_limit };
        range.set_value(changes, start);
        if let Some(candidate) = trial(tree, frame, data, changes, flow, work)?
            && score(&candidate, scope, measure, prefer_words, work)? <= 1e-6
        {
            out = candidate;
            continue;
        }
        let (mut low, mut high) = (0.0, 1.0);
        let mut best = range.limit;
        for _ in 0..5 {
            let t = (low + high) / 2.0;
            let value = start + (limit - start) * t;
            range.set_value(changes, value);
            let candidate = trial(tree, frame, data, changes, flow, work)?;
            if let Some(candidate) = candidate
                && score(&candidate, scope, measure, prefer_words, work)? <= 1e-6
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
        range.set_value(changes, best);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> crate::xml::El {
        crate::xml::parse(
            b"<dataModel><ptLst><pt modelId='root' type='doc'/><pt modelId='a'><t><p><r><t>Alpha</t></r></p></t></pt></ptLst>
              <cxnLst><cxn srcId='root' destId='a'/></cxnLst></dataModel>",
        )
        .unwrap()
        .root
    }

    #[test]
    fn local_rules_measure_the_preset_text_area_before_parent_allocation() {
        let data = data();
        let layout = crate::xml::parse(
            b"<layoutDef><layoutNode><alg type='lin'/>
              <layoutNode name='ellipse'><alg type='tx'/><presOf axis='ch'/><shape type='ellipse'/>
              <constrLst><constr type='w' val='80'/><constr type='h' val='40'/></constrLst>
              <ruleLst><rule type='w' val='100'/></ruleLst></layoutNode>
              <layoutNode><alg type='tx'/><presOf axis='ch'/><shape type='rect'/>
              <constrLst><constr type='w' val='80'/><constr type='h' val='40'/></constrLst></layoutNode>
              </layoutNode></layoutDef>",
        )
        .unwrap();
        let mm = unit("w");
        // The preset's five-decimal inset coordinates are 0.14645 and 0.85355.
        let text_fraction = 0.85355 - 0.14645;
        let mut largest_text_width: f64 = 0.;
        let mut measure = |items: &[Placed<'_>], _: super::super::MeasureKind| {
            Ok(items
                .iter()
                .map(|p| {
                    if p.name != "ellipse" {
                        return 0.;
                    }
                    let text = p.text_rect.unwrap();
                    assert!((text.w / p.rect.w - text_fraction).abs() < 1e-10);
                    largest_text_width = largest_text_width.max(text.w);
                    (65. * mm - text.w).max(0.)
                })
                .collect())
        };
        let out = super::super::super::evaluate_measured(&layout.root, &data, Xfrm::new(0., 0., 100. * mm, 60. * mm), Some(&mut measure)).unwrap();
        assert_eq!(out.len(), 2);
        assert!((largest_text_width - 100. * mm * text_fraction).abs() < 1e-8);
    }

    #[test]
    fn clipped_cross_growth_does_not_reopen_the_allocated_flow() {
        let data = data();
        let layout = |allocated: bool| {
            let mut body = "<layoutDef><layoutNode><alg type='lin'/>".to_string();
            for (seed, limit) in [(20., 100.), (30., 80.)] {
                let (width, height, font) = if allocated { (limit * 0.75, 17., 13.) } else { (seed, 20., 25.) };
                body.push_str(&format!(
                    "<layoutNode><alg type='tx'/><presOf axis='ch'/><shape type='rect'/><constrLst>
                     <constr type='w' val='{width}'/><constr type='h' val='{height}'/><constr type='primFontSz' val='{font}'/></constrLst>"
                ));
                if !allocated {
                    body.push_str(&format!(
                        "<ruleLst><rule type='w' val='{limit}'/><rule type='h' val='80'/><rule type='primFontSz' val='13'/></ruleLst>"
                    ));
                }
                body.push_str("</layoutNode>");
            }
            body.push_str("</layoutNode></layoutDef>");
            crate::xml::parse(body.as_bytes()).unwrap().root
        };
        let mm = unit("w");
        let frame = Xfrm::new(0., 0., 135. * mm, 17. * mm);
        // Positive remaining overflow cannot justify changing a sibling's already
        // allocated share when cross-axis growth only encounters the same clip.
        let mut measure = |items: &[Placed<'_>], _: super::super::MeasureKind| Ok(items.iter().map(|p| 1. + 100. * mm / p.rect.w).collect());
        let authored = layout(false);
        let fixed = layout(true);
        let actual = super::super::super::evaluate_measured(&authored, &data, frame, Some(&mut measure)).unwrap();
        let expected = super::super::super::evaluate_measured(&fixed, &data, frame, Some(&mut measure)).unwrap();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual.rect.w - expected.rect.w).abs() < 1e-8);
            assert!((actual.rect.h - expected.rect.h).abs() < 1e-8);
            assert_eq!(actual.font, Some(13.));
        }
    }

    #[test]
    fn root_dimension_rules_retain_text_fit_without_repeating_capacity_fit() {
        let data = data();
        let mm = unit("w");
        // Independent native controls distinguish a text-only fit from a flow
        // that first exceeds its frame. A no-op width rule still owns its fit.
        for (frame_width, ordinary_scale, dimension_scale) in [
            (118., 0.9912109375, 0.9902429580688477),
            (119., 0.9990234375, 0.9980478286743164),
            (120., 0.9990234375, 0.9980478286743164),
            (167., 0.9990234375, 0.9980478286743164),
        ] {
            for (rules, expected) in [("", ordinary_scale), ("<rule type='w' val='91'/>", dimension_scale)] {
                let xml = format!(
                    "<layoutDef><layoutNode><alg type='lin'/>
                     <layoutNode name='target'><alg type='tx'/><presOf axis='ch'/><shape type='rect'/>
                     <constrLst><constr type='w' val='91'/><constr type='h' val='13'/>
                     <constr type='primFontSz' val='23'/></constrLst><ruleLst>{rules}</ruleLst></layoutNode>
                     <layoutNode name='marker'><alg type='tx'/><presOf axis='ch'/><shape type='rect'/>
                     <constrLst><constr type='w' val='28'/><constr type='h' val='18'/>
                     <constr type='primFontSz' val='12'/></constrLst></layoutNode></layoutNode></layoutDef>"
                );
                let layout = crate::xml::parse(xml.as_bytes()).unwrap();
                let mut measure =
                    |items: &[Placed<'_>], _: super::super::MeasureKind| Ok(items.iter().map(|p| if p.name == "target" { 1. } else { 0. }).collect());
                let out =
                    super::super::super::evaluate_measured(&layout.root, &data, Xfrm::new(0., 0., frame_width * mm, 94. * mm), Some(&mut measure))
                        .unwrap();
                let marker = out.iter().find(|p| p.name == "marker").unwrap();
                assert!((marker.rect.w / (28. * mm) - expected).abs() < 1e-10, "frame={frame_width}, rules={rules}, marker={:?}", marker.rect);
            }
        }
    }

    #[test]
    fn successful_later_font_rule_removes_the_root_text_refit() {
        let data = data();
        let layout = crate::xml::parse(
            b"<layoutDef><layoutNode><alg type='lin'/>
              <layoutNode name='target'><alg type='tx'/><presOf axis='ch'/><shape type='rect'/>
              <constrLst><constr type='w' val='91'/><constr type='h' val='13'/>
              <constr type='primFontSz' val='23'/></constrLst>
              <ruleLst><rule type='w' val='91'/><rule type='primFontSz' val='7'/></ruleLst></layoutNode>
              <layoutNode name='marker'><alg type='tx'/><presOf axis='ch'/><shape type='rect'/>
              <constrLst><constr type='w' val='28'/><constr type='h' val='18'/></constrLst></layoutNode>
              </layoutNode></layoutDef>",
        )
        .unwrap();
        let mm = unit("w");
        let mut measure = |items: &[Placed<'_>], _: super::super::MeasureKind| {
            Ok(items.iter().map(|p| if p.name == "target" { (p.font.unwrap_or(23.) - 10.).max(0.) } else { 0. }).collect())
        };
        let out = super::super::super::evaluate_measured(&layout.root, &data, Xfrm::new(0., 0., 146. * mm, 94. * mm), Some(&mut measure)).unwrap();
        let marker = out.iter().find(|p| p.name == "marker").unwrap();
        let target = out.iter().find(|p| p.name == "target").unwrap();
        assert!((marker.rect.w - 28. * mm).abs() < 1e-8);
        assert!((target.rect.w - 91. * mm).abs() < 1e-8);
        assert!(target.font.unwrap() <= 10.);
    }

    #[test]
    fn scope_requires_independent_text_leaves_and_obeys_budget() {
        let xml = crate::xml::parse(b"<node/>").unwrap().root;
        let rule = crate::xml::parse(b"<rule type='w' val='80'/>").unwrap().root;
        let reference = crate::xml::parse(b"<constr type='w' refType='w' fact='0.5'/>").unwrap().root;
        for (shape_xml, alg, dependent, supported) in [
            ("<shape type='rect'/>", "lin", false, true),
            ("<shape type='rect' rot='30'/>", "lin", false, true),
            ("<shape type='ellipse'/>", "lin", false, true),
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

#[cfg(test)]
mod direction_tests {
    use super::super::super::evaluate_measured;
    use super::*;

    #[test]
    fn unresolved_local_dimensions_grow_without_minimizing_positive_overflow() {
        let data = crate::xml::parse(b"<dataModel><ptLst><pt modelId='root' type='doc'/></ptLst><cxnLst/></dataModel>").unwrap().root;
        let mm = unit("w");
        let frame = Xfrm::new(0., 0., 148. * mm, 92. * mm);
        for (width, height, limit, rotation, unresolved) in [
            (24., 22., 33., 17., true),
            (28., 24., 38., 37., true),
            (27., 14., 9., 25., true),
            (24., 22., 12., 17., true),
            (20., 20., 25., 0., true),
            (20., 20., 12., 0., true),
            (24., 22., 33., 17., false),
        ] {
            let layout = |rule: &str| {
                crate::xml::parse(
                    format!(
                        "<layoutDef><layoutNode name='flow'><alg type='lin'/><shape type='none'/>
                 <layoutNode name='first'><alg type='tx'/><shape type='rect' rot='{rotation}'/>
                 <constrLst><constr type='w' val='{width}'/><constr type='h' val='{height}'/></constrLst>
                 <ruleLst>{rule}</ruleLst></layoutNode>
                 <layoutNode name='marker'><alg type='tx'/><shape type='rect'/>
                 <constrLst><constr type='w' val='31'/><constr type='h' val='16'/></constrLst>
                 </layoutNode></layoutNode></layoutDef>"
                    )
                    .as_bytes(),
                )
                .unwrap()
                .root
            };
            let seed = layout("");
            let changed = layout(&format!("<rule type='w' val='{limit}'/>"));
            // A positive height deficit can worsen or improve as rotation trades
            // text width for height. Native seed/locked/rule controls distinguish
            // that score from the ordered dimension reservation.
            let mut measure = |items: &[Placed<'_>], _: super::super::MeasureKind| {
                Ok(items
                    .iter()
                    .map(|p| if unresolved && p.name == "first" { (100. * mm - p.text_rect.unwrap_or(p.rect).h).max(0.) } else { 0. })
                    .collect())
            };
            let before = evaluate_measured(&seed, &data, frame, Some(&mut measure)).unwrap();
            let after = evaluate_measured(&changed, &data, frame, Some(&mut measure)).unwrap();
            // The marker's actual width also carries the final flow-fit scale;
            // recover the reserved dimension without confusing it with that fit.
            let allocation = |items: &[Placed<'_>]| {
                let marker = items.iter().find(|p| p.name == "marker").unwrap().rect;
                2. * (marker.center().x - frame.w / 2.) / (marker.w / 31.)
            };
            let expected = if unresolved && limit > width { limit } else { width };
            assert!((allocation(&before) - width).abs() < 1e-7);
            assert!((allocation(&after) - expected).abs() < 1e-7, "{width}..{limit}, {rotation}, {unresolved}: {} != {expected}", allocation(&after));
        }
    }
}
