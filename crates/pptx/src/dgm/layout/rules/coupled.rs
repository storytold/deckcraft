//! Joint fallback for rules whose independent moves make no text-fitting progress.
//! It runs only after the authored single-rule pass, retaining its successful choices.
use super::{Choice, Control, Data, Instance, Measure, Placed, Xfrm, charge, choose, constraints, control_scope, range, score, trial};
use std::collections::{HashMap, HashSet};

fn root(parent: &mut [usize], mut i: usize, work: &mut usize) -> Result<usize, &'static str> {
    while parent[i] != i {
        charge(work, 1)?;
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    Ok(i)
}
fn groups(scopes: &[HashSet<usize>], work: &mut usize) -> Result<Vec<Vec<usize>>, &'static str> {
    let mut parent: Vec<_> = (0..scopes.len()).collect();
    let mut owner = HashMap::new();
    for (i, scope) in scopes.iter().enumerate() {
        charge(work, scope.len())?;
        for node in scope {
            if let Some(&j) = owner.get(node) {
                let a = root(&mut parent, i, work)?;
                let b = root(&mut parent, j, work)?;
                parent[a.max(b)] = a.min(b);
            } else {
                owner.insert(*node, i);
            }
        }
    }
    // Ordered map keeps component traversal deterministic, independent of hash seeds.
    let mut groups = std::collections::BTreeMap::<usize, Vec<usize>>::new();
    for i in 0..parent.len() {
        groups.entry(root(&mut parent, i, work)?).or_default().push(i);
    }
    Ok(groups.into_values().filter(|g| g.len() > 1).collect())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn search<'a>(
    tree: &Instance<'a>,
    frame: Xfrm,
    data: &Data<'a>,
    entries: &[constraints::Entry<'_, 'a>],
    controls: &[Control<'a>],
    measure: &mut Measure<'_>,
    changes: &mut constraints::Adjustments,
    out: &mut Vec<Placed<'a>>,
    work: &mut usize,
) -> Result<(), &'static str> {
    let all: HashSet<_> = out.iter().map(|p| p.node_key).collect();
    if controls.len() < 2 || score(out, &all, measure, false, work)? <= 1e-6 {
        return Ok(());
    }
    let solution = constraints::resolve_adjusted(tree, frame, data, work, changes)?;
    let mut ranges = Vec::new();
    let mut scopes = Vec::new();
    let mut seen = HashSet::new();
    let mut duplicate = HashSet::new();
    for c in controls {
        if let Some(r) = range(c, entries, &solution, changes)? {
            for key in &r.keys {
                if !seen.insert(key.clone()) {
                    duplicate.insert(key.clone());
                }
            }
            ranges.push(r);
            scopes.push(control_scope(entries, data, c, work)?);
        }
    }
    for group in groups(&scopes, work)? {
        // Repeated rules for one property express an ordered policy, not independent axes.
        if group.iter().any(|&i| ranges[i].keys.iter().any(|key| duplicate.contains(key))) {
            continue;
        }
        let scope = group.iter().flat_map(|&i| scopes[i].iter().copied()).collect();
        let before = score(out, &scope, measure, false, work)?;
        if before <= 1e-6 {
            continue;
        }
        let baseline = changes.clone();
        let mut probe = |t: f64| -> Result<Option<Choice<'a>>, &'static str> {
            charge(work, baseline.len() + group.len())?;
            changes.clone_from(&baseline);
            for &i in &group {
                ranges[i].set(changes, t);
            }
            let Some(candidate) = trial(tree, frame, data, changes, work)? else { return Ok(None) };
            // Simultaneous reference-factor changes can alter one another's absolute max.
            if group.iter().any(|&i| ranges[i].maximum.is_some()) {
                let solution = constraints::resolve_adjusted(tree, frame, data, work, changes)?;
                for &i in &group {
                    let r = &ranges[i];
                    if let Some(max) = r.maximum
                        && r.keys.iter().any(|key| solution.get(&key.0).and_then(|s| s.get(&key.1)).is_some_and(|v| v > max + 1e-6))
                    {
                        return Ok(None);
                    }
                }
            }
            let score = score(&candidate, &scope, measure, false, work)?;
            Ok(Some(Choice { out: candidate, score, t, low: 0. }))
        };
        let selected = choose(false, &mut probe)?.filter(|c| c.score < before - 1e-6);
        if let Some(mut best) = selected {
            if best.score <= 1e-6 {
                let (mut low, mut high) = (best.low, best.t);
                for _ in 0..18 {
                    let t = (low + high) / 2.;
                    if let Some(candidate) = probe(t)?.filter(|c| c.score <= 1e-6) {
                        high = t;
                        best = candidate;
                    } else {
                        low = t;
                    }
                }
            }
            changes.clone_from(&baseline);
            for &i in &group {
                ranges[i].set(changes, best.t);
            }
            *out = best.out;
        } else {
            changes.clone_from(&baseline);
        }
    }
    Ok(())
}
