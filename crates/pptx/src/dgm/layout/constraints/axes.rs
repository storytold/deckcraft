//! Constraint-space endpoints and nonnegative display extents are distinct.
use super::{Entry, Key, Row, apply_bounds, charge, solve};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn signed(size: &str) -> &str {
    match size {
        "w" => "_signed_w",
        "h" => "_signed_h",
        _ => size,
    }
}
fn default(entries: &[Entry<'_, '_>], i: usize, property: &str, size: &str) -> Row {
    let entry = &entries[i];
    let value = entry.style.get(property).unwrap_or(0.);
    let mut row = Row::default();
    row.add((i, property.into()), 1.);
    if let Some(parent) = entry.parent {
        if property == size && matches!(entries[parent].node.alg.as_str(), "lin" | "cycle") {
            // Flow nodes start with intrinsic dimensions, independent of the
            // allocated container. Packing applies the eventual fit afterwards.
            row.value = value;
            return row;
        }
        let parent_value = entries[parent].style.get(size).unwrap_or(0.);
        let ratio = if parent_value.abs() > 1e-9 { value / parent_value } else { 0. };
        row.add((parent, size.into()), -ratio);
    } else {
        row.value = value;
    }
    row
}

pub(super) fn complete(
    equations: &mut BTreeMap<Key, Row>,
    size_fallbacks: &BTreeSet<Key>,
    entries: &[Entry<'_, '_>],
    bounded: &BTreeSet<Key>,
    work: &mut usize,
) -> Result<Vec<(Key, Key)>, &'static str> {
    // A node's own dimension is a fallback for its parent's allocation. Two
    // parent coordinates already determine that allocation. A sole end/centre
    // cannot position a local fallback size; native layout retains the authored
    // reference but collapses the other coordinates and displayed extent.
    let mut collapsed = BTreeSet::new();
    for (i, entry) in entries.iter().enumerate() {
        charge(work, 8)?;
        for (start, end, center, size) in [("l", "r", "ctrX", "w"), ("t", "b", "ctrY", "h")] {
            let key = (i, size.to_string());
            if !size_fallbacks.contains(&key) || entry.parent.is_none() {
                continue;
            }
            let coords = [start, end, center].map(|p| equations.contains_key(&(i, p.into())));
            let count = coords.into_iter().filter(|v| *v).count();
            if count >= 2 || (count == 1 && !coords[0]) {
                equations.remove(&key);
                if count == 1 {
                    collapsed.insert(key);
                }
            }
        }
    }
    super::select_axes(equations, entries.len(), work)?;
    let mut extents = Vec::new();
    for i in 0..entries.len() {
        charge(work, 16)?;
        for (start, end, center, size) in [("l", "r", "ctrX", "w"), ("t", "b", "ctrY", "h")] {
            if collapsed.contains(&(i, size.into())) {
                for property in [start, end, center, size] {
                    equations.entry((i, property.into())).or_insert_with(|| {
                        let mut row = Row::default();
                        row.add((i, property.into()), 1.);
                        row
                    });
                }
                continue;
            }
            let has = |p: &str| equations.contains_key(&(i, p.into()));
            let explicit_size = has(size);
            let bounded_size = bounded.contains(&(i, size.into()));
            let coords = [has(start), has(end), has(center)];
            let count = coords.into_iter().filter(|v| *v).count();
            let implicit = !explicit_size && !bounded_size && count > 0;
            if explicit_size || (implicit && coords != [false, false, true]) {
                extents.push(((i, size.into()), (i, signed(size).into())));
            }
            let lone_center = implicit && coords == [false, false, true];
            if lone_center {
                // A centre without a size does not allocate geometry, but its
                // authored value is still available to another node's reference.
                let mut row = Row::default();
                row.add((i, size.into()), 1.);
                equations.insert((i, size.into()), row);
                equations.insert((i, start.into()), default(entries, i, start, size));
            } else if implicit && count == 1 {
                let other = if coords[0] { end } else { start };
                equations.insert((i, other.into()), default(entries, i, other, size));
            } else if !implicit {
                if !explicit_size && !bounded_size {
                    equations.insert((i, size.into()), default(entries, i, size, size));
                }
                if count == 0 && !bounded.contains(&(i, start.into())) {
                    equations.insert((i, start.into()), default(entries, i, start, size));
                }
            }
            let span = if implicit && !lone_center { signed(size) } else { size };
            // These identities complete the unused coordinates. For implicit sizes
            // span may be negative; display size is its absolute value, without a flip.
            let mut edge = Row::default();
            edge.add((i, end.into()), 1.);
            edge.add((i, start.into()), -1.);
            edge.add((i, span.into()), -1.);
            equations.insert((i, format!("_edge_{end}")), edge);
            if !lone_center {
                let mut mid = Row::default();
                mid.add((i, center.into()), 1.);
                mid.add((i, start.into()), -1.);
                mid.add((i, span.into()), -0.5);
                equations.insert((i, format!("_edge_{center}")), mid);
            }
        }
    }
    Ok(extents)
}

pub(super) fn solve_extents(
    rows: &[Row],
    bounds: &[Row],
    extents: &[(Key, Key)],
    entries: &[Entry<'_, '_>],
    work: &mut usize,
) -> Result<BTreeMap<Key, f64>, &'static str> {
    let mut signs = vec![1_i8; extents.len()];
    let mut seen = BTreeSet::new();
    loop {
        charge(work, rows.len() + extents.len())?;
        if !seen.insert(signs.clone()) {
            return Err("inconsistent diagram extent signs");
        }
        let mut trial = rows.to_vec();
        for ((size, raw), sign) in extents.iter().zip(&signs) {
            let mut row = Row::default();
            row.add(size.clone(), 1.);
            row.add(raw.clone(), -f64::from(*sign));
            trial.push(row);
        }
        // Resolve sign choices before bounds: a negative raw extent can have a
        // valid positive bounded display size. Never silently clamp an endpoint.
        let values = solve(&trial, entries, work)?;
        let choose = |values: &BTreeMap<Key, f64>, signs: &mut [i8]| {
            let mut changed = false;
            for ((_, raw), sign) in extents.iter().zip(signs) {
                let value = values.get(raw).copied().unwrap_or(0.);
                if value.abs() > 1e-6 {
                    let wanted = if value < 0. { -1 } else { 1 };
                    changed |= *sign != wanted;
                    *sign = wanted;
                }
            }
            changed
        };
        if choose(&values, &mut signs) {
            continue;
        }
        let values = if bounds.is_empty() {
            values
        } else {
            apply_bounds(&mut trial, bounds.to_vec(), entries, work)?;
            solve(&trial, entries, work)?
        };
        if choose(&values, &mut signs) {
            continue;
        }
        if values.iter().any(|((_, p), v)| matches!(p.as_str(), "w" | "h") && *v < -1e-6) {
            return Err("negative diagram extent");
        }
        return Ok(values);
    }
}
