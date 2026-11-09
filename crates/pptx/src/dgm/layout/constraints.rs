//! Bounded sparse affine solver for diagram references. Unlike fixed-count relaxation,
//! forward chains and solvable cycles are solved together; contradictions are reported.
mod axes;
mod font_defaults;
mod local_fonts;
use super::geometry::{BoxStyle, initial, self_constraints};
use super::{Data, Instance, MAX_WORK};
use crate::xml::El;
use deckcraft_geom::Xfrm;
use std::collections::{BTreeMap, BTreeSet, HashMap};
type Key = (usize, String);
#[derive(Clone, Default)]
struct Row {
    terms: BTreeMap<Key, f64>,
    value: f64,
}
impl Row {
    fn add(&mut self, key: Key, value: f64) {
        let v = self.terms.entry(key.clone()).or_default();
        *v += value;
        if v.abs() < 1e-10 {
            self.terms.remove(&key);
        }
    }
    fn subtract(&mut self, other: &Self, factor: f64, work: &mut usize) -> Result<(), &'static str> {
        charge(work, other.terms.len() + 1)?;
        self.value -= other.value * factor;
        for (k, v) in &other.terms {
            self.add(k.clone(), -v * factor);
        }
        Ok(())
    }
}
fn charge(work: &mut usize, n: usize) -> Result<(), &'static str> {
    *work = work.saturating_add(n);
    if *work > MAX_WORK { Err("diagram constraint budget exceeded") } else { Ok(()) }
}
pub(super) struct Entry<'t, 'a> {
    pub node: &'t Instance<'a>,
    pub parent: Option<usize>,
    pub style: BoxStyle,
    pub end: usize,
    /// Local allocation scope. Linear placement reopens descendant-size defaults.
    pub geometry_scope: usize,
    /// Authored self dimensions that an ancestor may replace as defaults, without
    /// making them a parent allocation.
    pub self_sizes: [bool; 2],
}
pub(super) type Solution = HashMap<usize, BoxStyle>;
pub(super) fn id(node: &Instance<'_>) -> usize {
    std::ptr::from_ref(node) as usize
}
pub(super) fn lookup(solution: &Solution, node: &Instance<'_>) -> Option<BoxStyle> {
    solution.get(&id(node)).copied()
}
pub(super) fn position(property: &str) -> bool {
    matches!(property, "l" | "r" | "ctrX" | "t" | "b" | "ctrY")
}
pub(super) fn applies(entries: &[Entry<'_, '_>], owner: usize, target: usize, property: &str, selector: &El) -> bool {
    // A text node controls its text, not its position in the parent. Flow algorithms
    // determine child positions themselves; only a composite consumes explicit coordinates.
    if position(property) {
        return selector.attr("for") == Some("ch") && entries[target].parent == Some(owner) && entries[owner].node.alg == "composite";
    }
    // Composite and space algorithms establish local size scopes. Flow algorithms
    // can consume ancestor descendant-size constraints. Font constraints still
    // reach text across both kinds of container.
    if !matches!(property, "w" | "h" | "diam") {
        return true;
    }
    let entry = &entries[target];
    if owner == target && entry.parent.is_some_and(|p| entries[p].node.alg == "sp") {
        return false;
    }
    let self_default = entries[entry.geometry_scope].node.alg == "composite"
        && match property {
            "w" => entry.self_sizes[0],
            "h" => entry.self_sizes[1],
            _ => entry.self_sizes.iter().all(|v| *v),
        };
    (owner >= entry.geometry_scope || self_default) && (entries[owner].node.alg != "composite" || selector.attr("ptType").is_none_or(|t| t == "all"))
}
pub(super) fn collect<'t, 'a>(
    node: &'t Instance<'a>,
    mut style: BoxStyle,
    parent: Option<usize>,
    entries: &mut Vec<Entry<'t, 'a>>,
    work: &mut usize,
) -> Result<(), &'static str> {
    charge(work, node.constraints.len().saturating_mul(3).saturating_add(1))?;
    let self_sizing = parent.is_none_or(|p| entries[p].node.alg != "sp");
    self_constraints(node, &mut style, self_sizing);
    let index = entries.len();
    let geometry_scope = parent.map_or(0, |p| match entries[p].node.alg.as_str() {
        "composite" | "sp" => p,
        "lin" | "hierChild" => 0,
        _ => entries[p].geometry_scope,
    });
    let self_sizes = ["w", "h"].map(|axis| {
        self_sizing
            && node
                .constraints
                .iter()
                .any(|c| c.attr("for").unwrap_or("self") == "self" && c.attr("type").is_some_and(|t| t == axis || t == "diam") && !size_bound(c))
    });
    entries.push(Entry { node, parent, style, end: index + 1, geometry_scope, self_sizes });
    let local = BoxStyle { rect: Xfrm::new(0., 0., style.rect.w, style.rect.h), ..style };
    for (child, rect) in node.children.iter().zip(initial(node, local)) {
        // This node's typography is not a descendant default. Child/descendant
        // selectors and explicit references establish links between text nodes.
        collect(
            child,
            BoxStyle { rect, font: BoxStyle::new(rect).font, secondary_font: None, margins: [None; 4], ..local },
            Some(index),
            entries,
            work,
        )?;
    }
    let end = entries.len();
    if let Some(entry) = entries.get_mut(index) {
        entry.end = end;
    }
    Ok(())
}
pub(super) fn selected(entries: &[Entry<'_, '_>], owner: usize, axis: &str, name: Option<&str>, point: Option<&str>, data: &Data<'_>) -> Vec<usize> {
    let Some(root) = entries.get(owner) else { return vec![] };
    (owner..root.end)
        .filter(|&i| {
            let e = &entries[i];
            (match axis {
                "ch" => e.parent == Some(owner),
                "des" => i != owner,
                _ => i == owner,
            }) && name.is_none_or(|n| n.is_empty() || n == e.node.name)
                && point.is_none_or(|t| t == "all" || e.node.points.iter().any(|i| data.matches(*i, t)))
        })
        .collect()
}
fn expr(entries: &[Entry<'_, '_>], node: usize, owner: usize, property: &str) -> Row {
    let mut row = Row::default();
    let (start, size, factor) = match property {
        "l" => ("l", "w", 0.),
        "r" => ("l", "w", 1.),
        "ctrX" => ("l", "w", 0.5),
        "t" => ("t", "h", 0.),
        "b" => ("t", "h", 1.),
        "ctrY" => ("t", "h", 0.5),
        _ => {
            row.add((node, property.into()), 1.);
            return row;
        }
    };
    // Keep edge values separate from display extents: inverted edges retain their
    // authored endpoints, while references to width/height see positive sizes.
    if node == owner {
        row.add((node, size.into()), factor);
    } else {
        row.add((node, property.into()), 1.);
        let mut parent = entries.get(node).and_then(|e| e.parent);
        for _ in 0..64 {
            let Some(p) = parent.filter(|p| *p != owner) else { break };
            row.add((p, start.into()), 1.);
            parent = entries.get(p).and_then(|e| e.parent);
        }
    }
    row
}

fn equation(
    entries: &[Entry<'_, '_>],
    owner: usize,
    target: usize,
    reference: Option<usize>,
    c: &El,
    property: &str,
    work: &mut usize,
) -> Result<Row, &'static str> {
    let target_property =
        if matches!(property, "w" | "h") && !matches!(c.attr("op"), Some("gte" | "lte")) { axes::signed(property) } else { property };
    let mut row = expr(entries, target, if target == owner { entries[owner].parent.unwrap_or(owner) } else { owner }, target_property);
    let scale = if matches!(property, "primFontSz" | "secFontSz" | "pyraAcctRatio") || property.starts_with("user") { 1. } else { 72. / 25.4 };
    row.value = c.f64("val").unwrap_or(0.) * scale;
    if let Some(ref_type) = c.attr("refType").filter(|s| !matches!(*s, "" | "none")) {
        // ECMA-376: val is an alternative to a reference, never an additive offset.
        // Native PowerPoint gives an explicit refType precedence if both are present.
        row.value = 0.;
        let reference = reference.ok_or("missing diagram constraint reference")?;
        let ref_type =
            if ref_type == "diam" { if entries[reference].style.rect.w < entries[reference].style.rect.h { "w" } else { "h" } } else { ref_type };
        row.subtract(&expr(entries, reference, owner, ref_type), c.f64("fact").unwrap_or(1.), work)?;
    } else if matches!(property, "primFontSz" | "secFontSz") {
        // Authored font defaults are whole-point sizes before reference defaults
        // are evaluated. Local fitted values have their own unrounded propagation.
        row.value = row.value.round();
    }
    if !row.value.is_finite() || row.terms.values().any(|v| !v.is_finite() || v.abs() > 1e6) {
        return Err("invalid diagram constraint coefficient");
    }
    Ok(row)
}
fn basis(rows: &[Row], work: &mut usize) -> Result<BTreeMap<Key, Row>, &'static str> {
    let mut basis: BTreeMap<Key, Row> = BTreeMap::new();
    for source in rows {
        let mut row = source.clone();
        while let Some((key, &coefficient)) = row.terms.first_key_value() {
            let key = key.clone();
            if let Some(pivot) = basis.get(&key) {
                row.subtract(pivot, coefficient, work)?;
            } else {
                row.value /= coefficient;
                for value in row.terms.values_mut() {
                    *value /= coefficient;
                }
                basis.insert(key, row.clone());
                break;
            }
        }
        if row.terms.is_empty() && row.value.abs() > 1e-6 {
            return Err("inconsistent diagram constraints");
        }
    }
    Ok(basis)
}
fn solve(rows: &[Row], entries: &[Entry<'_, '_>], work: &mut usize) -> Result<BTreeMap<Key, f64>, &'static str> {
    let basis = basis(rows, work)?;
    let mut values = BTreeMap::new();
    for (key, row) in basis.iter().rev() {
        let mut value = row.value;
        for (k, c) in row.terms.iter().filter(|(k, _)| *k != key) {
            charge(work, 1)?;
            let default = entries.get(k.0).and_then(|e| e.style.get(&k.1)).unwrap_or(0.);
            value -= c * values.get(k).copied().unwrap_or(default);
        }
        if !value.is_finite() || value.abs() > 1e6 {
            return Err("unbounded diagram constraint solution");
        }
        values.insert(key.clone(), value);
    }
    Ok(values)
}
pub(super) fn resolve(node: &Instance<'_>, rect: Xfrm, data: &Data<'_>, work: &mut usize) -> Result<Solution, &'static str> {
    resolve_adjusted(node, rect, data, work, &HashMap::new())
}

/// Parent rules change constraint values; local font rules change the displayed font while
/// retaining the earlier constraint value for default (non-equality) references.
#[derive(Clone, Copy)]
pub(super) enum Adjustment {
    Value(f64),
    Factor(f64),
    LocalFont { value: f64, base: Option<ConstraintValue> },
}
#[derive(Clone, Copy)]
pub(super) enum ConstraintValue {
    Value(f64),
    Factor(f64),
}
impl Adjustment {
    pub(super) fn constraint(self) -> Option<ConstraintValue> {
        match self {
            Self::Value(v) => Some(ConstraintValue::Value(v)),
            Self::Factor(v) => Some(ConstraintValue::Factor(v)),
            Self::LocalFont { base, .. } => base,
        }
    }
}
pub(super) type Adjustments = HashMap<(usize, String), Adjustment>;

pub(super) fn font_bound(c: &El) -> bool {
    matches!(c.attr("type"), Some("primFontSz" | "secFontSz")) && matches!(c.attr("op"), Some("gte" | "lte"))
}

/// Size inequalities do not allocate geometry or cap native size-rule trials.
/// Keep general affine bounds for other numeric properties separate from them.
pub(super) fn size_bound(c: &El) -> bool {
    matches!(c.attr("type"), Some("w" | "h" | "diam")) && matches!(c.attr("op"), Some("gte" | "lte"))
}

pub(super) fn unnamed_child_geometry_default(c: &El, property: &str) -> bool {
    (position(property) || matches!(property, "w" | "h")) && c.attr("for") == Some("ch") && c.attr("forName").is_none_or(str::is_empty)
}

pub(super) fn resolve_adjusted(
    node: &Instance<'_>,
    rect: Xfrm,
    data: &Data<'_>,
    work: &mut usize,
    adjustments: &Adjustments,
) -> Result<Solution, &'static str> {
    let mut entries = Vec::new();
    collect(node, BoxStyle::new(rect), None, &mut entries, work)?;
    let mut equations: BTreeMap<Key, Row> = BTreeMap::new();
    let mut equation_owners = HashMap::new();
    let mut size_fallbacks = BTreeSet::new();
    let mut bounds = Vec::new();
    let mut bounded = BTreeSet::new();
    // Inner defaults first. A local child allocation beats ancestor defaults, but
    // a node's own size is a fallback to its parent's allocation. Text constraints
    // can still coordinate the same property across descendants.
    for owner in (0..entries.len()).rev() {
        let node = entries[owner].node;
        for c in &node.constraints {
            // Native font bounds participate in local fitting, not initial font selection.
            // Unconstrained nodes retain the diagram default until a rule is applied.
            if font_bound(c) || size_bound(c) {
                continue;
            }
            charge(work, entries[owner].end - owner)?;
            let targets = selected(&entries, owner, c.attr("for").unwrap_or("self"), c.attr("forName"), c.attr("ptType"), data);
            let refs = selected(&entries, owner, c.attr("refFor").unwrap_or("self"), c.attr("refForName"), c.attr("refPtType"), data);
            let ty = c.attr("type").unwrap_or("");
            for (j, target) in targets.iter().enumerate() {
                let reference = if refs.len() == targets.len() { refs.get(j) } else { refs.first() }.copied();
                for property in if ty == "diam" { vec!["w", "h"] } else { vec![ty] } {
                    if !applies(&entries, owner, *target, property, c) {
                        continue;
                    }
                    let key = (*target, property.to_string());
                    // A later unnamed declaration does not replace dimensions or
                    // positions already assigned at this layout node.
                    if unnamed_child_geometry_default(c, property) && equation_owners.get(&key) == Some(&owner) {
                        continue;
                    }
                    if matches!(property, "w" | "h") && equation_owners.get(&key).is_some_and(|prior| *prior > owner && *prior != *target) {
                        continue;
                    }
                    let mut adjusted = (*c).clone();
                    if let Some(change) = adjustments.get(&(id(entries[*target].node), property.into())).and_then(|a| a.constraint()) {
                        match change {
                            ConstraintValue::Value(value) => {
                                adjusted.set_attr("refType", "none");
                                adjusted.set_attr("val", &value.to_string());
                            }
                            ConstraintValue::Factor(factor) => adjusted.set_attr("fact", &factor.to_string()),
                        }
                    }
                    let row = equation(&entries, owner, *target, reference, &adjusted, property, work)?;
                    if matches!(c.attr("op"), Some("gte" | "lte")) {
                        bounded.insert((*target, property.to_string()));
                    }
                    match c.attr("op").unwrap_or("equ") {
                        "gte" => bounds.push(row),
                        "lte" => {
                            let mut row = row;
                            row.value = -row.value;
                            for v in row.terms.values_mut() {
                                *v = -*v;
                            }
                            bounds.push(row);
                        }
                        _ => {
                            if matches!(property, "w" | "h") {
                                if owner == *target || owner < entries[*target].geometry_scope {
                                    size_fallbacks.insert(key.clone());
                                } else {
                                    size_fallbacks.remove(&key);
                                }
                            }
                            equation_owners.insert(key.clone(), owner);
                            equations.insert(key, row);
                        }
                    }
                }
            }
        }
    }
    // Rules may supply values for otherwise implicit properties. Insert them before
    // selecting axis equations so a changed redundant edge cannot re-enter the system.
    for (i, entry) in entries.iter().enumerate() {
        charge(work, adjustments.len())?;
        for ((node, property), change) in adjustments {
            if *node == id(entry.node)
                && !equations.contains_key(&(i, property.clone()))
                && let Some(ConstraintValue::Value(value)) = change.constraint()
            {
                let mut c = El::default();
                c.set_attr("val", &value.to_string());
                let row = equation(&entries, i, i, None, &c, property, work)?;
                if matches!(property.as_str(), "w" | "h") {
                    size_fallbacks.insert((i, property.clone()));
                }
                equations.insert((i, property.clone()), row);
            }
        }
    }
    let extents = axes::complete(&mut equations, &size_fallbacks, &entries, &bounded, work)?;
    let rows: Vec<_> = equations.values().cloned().collect();
    let values = axes::solve_extents(&rows, &bounds, &extents, &entries, work)?;
    for ((index, property), value) in &values {
        // Edge/centre variables retain constraint-space values for references.
        // Only starts and positive extents form the displayed rectangle.
        if matches!(property.as_str(), "r" | "b" | "ctrX" | "ctrY") {
            continue;
        }
        if let Some(entry) = entries.get_mut(*index) {
            entry.style.set(property, *value, "equ");
        }
    }
    font_defaults::apply(&mut entries, &equations, &values, work)?;
    local_fonts::apply(&mut entries, data, adjustments, work)?;
    Ok(entries.into_iter().map(|e| (id(e.node), e.style)).collect())
}

fn select_axes(equations: &mut BTreeMap<Key, Row>, count: usize, work: &mut usize) -> Result<(), &'static str> {
    // An axis has two degrees of freedom. Native composite layout prefers explicit
    // size, then start, end, centre; redundant coordinates do not create extra equations.
    // This priority is independent of XML order. Retain inequality bounds separately.
    for i in 0..count {
        charge(work, 8)?;
        for properties in [["w", "l", "r", "ctrX"], ["h", "t", "b", "ctrY"]] {
            let mut count = 0;
            for property in properties {
                let key = (i, property.to_string());
                if equations.contains_key(&key) {
                    count += 1;
                    if count > 2 {
                        equations.remove(&key);
                    }
                }
            }
        }
    }
    Ok(())
}

fn apply_bounds(rows: &mut Vec<Row>, bounds: Vec<Row>, entries: &[Entry<'_, '_>], work: &mut usize) -> Result<(), &'static str> {
    if !bounds.is_empty() {
        let basis = basis(rows, work)?;
        // Express each half-space in the free variables of the exact equality solution.
        // Dykstra projection keeps previously visited bounds as corrections, so tightening
        // or intersecting bounds cannot turn a feasible set into a false contradiction.
        let mut reduced = Vec::new();
        let mut free = BTreeMap::new();
        for bound in bounds {
            let mut row = bound;
            for (key, pivot) in &basis {
                if let Some(c) = row.terms.get(key).copied() {
                    row.subtract(pivot, c, work)?;
                }
            }
            let norm: f64 = row.terms.values().map(|v| v * v).sum();
            if norm < 1e-14 {
                if row.value > 1e-6 {
                    return Err("inconsistent diagram bounds");
                }
                continue;
            }
            for key in row.terms.keys() {
                free.entry(key.clone()).or_insert_with(|| entries[key.0].style.get(&key.1).unwrap_or(0.));
            }
            reduced.push((row, norm, 0.));
        }
        let mut converged = false;
        for _ in 0..4096 {
            for (row, norm, correction) in &mut reduced {
                charge(work, row.terms.len() * 3 + 1)?;
                for (key, c) in &row.terms {
                    if let Some(v) = free.get_mut(key) {
                        *v += *correction * c;
                    }
                }
                let dot: f64 = row.terms.iter().map(|(k, c)| c * free.get(k).copied().unwrap_or(0.)).sum();
                let step = ((row.value - dot) / *norm).max(0.);
                for (key, c) in &row.terms {
                    if let Some(v) = free.get_mut(key) {
                        *v += step * c;
                    }
                }
                *correction = -step;
            }
            if reduced.iter().all(|(r, _, _)| r.terms.iter().map(|(k, c)| c * free.get(k).copied().unwrap_or(0.)).sum::<f64>() + 1e-6 >= r.value) {
                converged = true;
                break;
            }
        }
        if !converged {
            return Err("diagram bounds did not converge");
        }
        for (key, value) in free {
            let mut row = Row { value, ..Default::default() };
            row.add(key, 1.);
            rows.push(row);
        }
    }
    Ok(())
}
