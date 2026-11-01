//! Local font fitting follows explicit font equalities/bounds, not ordinary defaults.
use super::{Adjustment, Adjustments, Data, Entry, Key, Row, apply_bounds, charge, equation, id, selected, solve};
use std::collections::{HashMap, HashSet};

fn activate(relations: &[(Row, bool)], fixed: &HashSet<Key>, work: &mut usize) -> Result<(Vec<Row>, Vec<Row>), &'static str> {
    let mut links = HashMap::<Key, Vec<usize>>::new();
    for (i, (row, _)) in relations.iter().enumerate() {
        charge(work, row.terms.len())?;
        for key in row.terms.keys() {
            links.entry(key.clone()).or_default().push(i);
        }
    }
    let mut seen = vec![false; relations.len()];
    let mut affected = fixed.clone();
    let mut pending: Vec<_> = fixed.iter().cloned().collect();
    while let Some(key) = pending.pop() {
        for &i in links.get(&key).into_iter().flatten() {
            charge(work, 1)?;
            if seen[i] {
                continue;
            }
            seen[i] = true;
            let (row, _) = &relations[i];
            charge(work, row.terms.len())?;
            for key in row.terms.keys() {
                if affected.insert(key.clone()) {
                    pending.push(key.clone());
                }
            }
        }
    }
    // Reachability is unordered; equation/bound evaluation retains authored order.
    let (mut rows, mut bounds) = (Vec::new(), Vec::new());
    for ((row, bound), active) in relations.iter().zip(seen) {
        if active {
            if *bound { &mut bounds } else { &mut rows }.push(row.clone());
        }
    }
    Ok((rows, bounds))
}

pub(super) fn apply(entries: &mut [Entry<'_, '_>], data: &Data<'_>, changes: &Adjustments, work: &mut usize) -> Result<(), &'static str> {
    let mut rows = Vec::new();
    let mut fixed = HashSet::new();
    for (i, entry) in entries.iter().enumerate() {
        for property in ["primFontSz", "secFontSz"] {
            if let Some(Adjustment::LocalFont { value, .. }) = changes.get(&(id(entry.node), property.into())) {
                charge(work, 1)?;
                let mut row = Row { value: *value, ..Default::default() };
                row.add((i, property.into()), 1.);
                rows.push(row);
                fixed.insert((i, property.to_string()));
            }
        }
    }
    if rows.is_empty() {
        return Ok(());
    }
    let mut relations = Vec::new();
    for (owner, entry) in entries.iter().enumerate() {
        for c in &entry.node.constraints {
            let property = c.attr("type").unwrap_or("");
            if !matches!(property, "primFontSz" | "secFontSz")
                || !matches!(c.attr("refType"), Some("primFontSz" | "secFontSz"))
                || !matches!(c.attr("op"), Some("equ" | "gte" | "lte"))
            {
                continue;
            }
            charge(work, entry.end - owner)?;
            let targets = selected(entries, owner, c.attr("for").unwrap_or("self"), c.attr("forName"), c.attr("ptType"), data);
            let refs = selected(entries, owner, c.attr("refFor").unwrap_or("self"), c.attr("refForName"), c.attr("refPtType"), data);
            for (i, &target) in targets.iter().enumerate() {
                let reference = if refs.len() == targets.len() { refs.get(i) } else { refs.first() }.copied();
                let mut row = equation(entries, owner, target, reference, c, property, work)?;
                if c.attr("op") == Some("equ") {
                    relations.push((row, false));
                } else {
                    if c.attr("op") == Some("lte") {
                        row.value = -row.value;
                        for v in row.terms.values_mut() {
                            *v = -*v;
                        }
                    }
                    relations.push((row, true));
                }
            }
        }
    }
    let (equalities, bounds) = activate(&relations, &fixed, work)?;
    rows.extend(equalities);
    apply_bounds(&mut rows, bounds, entries, work)?;
    for ((index, property), value) in solve(&rows, entries, work)? {
        if let Some(entry) = entries.get_mut(index) {
            let value = if fixed.contains(&(index, property.clone())) { value } else { value.round() };
            entry.style.set(&property, value, "equ");
        }
    }
    Ok(())
}
