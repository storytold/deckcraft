//! Bounded sparse affine solver for diagram references. Unlike fixed-count relaxation,
//! forward chains and solvable cycles are solved together; contradictions are reported.
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
}
pub(super) type Solution = HashMap<usize, BoxStyle>;
pub(super) fn id(node: &Instance<'_>) -> usize {
    std::ptr::from_ref(node) as usize
}
pub(super) fn lookup(solution: &Solution, node: &Instance<'_>) -> Option<BoxStyle> {
    solution.get(&id(node)).copied()
}
pub(super) fn collect<'t, 'a>(
    node: &'t Instance<'a>,
    mut style: BoxStyle,
    parent: Option<usize>,
    entries: &mut Vec<Entry<'t, 'a>>,
    work: &mut usize,
) -> Result<(), &'static str> {
    charge(work, 1 + node.constraints.len())?;
    self_constraints(node, &mut style);
    let index = entries.len();
    entries.push(Entry { node, parent, style, end: index + 1 });
    let local = BoxStyle { rect: Xfrm::new(0., 0., style.rect.w, style.rect.h), ..style };
    for (child, rect) in node.children.iter().zip(initial(node, local)) {
        collect(child, BoxStyle { rect, ..local }, Some(index), entries, work)?;
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
    let (position, extent, factor) = match property {
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
    row.add((node, position.into()), 1.);
    row.add((node, extent.into()), factor);
    // Named descendant coordinates are relative to the owning layout, not their immediate
    // container. The owner's own box is local (0,0,w,h).
    if node == owner {
        row.terms.remove(&(node, position.into()));
    } else {
        let mut parent = entries.get(node).and_then(|e| e.parent);
        for _ in 0..64 {
            let Some(p) = parent.filter(|p| *p != owner) else { break };
            row.add((p, position.into()), 1.);
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
    let mut row = expr(entries, target, if target == owner { entries[owner].parent.unwrap_or(owner) } else { owner }, property);
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

/// A rule changes a constraint's absolute value or reference multiplier; it does not sever
/// dependent equations. Node identity distinguishes repeated instances with the same name.
#[derive(Clone, Copy)]
pub(super) enum Adjustment {
    Value(f64),
    Factor(f64),
}
pub(super) type Adjustments = HashMap<(usize, String), Adjustment>;

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
    let mut bounds = Vec::new();
    let mut bounded = BTreeSet::new();
    // Inner defaults first; outer explicit constraints take precedence for the same property.
    for owner in (0..entries.len()).rev() {
        let node = entries[owner].node;
        for c in &node.constraints {
            charge(work, entries[owner].end - owner)?;
            let targets = selected(&entries, owner, c.attr("for").unwrap_or("self"), c.attr("forName"), c.attr("ptType"), data);
            let refs = selected(&entries, owner, c.attr("refFor").unwrap_or("self"), c.attr("refForName"), c.attr("refPtType"), data);
            let ty = c.attr("type").unwrap_or("");
            for (j, target) in targets.iter().enumerate() {
                let reference = if refs.len() == targets.len() { refs.get(j) } else { refs.first() }.copied();
                for property in if ty == "diam" { vec!["w", "h"] } else { vec![ty] } {
                    let mut adjusted = (*c).clone();
                    if let Some(change) = adjustments.get(&(id(entries[*target].node), property.into())) {
                        match change {
                            Adjustment::Value(value) => {
                                adjusted.set_attr("refType", "none");
                                adjusted.set_attr("val", &value.to_string());
                            }
                            Adjustment::Factor(factor) => adjusted.set_attr("fact", &factor.to_string()),
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
                            equations.insert((*target, property.into()), row);
                        }
                    }
                }
            }
        }
    }
    // Algorithm geometry supplies only unconstrained degrees of freedom. Relationships
    // remain affine so children follow a resized container, rather than retaining its old size.
    for (i, entry) in entries.iter().enumerate() {
        charge(work, adjustments.len())?;
        for ((node, property), change) in adjustments {
            if *node == id(entry.node)
                && !equations.contains_key(&(i, property.clone()))
                && let Adjustment::Value(value) = change
            {
                let mut c = El::default();
                c.set_attr("val", &value.to_string());
                let row = equation(&entries, i, i, None, &c, property, work)?;
                equations.insert((i, property.clone()), row);
            }
        }
        for (position, size, coords) in [("l", "w", ["l", "r", "ctrX"]), ("t", "h", ["t", "b", "ctrY"])] {
            let count = coords.iter().filter(|p| equations.contains_key(&(i, p.to_string()))).count();
            for property in [position, size] {
                if bounded.contains(&(i, property.into()))
                    || equations.contains_key(&(i, property.into()))
                    || (property == size && count >= 2)
                    || (property == position && count >= 1)
                {
                    continue;
                }
                let value = entry.style.get(property).unwrap_or(0.);
                let mut row = Row::default();
                row.add((i, property.into()), 1.);
                if let Some(parent) = entry.parent {
                    let parent_value = entries[parent].style.get(size).unwrap_or(0.);
                    let ratio = if parent_value.abs() > 1e-9 { value / parent_value } else { 0. };
                    row.add((parent, size.into()), -ratio);
                } else {
                    row.value = value;
                }
                equations.insert((i, property.into()), row);
            }
        }
    }
    let mut rows: Vec<_> = equations.into_values().collect();
    apply_bounds(&mut rows, bounds, &entries, work)?;
    let values = solve(&rows, &entries, work)?;
    for ((index, property), value) in values {
        if matches!(property.as_str(), "w" | "h") && value < -1e-6 {
            return Err("negative diagram extent");
        }
        if let Some(entry) = entries.get_mut(index) {
            entry.style.set(&property, value, "equ");
        }
    }
    Ok(entries.into_iter().map(|e| (id(e.node), e.style)).collect())
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
