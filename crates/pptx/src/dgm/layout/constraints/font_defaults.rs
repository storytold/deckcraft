//! Authored font defaults quantize at each reference, before local fitting takes over.
use super::{Entry, Key, Row, charge};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

fn font(key: &Key) -> bool {
    matches!(key.1.as_str(), "primFontSz" | "secFontSz")
}

pub(super) fn apply(entries: &mut [Entry<'_, '_>], equations: &BTreeMap<Key, Row>, work: &mut usize) -> Result<(), &'static str> {
    charge(work, entries.len() + equations.len())?;
    for entry in entries.iter_mut() {
        entry.style.font = entry.style.font.map(f64::round);
        entry.style.secondary_font = entry.style.secondary_font.map(f64::round);
    }
    let keys: BTreeSet<_> = equations.keys().filter(|k| font(k)).cloned().collect();
    let mut waiting = HashMap::new();
    let mut dependents = HashMap::<Key, Vec<Key>>::new();
    let mut ready = VecDeque::new();
    for key in &keys {
        let Some(row) = equations.get(key) else { continue };
        charge(work, row.terms.len())?;
        let mut count = 0usize;
        for dependency in row.terms.keys().filter(|other| *other != key && keys.contains(*other)) {
            dependents.entry(dependency.clone()).or_default().push(key.clone());
            count += 1;
        }
        waiting.insert(key.clone(), count);
        if count == 0 {
            ready.push_back(key.clone());
        }
    }
    while let Some(key) = ready.pop_front() {
        let Some(row) = equations.get(&key) else { continue };
        charge(work, row.terms.len() + 1)?;
        if let Some(&coefficient) = row.terms.get(&key).filter(|c| c.abs() > 1e-10) {
            let mut value = row.value;
            for (other, c) in &row.terms {
                if other == &key {
                    continue;
                }
                let other_value = entries.get(other.0).and_then(|e| e.style.get(&other.1)).unwrap_or(0.);
                value -= c * if font(other) { other_value.round() } else { other_value };
            }
            if let Some(entry) = entries.get_mut(key.0) {
                entry.style.set(&key.1, (value / coefficient).round(), "equ");
            }
        }
        for dependent in dependents.get(&key).into_iter().flatten() {
            charge(work, 1)?;
            if let Some(count) = waiting.get_mut(dependent) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    ready.push_back(dependent.clone());
                }
            }
        }
    }
    // Cyclic components retain the rounded affine solution. Do not iterate rounding
    // around a cycle: it can drift or oscillate without an authored fixed point.
    Ok(())
}
